-- RUB + USDT only: market ticks, CBR indicators, direct USDT/RUB mid.

UPDATE fx_settings
   SET enabled_currencies = ARRAY['RUB']
 WHERE id = 1;

ALTER TABLE crypto_settings
    ADD COLUMN IF NOT EXISTS usdt_rub_mid NUMERIC(24, 12) NOT NULL DEFAULT 0
        CHECK (usdt_rub_mid >= 0),
    ADD COLUMN IF NOT EXISTS usdt_rub_source TEXT NOT NULL DEFAULT 'unset',
    ADD COLUMN IF NOT EXISTS rates_updated_at TIMESTAMPTZ;

CREATE TABLE IF NOT EXISTS market_ticks (
    id BIGSERIAL PRIMARY KEY,
    source TEXT NOT NULL,
    symbol TEXT NOT NULL,
    price NUMERIC(24, 12) NOT NULL,
    meta JSONB NOT NULL DEFAULT '{}'::jsonb,
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS market_ticks_src_sym_idx
    ON market_ticks (source, symbol, fetched_at DESC);

CREATE TABLE IF NOT EXISTS cbr_indicators (
    id INT PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    key_rate NUMERIC(12, 4),
    key_rate_date DATE,
    inflation NUMERIC(12, 4),
    inflation_date DATE,
    usd_rub NUMERIC(24, 12),
    eur_rub NUMERIC(24, 12),
    cny_rub NUMERIC(24, 12),
    fx_as_of DATE,
    gold_rub NUMERIC(24, 4),
    reserves_bln_usd NUMERIC(24, 4),
    ruonia NUMERIC(12, 4),
    raw JSONB NOT NULL DEFAULT '{}'::jsonb,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

INSERT INTO cbr_indicators (id) VALUES (1)
ON CONFLICT (id) DO NOTHING;

-- Direct USDT→RUB quote using market mid (fallback: USDT/USD × CBR USD/RUB).
DROP FUNCTION IF EXISTS crypto_quote_deposit(BIGINT);
DROP FUNCTION IF EXISTS crypto_quote_withdraw(BIGINT);

CREATE OR REPLACE FUNCTION crypto_quote_deposit(p_usdt_minor BIGINT)
RETURNS TABLE (
    usdt_minor BIGINT,
    rub_gross_minor BIGINT,
    rub_credit_minor BIGINT,
    fee_minor BIGINT,
    usdt_usd_mid NUMERIC,
    usd_rub_mid NUMERIC,
    usdt_rub_mid NUMERIC,
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
    v_usdt_rub NUMERIC;
    v_gross BIGINT;
    v_fee BIGINT;
BEGIN
    SELECT * INTO s FROM crypto_settings WHERE id = 1;
    IF NOT FOUND OR NOT s.enabled THEN
        RAISE EXCEPTION 'crypto rail disabled' USING ERRCODE = 'P0001';
    END IF;
    IF p_usdt_minor < s.min_deposit_usdt_minor THEN
        RAISE EXCEPTION 'ниже минимума пополнения' USING ERRCODE = 'P0001';
    END IF;

    v_usdt_rub := s.usdt_rub_mid;
    IF v_usdt_rub IS NULL OR v_usdt_rub <= 0 THEN
        v_usd := crypto_usdt_to_usd_gross(p_usdt_minor, s.usdt_usd_mid);
        IF v_usd <= 0 THEN
            RAISE EXCEPTION 'сумма слишком мала' USING ERRCODE = 'P0001';
        END IF;
        SELECT mid INTO v_usd_rub FROM fx_mid_rate('USD', 'RUB');
        IF v_usd_rub IS NULL OR v_usd_rub <= 0 THEN
            RAISE EXCEPTION 'нет курса USDT/RUB — дождитесь обновления рынка' USING ERRCODE = 'P0001';
        END IF;
        v_usdt_rub := s.usdt_usd_mid * v_usd_rub;
        v_gross := round(v_usd::NUMERIC * v_usd_rub)::BIGINT;
    ELSE
        -- usdt_minor (6dp) * rub_per_usdt / 1e4 = kopecks
        v_gross := round(p_usdt_minor::NUMERIC * v_usdt_rub / 10000)::BIGINT;
        SELECT mid INTO v_usd_rub FROM fx_mid_rate('USD', 'RUB');
    END IF;

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
    usd_rub_mid := COALESCE(v_usd_rub, 0);
    usdt_rub_mid := v_usdt_rub;
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
    usdt_rub_mid NUMERIC,
    spread_bps INT,
    min_usdt_minor BIGINT
)
LANGUAGE plpgsql
STABLE
AS $$
DECLARE
    s crypto_settings%ROWTYPE;
    v_usd_rub NUMERIC;
    v_usdt_rub NUMERIC;
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

    v_fee := GREATEST(
        calc_fee('crypto_withdraw', 'RUB', p_rub_minor),
        ((p_rub_minor * s.withdraw_spread_bps + 5000) / 10000)
    );
    IF v_fee >= p_rub_minor THEN
        RAISE EXCEPTION 'комиссия превышает сумму' USING ERRCODE = 'P0001';
    END IF;
    v_net := p_rub_minor - v_fee;

    v_usdt_rub := s.usdt_rub_mid;
    IF v_usdt_rub IS NULL OR v_usdt_rub <= 0 THEN
        SELECT mid INTO v_usd_rub FROM fx_mid_rate('USD', 'RUB');
        IF v_usd_rub IS NULL OR v_usd_rub <= 0 THEN
            RAISE EXCEPTION 'нет курса USDT/RUB — дождитесь обновления рынка' USING ERRCODE = 'P0001';
        END IF;
        v_usdt_rub := s.usdt_usd_mid * v_usd_rub;
        v_usd := round(v_net::NUMERIC / v_usd_rub)::BIGINT;
        IF v_usd <= 0 THEN
            RAISE EXCEPTION 'сумма слишком мала' USING ERRCODE = 'P0001';
        END IF;
        v_usdt := crypto_usd_to_usdt_gross(v_usd, s.usdt_usd_mid);
    ELSE
        SELECT mid INTO v_usd_rub FROM fx_mid_rate('USD', 'RUB');
        -- kopecks → USDT minor: net/100 / mid * 1e6 = net * 10000 / mid
        v_usdt := round(v_net::NUMERIC * 10000 / v_usdt_rub)::BIGINT;
    END IF;

    IF v_usdt < s.min_withdraw_usdt_minor THEN
        RAISE EXCEPTION 'ниже минимума вывода' USING ERRCODE = 'P0001';
    END IF;

    rub_debit_minor := p_rub_minor;
    usdt_out_minor := v_usdt;
    fee_minor := v_fee;
    usdt_usd_mid := s.usdt_usd_mid;
    usd_rub_mid := COALESCE(v_usd_rub, 0);
    usdt_rub_mid := v_usdt_rub;
    spread_bps := s.withdraw_spread_bps;
    min_usdt_minor := s.min_withdraw_usdt_minor;
    RETURN NEXT;
END;
$$;
