-- USDT TRC-20 (TRON) on/off-ramp.
-- Liquidity rail: on-chain USDT backs internal USD checking (peg ≈ 1:1).
-- All deposit/withdraw pricing & postings live in SQL; Rust only watches chain / signs txs.

-- Extend account kinds for crypto float (USD claims issued against USDT reserves).
ALTER TABLE accounts DROP CONSTRAINT IF EXISTS accounts_kind_chk;
ALTER TABLE accounts ADD CONSTRAINT accounts_kind_chk CHECK (
    kind IN (
        'user_checking', 'system_float', 'system_clearing', 'system_suspense',
        'system_fx', 'system_fee', 'system_crypto_float'
    )
);

INSERT INTO accounts (owner_user_id, kind, currency, status)
SELECT NULL, 'system_crypto_float', 'USD', 'open'
WHERE NOT EXISTS (
    SELECT 1 FROM accounts WHERE kind = 'system_crypto_float' AND currency = 'USD'
);

-- Ensure USD fee sink exists
INSERT INTO accounts (owner_user_id, kind, currency, status)
SELECT NULL, 'system_fee', 'USD', 'open'
WHERE NOT EXISTS (
    SELECT 1 FROM accounts WHERE kind = 'system_fee' AND currency = 'USD'
);

-- Fee rule kinds for crypto (bps applied on USD leg)
ALTER TABLE fee_rules DROP CONSTRAINT IF EXISTS fee_kind_chk;
ALTER TABLE fee_rules ADD CONSTRAINT fee_kind_chk CHECK (
    kind IN ('p2p_transfer', 'fx_convert', 'crypto_deposit', 'crypto_withdraw')
);

INSERT INTO fee_rules (kind, currency, fixed_minor, bps, min_minor, max_minor)
SELECT 'crypto_deposit', 'USD', 0, 80, 1, NULL
WHERE NOT EXISTS (SELECT 1 FROM fee_rules WHERE kind = 'crypto_deposit');

INSERT INTO fee_rules (kind, currency, fixed_minor, bps, min_minor, max_minor)
SELECT 'crypto_withdraw', 'USD', 0, 120, 1, NULL
WHERE NOT EXISTS (SELECT 1 FROM fee_rules WHERE kind = 'crypto_withdraw');

-- ── Crypto settings (singleton) ────────────────────────────
CREATE TABLE IF NOT EXISTS crypto_settings (
    id INT PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    chain TEXT NOT NULL DEFAULT 'tron',
    asset TEXT NOT NULL DEFAULT 'USDT',
    usdt_contract TEXT NOT NULL DEFAULT 'TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t',
    -- Mid USDT→USD (normally 1). Admin may tweak if peg wobbles.
    usdt_usd_mid NUMERIC(24, 12) NOT NULL DEFAULT 1
        CHECK (usdt_usd_mid > 0),
    deposit_spread_bps INT NOT NULL DEFAULT 80
        CHECK (deposit_spread_bps >= 0 AND deposit_spread_bps <= 5000),
    withdraw_spread_bps INT NOT NULL DEFAULT 120
        CHECK (withdraw_spread_bps >= 0 AND withdraw_spread_bps <= 5000),
    -- USDT has 6 decimals
    min_deposit_usdt_minor BIGINT NOT NULL DEFAULT 1000000,  -- 1 USDT
    min_withdraw_usdt_minor BIGINT NOT NULL DEFAULT 5000000, -- 5 USDT
    confirmations_required INT NOT NULL DEFAULT 1
        CHECK (confirmations_required >= 0),
    hot_wallet_address TEXT,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_by UUID
);

INSERT INTO crypto_settings (id)
VALUES (1)
ON CONFLICT (id) DO NOTHING;

-- Per-user deposit addresses (unique TRON address per user)
CREATE TABLE IF NOT EXISTS crypto_deposit_addresses (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL,
    chain TEXT NOT NULL DEFAULT 'tron',
    address TEXT NOT NULL,
    derivation_index INT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT crypto_dep_addr_chain_chk CHECK (chain = 'tron')
);

CREATE UNIQUE INDEX IF NOT EXISTS crypto_dep_addr_user_uidx
    ON crypto_deposit_addresses (user_id) WHERE chain = 'tron';

