-- Russian bank primary: RUB checking + USDT on/off-ramp credits RUB.

-- Welcome bonus on RUB (not EUR)
CREATE OR REPLACE FUNCTION ledger_ensure_checking(
    p_user UUID,
    p_currency CHAR(3),
    p_welcome_minor BIGINT DEFAULT 0
) RETURNS UUID
LANGUAGE plpgsql
AS $$
DECLARE
    v_id UUID;
    v_ccy CHAR(3) := upper(p_currency);
    v_float UUID;
    v_key TEXT;
BEGIN
    SELECT id INTO v_id
      FROM accounts
     WHERE owner_user_id = p_user
       AND kind = 'user_checking'
       AND currency = v_ccy
       AND status = 'open'
     FOR UPDATE;

    IF v_id IS NOT NULL THEN
        RETURN v_id;
    END IF;

    INSERT INTO accounts (owner_user_id, kind, currency, status)
    VALUES (p_user, 'user_checking', v_ccy, 'open')
    RETURNING id INTO v_id;

    IF p_welcome_minor > 0 AND v_ccy = 'RUB' THEN
        v_float := ledger_system_account('system_float', v_ccy);
        v_key := 'welcome:' || p_user::text || ':' || v_ccy;
        IF NOT EXISTS (SELECT 1 FROM journal_entries WHERE idempotency_key = v_key) THEN
            PERFORM ledger_post_two(
                v_key, p_user, 'Приветственный бонус',
                v_float, v_id, p_welcome_minor, v_ccy
            );
        END IF;
    END IF;

    RETURN v_id;
END;
$$;

-- System accounts for RUB crypto float / fee
INSERT INTO accounts (owner_user_id, kind, currency, status)
SELECT NULL, 'system_crypto_float', 'RUB', 'open'
WHERE NOT EXISTS (
    SELECT 1 FROM accounts WHERE kind = 'system_crypto_float' AND currency = 'RUB'
);

INSERT INTO accounts (owner_user_id, kind, currency, status)
SELECT NULL, 'system_fee', 'RUB', 'open'
WHERE NOT EXISTS (
    SELECT 1 FROM accounts WHERE kind = 'system_fee' AND currency = 'RUB'
);

INSERT INTO accounts (owner_user_id, kind, currency, status)
SELECT NULL, 'system_float', 'RUB', 'open'
WHERE NOT EXISTS (
    SELECT 1 FROM accounts WHERE kind = 'system_float' AND currency = 'RUB'
);

-- Fee rules in RUB
UPDATE fee_rules SET currency = 'RUB' WHERE kind IN ('crypto_deposit', 'crypto_withdraw');

-- Rename deposit/withdraw fiat columns to RUB semantics (amounts are kopecks)
ALTER TABLE crypto_deposits RENAME COLUMN usd_gross_minor TO rub_gross_minor;
ALTER TABLE crypto_deposits RENAME COLUMN usd_credit_minor TO rub_credit_minor;
ALTER TABLE crypto_withdrawals RENAME COLUMN usd_debit_minor TO rub_debit_minor;

-- Drop old quote/credit functions (signature change)
DROP FUNCTION IF EXISTS crypto_quote_deposit(BIGINT);
DROP FUNCTION IF EXISTS crypto_quote_withdraw(BIGINT);
DROP FUNCTION IF EXISTS crypto_credit_deposit(UUID, TEXT, TEXT, TEXT, BIGINT, TIMESTAMPTZ);
DROP FUNCTION IF EXISTS crypto_queue_withdraw(UUID, BIGINT, TEXT, TEXT);

CREATE OR REPLACE FUNCTION crypto_quote_deposit(p_usdt_minor BIGINT)
RETURNS TABLE (
    usdt_minor BIGINT,
    rub_gross_minor BIGINT,
    rub_credit_minor BIGINT,
    fee_minor BIGINT,
    usdt_usd_mid NUMERIC,
    usd_rub_mid NUMERIC,
    spread_bps INT,
    min_usdt_minor BIGINT
)
LANGUAGE plpgsql
STABLE
AS $$
DECLARE
    s crypto_settings%ROWTYPE;
    v_usd BIGINT;
    v_usd_rub NUMERIC;
    v_gross BIGINT;
    v_fee BIGINT;
    v_as_of DATE;
