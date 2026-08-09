-- Ledger / FX / fee engine in PostgreSQL (bank core).
-- Application code must call these functions; do not reimplement math in Rust.

ALTER TABLE accounts DROP CONSTRAINT IF EXISTS accounts_kind_chk;
ALTER TABLE accounts ADD CONSTRAINT accounts_kind_chk CHECK (
    kind IN ('user_checking', 'system_float', 'system_clearing', 'system_suspense', 'system_fx', 'system_fee')
);

ALTER TABLE accounts DROP CONSTRAINT IF EXISTS accounts_owner_chk;
ALTER TABLE accounts ADD CONSTRAINT accounts_owner_chk CHECK (
    (kind = 'user_checking' AND owner_user_id IS NOT NULL)
    OR (kind <> 'user_checking' AND owner_user_id IS NULL)
);

-- ── Fee schedule ───────────────────────────────────────────
CREATE TABLE IF NOT EXISTS fee_rules (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    kind TEXT NOT NULL,
    currency CHAR(3), -- NULL = any
    fixed_minor BIGINT NOT NULL DEFAULT 0 CHECK (fixed_minor >= 0),
    bps INT NOT NULL DEFAULT 0 CHECK (bps >= 0 AND bps <= 10000),
    min_minor BIGINT NOT NULL DEFAULT 0 CHECK (min_minor >= 0),
    max_minor BIGINT CHECK (max_minor IS NULL OR max_minor >= min_minor),
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT fee_kind_chk CHECK (kind IN ('p2p_transfer', 'fx_convert'))
);

CREATE UNIQUE INDEX IF NOT EXISTS fee_rules_kind_ccy_uidx
    ON fee_rules (kind, COALESCE(currency, '***'));

INSERT INTO fee_rules (kind, currency, fixed_minor, bps, min_minor, max_minor)
SELECT 'p2p_transfer', NULL, 0, 0, 0, NULL
WHERE NOT EXISTS (SELECT 1 FROM fee_rules WHERE kind = 'p2p_transfer' AND currency IS NULL);

INSERT INTO fee_rules (kind, currency, fixed_minor, bps, min_minor, max_minor)
SELECT 'fx_convert', NULL, 0, 0, 0, NULL
WHERE NOT EXISTS (SELECT 1 FROM fee_rules WHERE kind = 'fx_convert' AND currency IS NULL);

-- System fee sinks per currency
INSERT INTO accounts (owner_user_id, kind, currency, status)
SELECT NULL, 'system_fee', ccy, 'open'
FROM (VALUES ('EUR'), ('USD'), ('RUB')) AS t(ccy)
WHERE NOT EXISTS (
    SELECT 1 FROM accounts a WHERE a.kind = 'system_fee' AND a.currency = t.ccy
);

ALTER TABLE fx_conversions
    ADD COLUMN IF NOT EXISTS to_entry_id UUID REFERENCES journal_entries (id),
    ADD COLUMN IF NOT EXISTS fee_minor BIGINT NOT NULL DEFAULT 0;

-- ── Helpers ────────────────────────────────────────────────

CREATE OR REPLACE FUNCTION ledger_system_account(p_kind TEXT, p_currency CHAR(3))
RETURNS UUID
LANGUAGE plpgsql
AS $$
DECLARE
    v_id UUID;
BEGIN
    SELECT id INTO v_id
      FROM accounts
     WHERE kind = p_kind AND currency = upper(p_currency) AND status = 'open'
     FOR UPDATE;
    IF v_id IS NULL THEN
        RAISE EXCEPTION 'missing system account %/%', p_kind, p_currency
            USING ERRCODE = 'P0001';
    END IF;
    RETURN v_id;
END;
$$;

CREATE OR REPLACE FUNCTION ledger_checking_for_update(p_user UUID, p_currency CHAR(3))
RETURNS UUID
LANGUAGE plpgsql
AS $$
DECLARE
    v_id UUID;