CREATE UNIQUE INDEX IF NOT EXISTS crypto_dep_addr_address_uidx
    ON crypto_deposit_addresses (address);

CREATE UNIQUE INDEX IF NOT EXISTS crypto_dep_addr_index_uidx
    ON crypto_deposit_addresses (derivation_index);

CREATE SEQUENCE IF NOT EXISTS crypto_deposit_index_seq START 1;

-- Observed / credited deposits
CREATE TABLE IF NOT EXISTS crypto_deposits (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL,
    chain TEXT NOT NULL DEFAULT 'tron',
    tx_hash TEXT NOT NULL,
    from_address TEXT,
    to_address TEXT NOT NULL,
    usdt_minor BIGINT NOT NULL CHECK (usdt_minor > 0),
    usd_gross_minor BIGINT NOT NULL CHECK (usd_gross_minor > 0),
    usd_credit_minor BIGINT NOT NULL CHECK (usd_credit_minor >= 0),
    fee_minor BIGINT NOT NULL CHECK (fee_minor >= 0),
    mid NUMERIC(24, 12) NOT NULL,
    spread_bps INT NOT NULL,
    status TEXT NOT NULL DEFAULT 'detected'
        CHECK (status IN ('detected', 'credited', 'ignored', 'failed')),
    journal_entry_id UUID REFERENCES journal_entries (id),
    block_timestamp TIMESTAMPTZ,
    detected_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    credited_at TIMESTAMPTZ,
    error_message TEXT,
    CONSTRAINT crypto_dep_amounts_chk CHECK (usd_credit_minor + fee_minor = usd_gross_minor)
);

CREATE UNIQUE INDEX IF NOT EXISTS crypto_deposits_tx_uidx
    ON crypto_deposits (chain, tx_hash);

CREATE INDEX IF NOT EXISTS crypto_deposits_user_idx
    ON crypto_deposits (user_id, detected_at DESC);

-- Withdrawal queue
CREATE TABLE IF NOT EXISTS crypto_withdrawals (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL,
    chain TEXT NOT NULL DEFAULT 'tron',
    to_address TEXT NOT NULL,
    usd_debit_minor BIGINT NOT NULL CHECK (usd_debit_minor > 0),
    usdt_out_minor BIGINT NOT NULL CHECK (usdt_out_minor > 0),
    fee_minor BIGINT NOT NULL CHECK (fee_minor >= 0),
    mid NUMERIC(24, 12) NOT NULL,
    spread_bps INT NOT NULL,
    status TEXT NOT NULL DEFAULT 'queued'
        CHECK (status IN ('queued', 'broadcast', 'confirmed', 'failed', 'cancelled')),
    idempotency_key TEXT NOT NULL,
    journal_entry_id UUID REFERENCES journal_entries (id),
    tx_hash TEXT,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    broadcast_at TIMESTAMPTZ,
    confirmed_at TIMESTAMPTZ
);

CREATE UNIQUE INDEX IF NOT EXISTS crypto_withdrawals_idem_uidx
    ON crypto_withdrawals (idempotency_key);

CREATE INDEX IF NOT EXISTS crypto_withdrawals_status_idx
    ON crypto_withdrawals (status, created_at);

-- ── Pricing helpers ────────────────────────────────────────

CREATE OR REPLACE FUNCTION crypto_usdt_to_usd_gross(p_usdt_minor BIGINT, p_mid NUMERIC)
RETURNS BIGINT
LANGUAGE plpgsql
IMMUTABLE
AS $$
BEGIN
    -- USDT 6dp → USD 2dp: / 10_000
    IF p_usdt_minor <= 0 OR p_mid <= 0 THEN
        RETURN 0;
    END IF;
    RETURN round(p_usdt_minor::NUMERIC * p_mid / 10000)::BIGINT;
END;
$$;

CREATE OR REPLACE FUNCTION crypto_usd_to_usdt_gross(p_usd_minor BIGINT, p_mid NUMERIC)
RETURNS BIGINT
LANGUAGE plpgsql
IMMUTABLE
AS $$
BEGIN
    -- USD 2dp → USDT 6dp: * 10_000 / mid
    IF p_usd_minor <= 0 OR p_mid <= 0 THEN
        RETURN 0;
    END IF;
    RETURN round(p_usd_minor::NUMERIC * 10000 / p_mid)::BIGINT;
END;
$$;