BEGIN
    SELECT * INTO s FROM crypto_settings WHERE id = 1;
    IF NOT FOUND OR NOT s.enabled THEN
        RAISE EXCEPTION 'crypto rail disabled' USING ERRCODE = 'P0001';
    END IF;
    IF p_usdt_minor < s.min_deposit_usdt_minor THEN
        RAISE EXCEPTION 'ниже минимума пополнения' USING ERRCODE = 'P0001';
    END IF;

    v_usd := crypto_usdt_to_usd_gross(p_usdt_minor, s.usdt_usd_mid);
    IF v_usd <= 0 THEN
        RAISE EXCEPTION 'сумма слишком мала' USING ERRCODE = 'P0001';
    END IF;

    SELECT mid, as_of INTO v_usd_rub, v_as_of FROM fx_mid_rate('USD', 'RUB');
    IF v_usd_rub IS NULL OR v_usd_rub <= 0 THEN
        RAISE EXCEPTION 'нет курса USD/RUB — обновите курсы ЦБ' USING ERRCODE = 'P0001';
    END IF;

    -- usd cents * RUB per USD = rub kopecks
    v_gross := round(v_usd::NUMERIC * v_usd_rub)::BIGINT;
    IF v_gross <= 0 THEN
        RAISE EXCEPTION 'сумма слишком мала после конвертации' USING ERRCODE = 'P0001';
    END IF;

    v_fee := GREATEST(
        calc_fee('crypto_deposit', 'RUB', v_gross),
        ((v_gross * s.deposit_spread_bps + 5000) / 10000)
    );
    IF v_fee >= v_gross THEN
        RAISE EXCEPTION 'комиссия превышает сумму' USING ERRCODE = 'P0001';
    END IF;

    usdt_minor := p_usdt_minor;
    rub_gross_minor := v_gross;
    fee_minor := v_fee;
    rub_credit_minor := v_gross - v_fee;
    usdt_usd_mid := s.usdt_usd_mid;
    usd_rub_mid := v_usd_rub;
    spread_bps := s.deposit_spread_bps;
    min_usdt_minor := s.min_deposit_usdt_minor;
    RETURN NEXT;
END;
$$;

CREATE OR REPLACE FUNCTION crypto_quote_withdraw(p_rub_minor BIGINT)
RETURNS TABLE (
    rub_debit_minor BIGINT,
    usdt_out_minor BIGINT,
    fee_minor BIGINT,
    usdt_usd_mid NUMERIC,
    usd_rub_mid NUMERIC,
    spread_bps INT,
    min_usdt_minor BIGINT
)
LANGUAGE plpgsql
STABLE
AS $$
DECLARE
    s crypto_settings%ROWTYPE;
    v_usd_rub NUMERIC;
    v_fee BIGINT;
    v_net BIGINT;
    v_usd BIGINT;
    v_usdt BIGINT;
BEGIN
    SELECT * INTO s FROM crypto_settings WHERE id = 1;
    IF NOT FOUND OR NOT s.enabled THEN
        RAISE EXCEPTION 'crypto rail disabled' USING ERRCODE = 'P0001';
    END IF;
    IF p_rub_minor <= 0 THEN
        RAISE EXCEPTION 'сумма должна быть положительной' USING ERRCODE = 'P0001';
    END IF;

    SELECT mid INTO v_usd_rub FROM fx_mid_rate('USD', 'RUB');
    IF v_usd_rub IS NULL OR v_usd_rub <= 0 THEN
        RAISE EXCEPTION 'нет курса USD/RUB — обновите курсы ЦБ' USING ERRCODE = 'P0001';
    END IF;

    v_fee := GREATEST(
        calc_fee('crypto_withdraw', 'RUB', p_rub_minor),
        ((p_rub_minor * s.withdraw_spread_bps + 5000) / 10000)
    );
    IF v_fee >= p_rub_minor THEN
        RAISE EXCEPTION 'комиссия превышает сумму' USING ERRCODE = 'P0001';
    END IF;
    v_net := p_rub_minor - v_fee;
    v_usd := round(v_net::NUMERIC / v_usd_rub)::BIGINT;
    IF v_usd <= 0 THEN
        RAISE EXCEPTION 'сумма слишком мала' USING ERRCODE = 'P0001';
    END IF;
    v_usdt := crypto_usd_to_usdt_gross(v_usd, s.usdt_usd_mid);
    IF v_usdt < s.min_withdraw_usdt_minor THEN
        RAISE EXCEPTION 'ниже минимума вывода' USING ERRCODE = 'P0001';
    END IF;

    rub_debit_minor := p_rub_minor;
    usdt_out_minor := v_usdt;
    fee_minor := v_fee;
    usdt_usd_mid := s.usdt_usd_mid;
    usd_rub_mid := v_usd_rub;
    spread_bps := s.withdraw_spread_bps;
    min_usdt_minor := s.min_withdraw_usdt_minor;
    RETURN NEXT;