BEGIN
    SELECT id INTO v_id
      FROM accounts
     WHERE owner_user_id = p_user
       AND kind = 'user_checking'
       AND currency = upper(p_currency)
       AND status = 'open'
     FOR UPDATE;
    IF v_id IS NULL THEN
        RAISE EXCEPTION 'no open % checking for user %', p_currency, p_user
            USING ERRCODE = 'P0001';
    END IF;
    RETURN v_id;
END;
$$;

CREATE OR REPLACE FUNCTION calc_fee(
    p_kind TEXT,
    p_currency CHAR(3),
    p_amount_minor BIGINT
) RETURNS BIGINT
LANGUAGE plpgsql
STABLE
AS $$
DECLARE
    r fee_rules%ROWTYPE;
    fee BIGINT;
BEGIN
    IF p_amount_minor <= 0 THEN
        RETURN 0;
    END IF;

    SELECT * INTO r
      FROM fee_rules
     WHERE enabled
       AND kind = p_kind
       AND (currency IS NULL OR currency = upper(p_currency))
     ORDER BY currency NULLS LAST
     LIMIT 1;

    IF NOT FOUND THEN
        RETURN 0;
    END IF;

    fee := r.fixed_minor + ((p_amount_minor * r.bps + 5000) / 10000);
    IF fee < r.min_minor THEN
        fee := r.min_minor;
    END IF;
    IF r.max_minor IS NOT NULL AND fee > r.max_minor THEN
        fee := r.max_minor;
    END IF;
    RETURN fee;
END;
$$;

-- Post 2-leg balanced entry (debit from_acct, credit to_acct).
CREATE OR REPLACE FUNCTION ledger_post_two(
    p_idempotency_key TEXT,
    p_created_by UUID,
    p_memo TEXT,
    p_from_acct UUID,
    p_to_acct UUID,
    p_amount_minor BIGINT,
    p_currency CHAR(3)
) RETURNS UUID
LANGUAGE plpgsql
AS $$
DECLARE
    v_entry UUID;
    v_ccy CHAR(3) := upper(p_currency);
BEGIN
    IF p_amount_minor <= 0 THEN
        RAISE EXCEPTION 'amount must be positive' USING ERRCODE = 'P0001';
    END IF;
    IF p_from_acct = p_to_acct THEN
        RAISE EXCEPTION 'same account' USING ERRCODE = 'P0001';
    END IF;

    INSERT INTO journal_entries (idempotency_key, created_by, memo, status)
    VALUES (p_idempotency_key, p_created_by, COALESCE(p_memo, ''), 'posted')
    RETURNING id INTO v_entry;

    INSERT INTO postings (entry_id, account_id, amount_minor, currency)
    VALUES
        (v_entry, p_from_acct, -p_amount_minor, v_ccy),
        (v_entry, p_to_acct,    p_amount_minor, v_ccy);

    PERFORM validate_journal_entry(v_entry);
    RETURN v_entry;
END;
$$;

-- Post transfer with optional fee: sender pays amount+fee, recipient gets amount, fee → system_fee.
CREATE OR REPLACE FUNCTION ledger_post_transfer_with_fee(
    p_idempotency_key TEXT,
    p_created_by UUID,
    p_memo TEXT,
    p_from_acct UUID,
    p_to_acct UUID,
    p_fee_acct UUID,
    p_amount_minor BIGINT,
    p_fee_minor BIGINT,
    p_currency CHAR(3)
) RETURNS UUID
LANGUAGE plpgsql
AS $$
DECLARE
    v_entry UUID;
    v_ccy CHAR(3) := upper(p_currency);
    v_total BIGINT;