CREATE OR REPLACE FUNCTION crypto_quote_deposit(p_usdt_minor BIGINT)
RETURNS TABLE (
    usdt_minor BIGINT,
    usd_gross_minor BIGINT,
    usd_credit_minor BIGINT,
    fee_minor BIGINT,
    mid NUMERIC,
    spread_bps INT,
    min_usdt_minor BIGINT
)
LANGUAGE plpgsql
STABLE
AS $$
DECLARE
    s crypto_settings%ROWTYPE;
    v_gross BIGINT;
    v_fee BIGINT;
BEGIN
    SELECT * INTO s FROM crypto_settings WHERE id = 1;
    IF NOT FOUND OR NOT s.enabled THEN
        RAISE EXCEPTION 'crypto rail disabled' USING ERRCODE = 'P0001';
    END IF;
    IF p_usdt_minor < s.min_deposit_usdt_minor THEN
        RAISE EXCEPTION 'below min deposit % USDT minor', s.min_deposit_usdt_minor
            USING ERRCODE = 'P0001';
    END IF;

    v_gross := crypto_usdt_to_usd_gross(p_usdt_minor, s.usdt_usd_mid);
    IF v_gross <= 0 THEN
        RAISE EXCEPTION 'deposit too small after conversion' USING ERRCODE = 'P0001';
    END IF;

    -- Prefer explicit crypto_settings spread; keep fee_rules as floor via calc_fee
    v_fee := GREATEST(
        calc_fee('crypto_deposit', 'USD', v_gross),
        ((v_gross * s.deposit_spread_bps + 5000) / 10000)
    );
    IF v_fee >= v_gross THEN
        RAISE EXCEPTION 'spread exceeds deposit' USING ERRCODE = 'P0001';
    END IF;

    usdt_minor := p_usdt_minor;
    usd_gross_minor := v_gross;
    fee_minor := v_fee;
    usd_credit_minor := v_gross - v_fee;
    mid := s.usdt_usd_mid;
    spread_bps := s.deposit_spread_bps;
    min_usdt_minor := s.min_deposit_usdt_minor;
    RETURN NEXT;
END;
$$;

CREATE OR REPLACE FUNCTION crypto_quote_withdraw(p_usd_minor BIGINT)
RETURNS TABLE (
    usd_debit_minor BIGINT,
    usdt_out_minor BIGINT,
    fee_minor BIGINT,
    mid NUMERIC,
    spread_bps INT,
    min_usdt_minor BIGINT
)
LANGUAGE plpgsql
STABLE
AS $$
DECLARE
    s crypto_settings%ROWTYPE;
    v_usdt_gross BIGINT;
    v_fee_usd BIGINT;
    v_net_usd BIGINT;
BEGIN
    SELECT * INTO s FROM crypto_settings WHERE id = 1;
    IF NOT FOUND OR NOT s.enabled THEN
        RAISE EXCEPTION 'crypto rail disabled' USING ERRCODE = 'P0001';
    END IF;
    IF p_usd_minor <= 0 THEN
        RAISE EXCEPTION 'amount must be positive' USING ERRCODE = 'P0001';
    END IF;

    v_fee_usd := GREATEST(
        calc_fee('crypto_withdraw', 'USD', p_usd_minor),
        ((p_usd_minor * s.withdraw_spread_bps + 5000) / 10000)
    );
    IF v_fee_usd >= p_usd_minor THEN
        RAISE EXCEPTION 'spread exceeds withdrawal' USING ERRCODE = 'P0001';
    END IF;
    v_net_usd := p_usd_minor - v_fee_usd;
    v_usdt_gross := crypto_usd_to_usdt_gross(v_net_usd, s.usdt_usd_mid);

    IF v_usdt_gross < s.min_withdraw_usdt_minor THEN
        RAISE EXCEPTION 'below min withdraw % USDT minor', s.min_withdraw_usdt_minor
            USING ERRCODE = 'P0001';
    END IF;

    usd_debit_minor := p_usd_minor;
    usdt_out_minor := v_usdt_gross;
    fee_minor := v_fee_usd;
    mid := s.usdt_usd_mid;
    spread_bps := s.withdraw_spread_bps;
    min_usdt_minor := s.min_withdraw_usdt_minor;
    RETURN NEXT;
END;
$$;

