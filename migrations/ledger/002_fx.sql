-- Multi-currency + FX admin settings (ledger DB).

-- One open checking account per (user, currency).
DROP INDEX IF EXISTS accounts_one_checking_per_user;
CREATE UNIQUE INDEX IF NOT EXISTS accounts_one_checking_per_user_ccy
    ON accounts (owner_user_id, currency)
    WHERE kind = 'user_checking' AND status = 'open';

DROP INDEX IF EXISTS accounts_one_system_float;
CREATE UNIQUE INDEX IF NOT EXISTS accounts_one_system_float_ccy
    ON accounts (currency)
    WHERE kind = 'system_float';

ALTER TABLE accounts DROP CONSTRAINT IF EXISTS accounts_kind_chk;
ALTER TABLE accounts ADD CONSTRAINT accounts_kind_chk CHECK (
    kind IN ('user_checking', 'system_float', 'system_clearing', 'system_suspense', 'system_fx', 'system_fee')
);

-- Mid rates from CBR (always quote RUB per 1 unit of base, except RUB/RUB=1).
CREATE TABLE IF NOT EXISTS fx_rates (
    base CHAR(3) NOT NULL,
    quote CHAR(3) NOT NULL DEFAULT 'RUB',
    mid NUMERIC(24, 12) NOT NULL,
    nominal INT NOT NULL DEFAULT 1,
    as_of DATE NOT NULL,
    source TEXT NOT NULL DEFAULT 'cbr',
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (base, quote, as_of)
);

CREATE TABLE IF NOT EXISTS fx_settings (
    id SMALLINT PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    -- Bank markup applied on top of CBR mid (basis points, 100 = 1%).
    margin_bps INT NOT NULL DEFAULT 50 CHECK (margin_bps >= 0 AND margin_bps <= 5000),
    -- Half of buy/sell gap in bps (total buy↔sell ≈ 2 * spread_bps around margined mid).
    spread_bps INT NOT NULL DEFAULT 30 CHECK (spread_bps >= 0 AND spread_bps <= 5000),
    enabled_currencies TEXT[] NOT NULL DEFAULT ARRAY['EUR','USD','RUB'],
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_by UUID
);

INSERT INTO fx_settings (id) VALUES (1)
ON CONFLICT (id) DO NOTHING;

-- Seed system floats for additional currencies.
INSERT INTO accounts (owner_user_id, kind, currency, status)
SELECT NULL, 'system_float', ccy, 'open'
FROM (VALUES ('USD'), ('RUB')) AS t(ccy)
WHERE NOT EXISTS (
    SELECT 1 FROM accounts a WHERE a.kind = 'system_float' AND a.currency = t.ccy
);

CREATE TABLE IF NOT EXISTS fx_conversions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL,
    entry_id UUID NOT NULL REFERENCES journal_entries (id),
    from_currency CHAR(3) NOT NULL,
    to_currency CHAR(3) NOT NULL,
    from_amount_minor BIGINT NOT NULL,
    to_amount_minor BIGINT NOT NULL,
    mid_rate NUMERIC(24, 12) NOT NULL,
    applied_rate NUMERIC(24, 12) NOT NULL,
    margin_bps INT NOT NULL,
    spread_bps INT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