BEGIN
    IF p_amount_minor <= 0 THEN
        RAISE EXCEPTION 'amount must be positive' USING ERRCODE = 'P0001';
    END IF;
    IF p_fee_minor < 0 THEN
        RAISE EXCEPTION 'fee must be non-negative' USING ERRCODE = 'P0001';
    END IF;
    IF p_from_acct = p_to_acct THEN
        RAISE EXCEPTION 'same account' USING ERRCODE = 'P0001';
    END IF;

    v_total := p_amount_minor + p_fee_minor;

    INSERT INTO journal_entries (idempotency_key, created_by, memo, status)
    VALUES (p_idempotency_key, p_created_by, COALESCE(p_memo, ''), 'posted')
    RETURNING id INTO v_entry;

    IF p_fee_minor = 0 THEN
        INSERT INTO postings (entry_id, account_id, amount_minor, currency)
        VALUES
            (v_entry, p_from_acct, -p_amount_minor, v_ccy),
            (v_entry, p_to_acct,    p_amount_minor, v_ccy);
    ELSE
        INSERT INTO postings (entry_id, account_id, amount_minor, currency)
        VALUES
            (v_entry, p_from_acct, -v_total, v_ccy),
            (v_entry, p_to_acct,    p_amount_minor, v_ccy),
            (v_entry, p_fee_acct,   p_fee_minor, v_ccy);
    END IF;

    PERFORM validate_journal_entry(v_entry);
    RETURN v_entry;
END;
$$;

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

    IF p_welcome_minor > 0 AND v_ccy = 'EUR' THEN
        v_float := ledger_system_account('system_float', v_ccy);
        v_key := 'welcome:' || p_user::text || ':' || v_ccy;
        IF NOT EXISTS (SELECT 1 FROM journal_entries WHERE idempotency_key = v_key) THEN
            PERFORM ledger_post_two(
                v_key, p_user, 'Welcome credit',
                v_float, v_id, p_welcome_minor, v_ccy
            );
        END IF;
    END IF;

    RETURN v_id;
END;
$$;

CREATE OR REPLACE FUNCTION ledger_transfer_p2p(
    p_from_user UUID,
    p_to_user UUID,
    p_amount_minor BIGINT,
    p_currency CHAR(3),
    p_idempotency_key TEXT,
    p_memo TEXT,
    p_created_by UUID
) RETURNS TABLE (
    entry_id UUID,
    idempotent_replay BOOLEAN,
    from_balance_minor BIGINT,
    to_balance_minor BIGINT,
    fee_minor BIGINT
)
LANGUAGE plpgsql
AS $$
DECLARE
    v_ccy CHAR(3) := upper(p_currency);
    v_from UUID;
    v_to UUID;
    v_fee_acct UUID;
    v_fee BIGINT;
    v_entry UUID;
    v_u1 UUID;
    v_u2 UUID;
    v_a UUID;
    v_b UUID;
    v_bal BIGINT;