-- Credit user USD after confirmed on-chain USDT deposit (idempotent on tx_hash).
CREATE OR REPLACE FUNCTION crypto_credit_deposit(
    p_user UUID,
    p_tx_hash TEXT,
    p_from_address TEXT,
    p_to_address TEXT,
    p_usdt_minor BIGINT,
    p_block_ts TIMESTAMPTZ DEFAULT NULL
) RETURNS TABLE (
    deposit_id UUID,
    journal_entry_id UUID,
    usd_credit_minor BIGINT,
    fee_minor BIGINT,
    already_credited BOOLEAN
)
LANGUAGE plpgsql
AS $$
DECLARE
    v_q RECORD;
    v_dep crypto_deposits%ROWTYPE;
    v_user_acct UUID;
    v_float UUID;
    v_fee_acct UUID;
    v_entry UUID;
    v_key TEXT;
BEGIN
    IF p_tx_hash IS NULL OR length(trim(p_tx_hash)) < 8 THEN
        RAISE EXCEPTION 'tx_hash required' USING ERRCODE = 'P0001';
    END IF;

    SELECT * INTO v_dep
      FROM crypto_deposits
     WHERE chain = 'tron' AND tx_hash = p_tx_hash
     FOR UPDATE;

    IF FOUND THEN
        deposit_id := v_dep.id;
        journal_entry_id := v_dep.journal_entry_id;
        usd_credit_minor := v_dep.usd_credit_minor;
        fee_minor := v_dep.fee_minor;
        already_credited := (v_dep.status = 'credited');
        RETURN NEXT;
        RETURN;
    END IF;

    SELECT * INTO v_q FROM crypto_quote_deposit(p_usdt_minor);

    INSERT INTO crypto_deposits (
        user_id, chain, tx_hash, from_address, to_address,
        usdt_minor, usd_gross_minor, usd_credit_minor, fee_minor,
        mid, spread_bps, status, block_timestamp
    ) VALUES (
        p_user, 'tron', p_tx_hash, p_from_address, p_to_address,
        p_usdt_minor, v_q.usd_gross_minor, v_q.usd_credit_minor, v_q.fee_minor,
        v_q.mid, v_q.spread_bps, 'detected', p_block_ts
    )
    RETURNING * INTO v_dep;

    v_user_acct := ledger_ensure_checking(p_user, 'USD', 0);
    v_float := ledger_system_account('system_crypto_float', 'USD');
    v_fee_acct := ledger_system_account('system_fee', 'USD');
    v_key := 'crypto:dep:' || p_tx_hash;

    v_entry := ledger_post_transfer_with_fee(
        v_key,
        p_user,
        'USDT TRC-20 deposit',
        v_float,
        v_user_acct,
        v_fee_acct,
        v_q.usd_credit_minor,
        v_q.fee_minor,
        'USD'
    );

    UPDATE crypto_deposits
       SET status = 'credited',
           journal_entry_id = v_entry,
           credited_at = NOW()
     WHERE id = v_dep.id;

    deposit_id := v_dep.id;
    journal_entry_id := v_entry;
    usd_credit_minor := v_q.usd_credit_minor;
    fee_minor := v_q.fee_minor;
    already_credited := FALSE;
    RETURN NEXT;
END;
$$;

-- Queue withdrawal: debit USD (+ fee to system), enqueue USDT send.
CREATE OR REPLACE FUNCTION crypto_queue_withdraw(
    p_user UUID,
    p_usd_minor BIGINT,
    p_to_address TEXT,
    p_idempotency_key TEXT
) RETURNS TABLE (
    withdrawal_id UUID,
    journal_entry_id UUID,
    usdt_out_minor BIGINT,
    fee_minor BIGINT,
    usd_debit_minor BIGINT,
    status TEXT,
    idempotent_replay BOOLEAN
)
LANGUAGE plpgsql
AS $$
DECLARE
    v_q RECORD;
    v_w crypto_withdrawals%ROWTYPE;
    v_user_acct UUID;
    v_float UUID;
    v_fee_acct UUID;
    v_entry UUID;
    v_bal BIGINT;
    v_addr TEXT := trim(p_to_address);