END;
$$;

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
    rub_credit_minor BIGINT,
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
        rub_credit_minor := v_dep.rub_credit_minor;
        fee_minor := v_dep.fee_minor;
        already_credited := (v_dep.status = 'credited');
        RETURN NEXT;
        RETURN;
    END IF;

    SELECT * INTO v_q FROM crypto_quote_deposit(p_usdt_minor);

    INSERT INTO crypto_deposits (
        user_id, chain, tx_hash, from_address, to_address,
        usdt_minor, rub_gross_minor, rub_credit_minor, fee_minor,
        mid, spread_bps, status, block_timestamp
    ) VALUES (
        p_user, 'tron', p_tx_hash, p_from_address, p_to_address,
        p_usdt_minor, v_q.rub_gross_minor, v_q.rub_credit_minor, v_q.fee_minor,
        v_q.usd_rub_mid, v_q.spread_bps, 'detected', p_block_ts
    )
    RETURNING * INTO v_dep;

    v_user_acct := ledger_ensure_checking(p_user, 'RUB', 0);
    v_float := ledger_system_account('system_crypto_float', 'RUB');
    v_fee_acct := ledger_system_account('system_fee', 'RUB');
    v_key := 'crypto:dep:' || p_tx_hash;

    v_entry := ledger_post_transfer_with_fee(
        v_key,
        p_user,
        'Пополнение USDT TRC-20',
        v_float,
        v_user_acct,
        v_fee_acct,
        v_q.rub_credit_minor,
        v_q.fee_minor,
        'RUB'
    );

    UPDATE crypto_deposits
       SET status = 'credited',
           journal_entry_id = v_entry,
           credited_at = NOW()
     WHERE id = v_dep.id;

    deposit_id := v_dep.id;
    journal_entry_id := v_entry;
    rub_credit_minor := v_q.rub_credit_minor;
    fee_minor := v_q.fee_minor;
    already_credited := FALSE;
    RETURN NEXT;
END;
$$;

CREATE OR REPLACE FUNCTION crypto_queue_withdraw(
    p_user UUID,
    p_rub_minor BIGINT,
    p_to_address TEXT,
    p_idempotency_key TEXT
) RETURNS TABLE (
    withdrawal_id UUID,
    journal_entry_id UUID,
    usdt_out_minor BIGINT,
    fee_minor BIGINT,
    rub_debit_minor BIGINT,
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
        RAISE EXCEPTION 'некорректный адрес TRON' USING ERRCODE = 'P0001';
    END IF;
    IF left(v_addr, 1) <> 'T' THEN
        RAISE EXCEPTION 'адрес TRON должен начинаться с T' USING ERRCODE = 'P0001';
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
        rub_debit_minor := v_w.rub_debit_minor;
        status := v_w.status;
        idempotent_replay := TRUE;
        RETURN NEXT;
        RETURN;
    END IF;

    SELECT * INTO v_q FROM crypto_quote_withdraw(p_rub_minor);

    v_user_acct := ledger_checking_for_update(p_user, 'RUB');
    v_bal := account_balance(v_user_acct);
    IF v_bal < v_q.rub_debit_minor THEN
        RAISE EXCEPTION 'недостаточно средств на RUB-счёте' USING ERRCODE = 'P0001';
    END IF;

    v_float := ledger_system_account('system_crypto_float', 'RUB');
    v_fee_acct := ledger_system_account('system_fee', 'RUB');

    v_entry := ledger_post_transfer_with_fee(
        'crypto:wd:' || p_idempotency_key,
        p_user,
        'Вывод USDT TRC-20',
        v_user_acct,
        v_float,
        v_fee_acct,
        v_q.rub_debit_minor - v_q.fee_minor,
        v_q.fee_minor,
        'RUB'
    );

    INSERT INTO crypto_withdrawals (
        user_id, chain, to_address,
        rub_debit_minor, usdt_out_minor, fee_minor,
        mid, spread_bps, status, idempotency_key, journal_entry_id
    ) VALUES (
        p_user, 'tron', v_addr,
        v_q.rub_debit_minor, v_q.usdt_out_minor, v_q.fee_minor,
        v_q.usd_rub_mid, v_q.spread_bps, 'queued', p_idempotency_key, v_entry
    )
    RETURNING * INTO v_w;

    withdrawal_id := v_w.id;
    journal_entry_id := v_entry;
    usdt_out_minor := v_w.usdt_out_minor;
    fee_minor := v_w.fee_minor;
    rub_debit_minor := v_w.rub_debit_minor;
    status := v_w.status;
    idempotent_replay := FALSE;
    RETURN NEXT;
END;
$$;