BEGIN
    IF p_amount_minor <= 0 THEN
        RAISE EXCEPTION 'amount must be positive' USING ERRCODE = 'P0001';
    END IF;
    IF length(trim(p_idempotency_key)) = 0 OR length(p_idempotency_key) > 128 THEN
        RAISE EXCEPTION 'idempotency_key required (1–128 chars)' USING ERRCODE = 'P0001';
    END IF;
    IF p_from_user = p_to_user THEN
        RAISE EXCEPTION 'cannot transfer to yourself' USING ERRCODE = 'P0001';
    END IF;

    -- Idempotent replay
    SELECT je.id INTO v_entry
      FROM journal_entries je
     WHERE je.idempotency_key = p_idempotency_key;

    IF v_entry IS NOT NULL THEN
        v_from := ledger_checking_for_update(p_from_user, v_ccy);
        v_to := ledger_checking_for_update(p_to_user, v_ccy);
        entry_id := v_entry;
        idempotent_replay := TRUE;
        from_balance_minor := account_balance(v_from);
        to_balance_minor := account_balance(v_to);
        fee_minor := COALESCE(
            (SELECT fc.fee_minor FROM fx_conversions fc WHERE fc.entry_id = v_entry LIMIT 1),
            0
        );
        -- fee may not be in fx_conversions; store 0 on replay for p2p
        fee_minor := 0;
        RETURN NEXT;
        RETURN;
    END IF;

    -- Lock users' accounts in UUID order
    IF p_from_user < p_to_user THEN
        v_u1 := p_from_user; v_u2 := p_to_user;
    ELSE
        v_u1 := p_to_user; v_u2 := p_from_user;
    END IF;
    v_a := ledger_checking_for_update(v_u1, v_ccy);
    v_b := ledger_checking_for_update(v_u2, v_ccy);
    IF p_from_user < p_to_user THEN
        v_from := v_a; v_to := v_b;
    ELSE
        v_from := v_b; v_to := v_a;
    END IF;

    v_fee := calc_fee('p2p_transfer', v_ccy, p_amount_minor);
    v_fee_acct := ledger_system_account('system_fee', v_ccy);

    v_bal := account_balance(v_from);
    IF v_bal < (p_amount_minor + v_fee) THEN
        RAISE EXCEPTION 'insufficient funds' USING ERRCODE = 'P0001';
    END IF;

    v_entry := ledger_post_transfer_with_fee(
        p_idempotency_key,
        p_created_by,
        COALESCE(NULLIF(trim(p_memo), ''), 'P2P transfer'),
        v_from,
        v_to,
        v_fee_acct,
        p_amount_minor,
        v_fee,
        v_ccy
    );

    entry_id := v_entry;
    idempotent_replay := FALSE;
    from_balance_minor := account_balance(v_from);
    to_balance_minor := account_balance(v_to);
    fee_minor := v_fee;
    RETURN NEXT;
END;
$$;

-- ── FX in SQL ──────────────────────────────────────────────

CREATE OR REPLACE FUNCTION fx_latest_rub(p_base CHAR(3))
RETURNS TABLE (mid NUMERIC, as_of DATE)
LANGUAGE plpgsql
STABLE
AS $$
DECLARE
    v_base CHAR(3) := upper(p_base);
BEGIN
    IF v_base = 'RUB' THEN
        mid := 1;
        as_of := CURRENT_DATE;
        RETURN NEXT;
        RETURN;
    END IF;

    RETURN QUERY
    SELECT r.mid, r.as_of
      FROM fx_rates r
     WHERE r.base = v_base AND r.quote = 'RUB'
     ORDER BY r.as_of DESC
     LIMIT 1;
END;
$$;

CREATE OR REPLACE FUNCTION fx_mid_rate(p_base CHAR(3), p_quote CHAR(3))
RETURNS TABLE (mid NUMERIC, as_of DATE)
LANGUAGE plpgsql
STABLE
AS $$
DECLARE
    v_base CHAR(3) := upper(p_base);
    v_quote CHAR(3) := upper(p_quote);
    b_mid NUMERIC;
    q_mid NUMERIC;
    b_as DATE;
    q_as DATE;
BEGIN
    IF v_base = v_quote THEN
        mid := 1;
        as_of := CURRENT_DATE;
        RETURN NEXT;
        RETURN;
    END IF;

    IF v_quote = 'RUB' THEN
        SELECT lr.mid, lr.as_of INTO b_mid, b_as FROM fx_latest_rub(v_base) lr;
        IF b_mid IS NULL THEN
            RAISE EXCEPTION 'no CBR mid for %', v_base USING ERRCODE = 'P0001';
        END IF;
        mid := b_mid;
        as_of := b_as;
        RETURN NEXT;
        RETURN;
    END IF;

    IF v_base = 'RUB' THEN
        SELECT lr.mid, lr.as_of INTO q_mid, q_as FROM fx_latest_rub(v_quote) lr;
        IF q_mid IS NULL OR q_mid = 0 THEN
            RAISE EXCEPTION 'no CBR mid for %', v_quote USING ERRCODE = 'P0001';
        END IF;
        mid := 1 / q_mid;
        as_of := q_as;
        RETURN NEXT;
        RETURN;
    END IF;

    SELECT lr.mid, lr.as_of INTO b_mid, b_as FROM fx_latest_rub(v_base) lr;
    SELECT lr.mid, lr.as_of INTO q_mid, q_as FROM fx_latest_rub(v_quote) lr;
    IF b_mid IS NULL OR q_mid IS NULL OR q_mid = 0 THEN
        RAISE EXCEPTION 'no cross rate for %/%', v_base, v_quote USING ERRCODE = 'P0001';
    END IF;
    mid := b_mid / q_mid;
    as_of := LEAST(b_as, q_as);
    RETURN NEXT;