BEGIN
    IF length(trim(p_idempotency_key)) = 0 OR length(p_idempotency_key) > 128 THEN
        RAISE EXCEPTION 'idempotency_key required' USING ERRCODE = 'P0001';
    END IF;
    IF v_addr IS NULL OR length(v_addr) < 30 OR length(v_addr) > 64 THEN
        RAISE EXCEPTION 'invalid TRON address' USING ERRCODE = 'P0001';
    END IF;
    IF left(v_addr, 1) <> 'T' THEN
        RAISE EXCEPTION 'TRON address must start with T' USING ERRCODE = 'P0001';
    END IF;

    SELECT * INTO v_w
      FROM crypto_withdrawals
     WHERE idempotency_key = p_idempotency_key
     FOR UPDATE;

    IF FOUND THEN
        withdrawal_id := v_w.id;
        journal_entry_id := v_w.journal_entry_id;
        usdt_out_minor := v_w.usdt_out_minor;
        fee_minor := v_w.fee_minor;
        usd_debit_minor := v_w.usd_debit_minor;
        status := v_w.status;
        idempotent_replay := TRUE;
        RETURN NEXT;
        RETURN;
    END IF;

    SELECT * INTO v_q FROM crypto_quote_withdraw(p_usd_minor);

    v_user_acct := ledger_checking_for_update(p_user, 'USD');
    v_bal := account_balance(v_user_acct);
    IF v_bal < v_q.usd_debit_minor THEN
        RAISE EXCEPTION 'insufficient USD balance' USING ERRCODE = 'P0001';
    END IF;

    v_float := ledger_system_account('system_crypto_float', 'USD');
    v_fee_acct := ledger_system_account('system_fee', 'USD');

    -- User pays full USD; net USDT value returns to crypto float; fee → income
    v_entry := ledger_post_transfer_with_fee(
        'crypto:wd:' || p_idempotency_key,
        p_user,
        'USDT TRC-20 withdraw',
        v_user_acct,
        v_float,
        v_fee_acct,
        v_q.usd_debit_minor - v_q.fee_minor,
        v_q.fee_minor,
        'USD'
    );

    INSERT INTO crypto_withdrawals (
        user_id, chain, to_address,
        usd_debit_minor, usdt_out_minor, fee_minor,
        mid, spread_bps, status, idempotency_key, journal_entry_id
    ) VALUES (
        p_user, 'tron', v_addr,
        v_q.usd_debit_minor, v_q.usdt_out_minor, v_q.fee_minor,
        v_q.mid, v_q.spread_bps, 'queued', p_idempotency_key, v_entry
    )
    RETURNING * INTO v_w;

    withdrawal_id := v_w.id;
    journal_entry_id := v_entry;
    usdt_out_minor := v_w.usdt_out_minor;
    fee_minor := v_w.fee_minor;
    usd_debit_minor := v_w.usd_debit_minor;
    status := v_w.status;
    idempotent_replay := FALSE;
    RETURN NEXT;
END;
$$;

CREATE OR REPLACE FUNCTION crypto_mark_withdraw_broadcast(
    p_id UUID,
    p_tx_hash TEXT
) RETURNS VOID
LANGUAGE plpgsql
AS $$
BEGIN
    UPDATE crypto_withdrawals
       SET status = 'broadcast',
           tx_hash = p_tx_hash,
           broadcast_at = NOW(),
           updated_at = NOW()
     WHERE id = p_id AND status = 'queued';
    IF NOT FOUND THEN
        RAISE EXCEPTION 'withdrawal not queued' USING ERRCODE = 'P0001';
    END IF;
END;
$$;

CREATE OR REPLACE FUNCTION crypto_mark_withdraw_failed(
    p_id UUID,
    p_error TEXT
) RETURNS VOID
LANGUAGE plpgsql
AS $$
BEGIN
    UPDATE crypto_withdrawals
       SET status = 'failed',
           error_message = left(COALESCE(p_error, 'failed'), 500),
           updated_at = NOW()
     WHERE id = p_id AND status IN ('queued', 'broadcast');
END;
$$;

CREATE OR REPLACE FUNCTION crypto_mark_withdraw_confirmed(p_id UUID)
RETURNS VOID
LANGUAGE plpgsql
AS $$
BEGIN
    UPDATE crypto_withdrawals
       SET status = 'confirmed',
           confirmed_at = NOW(),
           updated_at = NOW()
     WHERE id = p_id AND status IN ('broadcast', 'queued');
END;
$$;
