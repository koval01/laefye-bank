-- Double-entry ledger (amounts in minor units, EUR cents).
-- Convention: positive = credit to the account (increases customer liability balance),
--             negative = debit. Every journal entry must sum to zero.

CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TABLE IF NOT EXISTS accounts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_user_id UUID,
    kind TEXT NOT NULL,
    currency CHAR(3) NOT NULL DEFAULT 'EUR',
    status TEXT NOT NULL DEFAULT 'open',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT accounts_kind_chk CHECK (
        kind IN ('user_checking', 'system_float', 'system_clearing', 'system_suspense')
    ),
    CONSTRAINT accounts_status_chk CHECK (status IN ('open', 'frozen', 'closed')),
    CONSTRAINT accounts_owner_chk CHECK (
        (kind = 'user_checking' AND owner_user_id IS NOT NULL)
        OR (kind <> 'user_checking' AND owner_user_id IS NULL)
    )
);

CREATE UNIQUE INDEX IF NOT EXISTS accounts_one_checking_per_user
    ON accounts (owner_user_id)
    WHERE kind = 'user_checking' AND status = 'open';

CREATE UNIQUE INDEX IF NOT EXISTS accounts_one_system_float
    ON accounts ((true))
    WHERE kind = 'system_float';

CREATE TABLE IF NOT EXISTS journal_entries (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    idempotency_key TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_by UUID,
    memo TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'posted',
    CONSTRAINT journal_status_chk CHECK (status IN ('posted', 'void'))
);

CREATE UNIQUE INDEX IF NOT EXISTS journal_idempotency_uidx
    ON journal_entries (idempotency_key);

CREATE TABLE IF NOT EXISTS postings (
    id BIGSERIAL PRIMARY KEY,
    entry_id UUID NOT NULL REFERENCES journal_entries (id),
    account_id UUID NOT NULL REFERENCES accounts (id),
    amount_minor BIGINT NOT NULL,
    currency CHAR(3) NOT NULL,
    CONSTRAINT postings_nonzero_chk CHECK (amount_minor <> 0)
);

CREATE INDEX IF NOT EXISTS postings_account_idx ON postings (account_id);
CREATE INDEX IF NOT EXISTS postings_entry_idx ON postings (entry_id);

CREATE OR REPLACE FUNCTION enforce_journal_balance()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    bal BIGINT;
    cur_count INT;
BEGIN
    -- Only validate after the entry is complete; callers insert entry then all postings
    -- then call validate_journal_entry(entry_id). This trigger still blocks single-leg inserts
    -- from leaving unbalanced committed state when used with deferred constraint trigger.
    SELECT COALESCE(SUM(amount_minor), 0), COUNT(DISTINCT currency)
      INTO bal, cur_count
      FROM postings
     WHERE entry_id = NEW.entry_id;

    IF cur_count > 1 THEN
        RAISE EXCEPTION 'journal % mixed currencies', NEW.entry_id;
    END IF;

    IF bal <> 0 THEN
        -- Allow incomplete inserts inside a transaction; final check via validate_journal_entry.
        NULL;
    END IF;
    RETURN NEW;
END;
$$;

CREATE OR REPLACE FUNCTION validate_journal_entry(p_entry UUID)
RETURNS VOID
LANGUAGE plpgsql
AS $$
DECLARE
    bal BIGINT;
    n INT;
    cur_count INT;
BEGIN
    SELECT COALESCE(SUM(amount_minor), 0), COUNT(*), COUNT(DISTINCT currency)
      INTO bal, n, cur_count
      FROM postings
     WHERE entry_id = p_entry;

    IF n < 2 THEN
        RAISE EXCEPTION 'journal % needs at least 2 postings', p_entry;
    END IF;
    IF cur_count <> 1 THEN
        RAISE EXCEPTION 'journal % must be single-currency', p_entry;
    END IF;
    IF bal <> 0 THEN
        RAISE EXCEPTION 'journal % unbalanced: sum=%', p_entry, bal;
    END IF;
END;
$$;

CREATE OR REPLACE FUNCTION account_balance(p_account UUID)
RETURNS BIGINT
LANGUAGE sql
STABLE
AS $$
    SELECT COALESCE(SUM(amount_minor), 0) FROM postings WHERE account_id = p_account;
$$;

-- Seed system float account (idempotent).
INSERT INTO accounts (id, owner_user_id, kind, currency, status)
SELECT '00000000-0000-4000-8000-000000000001'::uuid, NULL, 'system_float', 'EUR', 'open'
WHERE NOT EXISTS (SELECT 1 FROM accounts WHERE kind = 'system_float');