END;
$$;

CREATE OR REPLACE FUNCTION fx_apply_margin_spread(
    p_mid NUMERIC,
    p_margin_bps INT,
    p_spread_bps INT
) RETURNS TABLE (buy NUMERIC, sell NUMERIC)
LANGUAGE plpgsql
IMMUTABLE
AS $$
DECLARE
    margined NUMERIC;
    half_spread NUMERIC;
BEGIN
    margined := p_mid * (1 + (p_margin_bps::NUMERIC / 10000));
    half_spread := (p_spread_bps::NUMERIC / 10000) / 2;
    buy := margined * (1 - half_spread);
    sell := margined * (1 + half_spread);
    IF buy <= 0 OR sell <= 0 THEN
        RAISE EXCEPTION 'invalid quote' USING ERRCODE = 'P0001';
    END IF;
    RETURN NEXT;
END;
$$;

CREATE OR REPLACE FUNCTION fx_quote_pair(p_base CHAR(3), p_quote CHAR(3))
RETURNS TABLE (
    base CHAR(3),
    quote CHAR(3),
    mid NUMERIC,
    buy NUMERIC,
    sell NUMERIC,
    margin_bps INT,
    spread_bps INT,
    as_of DATE
)
LANGUAGE plpgsql
STABLE
AS $$
DECLARE
    s fx_settings%ROWTYPE;
    m NUMERIC;
    d DATE;
    b NUMERIC;
    sel NUMERIC;
BEGIN
    SELECT * INTO s FROM fx_settings WHERE id = 1;
    SELECT mr.mid, mr.as_of INTO m, d FROM fx_mid_rate(p_base, p_quote) mr;
    SELECT q.buy, q.sell INTO b, sel FROM fx_apply_margin_spread(m, s.margin_bps, s.spread_bps) q;

    base := upper(p_base);
    quote := upper(p_quote);
    mid := m;
    buy := b;
    sell := sel;
    margin_bps := s.margin_bps;
    spread_bps := s.spread_bps;
    as_of := d;
    RETURN NEXT;
END;
$$;

CREATE OR REPLACE FUNCTION fx_convert(
    p_user UUID,
    p_from_ccy CHAR(3),
    p_to_ccy CHAR(3),
    p_from_amount_minor BIGINT,
    p_idempotency_key TEXT
) RETURNS TABLE (
    from_entry_id UUID,
    to_entry_id UUID,
    from_amount_minor BIGINT,
    to_amount_minor BIGINT,
    applied_rate TEXT,
    mid_rate TEXT,
    fee_minor BIGINT
)
LANGUAGE plpgsql
AS $$
DECLARE
    v_from CHAR(3) := upper(p_from_ccy);
    v_to CHAR(3) := upper(p_to_ccy);
    v_q RECORD;
    v_to_amt BIGINT;
    v_fee BIGINT;
    v_from_acct UUID;
    v_to_acct UUID;
    v_from_float UUID;
    v_to_float UUID;
    v_fee_acct UUID;
    v_out UUID;
    v_in UUID;
    v_key_out TEXT := p_idempotency_key || ':out';
    v_key_in TEXT := p_idempotency_key || ':in';
    v_bal BIGINT;
    v_from_major NUMERIC;
BEGIN
    IF v_from = v_to THEN
        RAISE EXCEPTION 'same currency' USING ERRCODE = 'P0001';
    END IF;
    IF p_from_amount_minor <= 0 THEN
        RAISE EXCEPTION 'amount must be positive' USING ERRCODE = 'P0001';
    END IF;
    IF length(trim(p_idempotency_key)) = 0 OR length(p_idempotency_key) > 120 THEN
        RAISE EXCEPTION 'idempotency_key required' USING ERRCODE = 'P0001';
    END IF;

    IF EXISTS (SELECT 1 FROM journal_entries WHERE idempotency_key = v_key_out) THEN
        SELECT fc.entry_id, fc.to_entry_id, fc.from_amount_minor, fc.to_amount_minor,
               fc.applied_rate::text, fc.mid_rate::text, fc.fee_minor
          INTO from_entry_id, to_entry_id, from_amount_minor, to_amount_minor,
               applied_rate, mid_rate, fee_minor
          FROM fx_conversions fc
          JOIN journal_entries je ON je.id = fc.entry_id
         WHERE je.idempotency_key = v_key_out
         LIMIT 1;
        RETURN NEXT;
        RETURN;
    END IF;

    SELECT * INTO v_q FROM fx_quote_pair(v_from, v_to);

    -- Customer sells from → bank buy rate; amounts in minor (2 dp)
    v_from_major := p_from_amount_minor::NUMERIC / 100;
    v_to_amt := round(v_from_major * v_q.buy * 100)::BIGINT;
    IF v_to_amt <= 0 THEN
        RAISE EXCEPTION 'converted amount too small' USING ERRCODE = 'P0001';
    END IF;

    v_fee := calc_fee('fx_convert', v_from, p_from_amount_minor);

    -- Ensure accounts exist (welcome 0)
    PERFORM ledger_ensure_checking(p_user, v_from, 0);
    PERFORM ledger_ensure_checking(p_user, v_to, 0);

    v_from_acct := ledger_checking_for_update(p_user, v_from);
    v_to_acct := ledger_checking_for_update(p_user, v_to);
    v_from_float := ledger_system_account('system_float', v_from);
    v_to_float := ledger_system_account('system_float', v_to);
    v_fee_acct := ledger_system_account('system_fee', v_from);

    v_bal := account_balance(v_from_acct);
    IF v_bal < (p_from_amount_minor + v_fee) THEN
        RAISE EXCEPTION 'insufficient funds' USING ERRCODE = 'P0001';
    END IF;

    -- OUT: user from → float (+ fee to system_fee if any)
    IF v_fee = 0 THEN
        v_out := ledger_post_two(
            v_key_out, p_user, 'FX sell ' || v_from,
            v_from_acct, v_from_float, p_from_amount_minor, v_from
        );
    ELSE
        v_out := ledger_post_transfer_with_fee(
            v_key_out, p_user, 'FX sell ' || v_from,
            v_from_acct, v_from_float, v_fee_acct,
            p_from_amount_minor, v_fee, v_from
        );
    END IF;

    v_in := ledger_post_two(
        v_key_in, p_user, 'FX buy ' || v_to,
        v_to_float, v_to_acct, v_to_amt, v_to
    );

    INSERT INTO fx_conversions (
        user_id, entry_id, to_entry_id, from_currency, to_currency,
        from_amount_minor, to_amount_minor, mid_rate, applied_rate,
        margin_bps, spread_bps, fee_minor
    ) VALUES (
        p_user, v_out, v_in, v_from, v_to,
        p_from_amount_minor, v_to_amt, v_q.mid, v_q.buy,
        v_q.margin_bps, v_q.spread_bps, v_fee
    );

    from_entry_id := v_out;
    to_entry_id := v_in;
    from_amount_minor := p_from_amount_minor;
    to_amount_minor := v_to_amt;
    applied_rate := v_q.buy::text;
    mid_rate := v_q.mid::text;
    fee_minor := v_fee;
    RETURN NEXT;
END;
$$;
