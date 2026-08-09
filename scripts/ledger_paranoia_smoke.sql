-- Paranoia smoke for 008_ledger_paranoid.sql
-- Usage (against a migrated ledger DB):
--   psql "$LEDGER_DATABASE_URL" -v ON_ERROR_STOP=1 -f scripts/ledger_paranoia_smoke.sql

BEGIN;

DO $$
DECLARE
    u1 UUID := '11111111-1111-4111-8111-111111111111';
    u2 UUID := '22222222-2222-4222-8222-222222222222';
    a1 UUID;
    a2 UUID;
    r1 RECORD;
    r2 RECORD;
    bal1 BIGINT;
    bal2 BIGINT;
    bal1b BIGINT;
    bal2b BIGINT;
    n INT;
BEGIN
    -- Fresh users + welcome float credit
    a1 := ledger_ensure_checking(u1, 'RUB', 100000); -- 1000.00
    a2 := ledger_ensure_checking(u2, 'RUB', 0);

    -- Fund u1 from float if welcome skipped (already had account)
    IF account_balance(a1) < 100000 THEN
        PERFORM ledger_post_two(
            'smoke:fund:' || u1::text, u1, 'smoke fund',
            ledger_system_account('system_float', 'RUB'), a1, 100000, 'RUB'
        );
    END IF;

    bal1 := account_balance(a1);
    bal2 := account_balance(a2);

    SELECT * INTO r1 FROM ledger_transfer_p2p(
        u1, u2, 2500, 'RUB', 'smoke-key-1', 'smoke', u1
    );
    SELECT * INTO r2 FROM ledger_transfer_p2p(
        u1, u2, 2500, 'RUB', 'smoke-key-1', 'smoke', u1
    );

    IF r1.idempotent_replay THEN
        RAISE EXCEPTION 'first transfer must not be replay';
    END IF;
    IF NOT r2.idempotent_replay THEN
        RAISE EXCEPTION 'second transfer must be idempotent replay';
    END IF;
    IF r1.entry_id <> r2.entry_id THEN
        RAISE EXCEPTION 'replay must return same entry_id';
    END IF;

    bal1b := account_balance(a1);
    bal2b := account_balance(a2);

    -- Exactly one movement of 25.00 (+ fee from sender)
    IF bal2b - bal2 <> 2500 THEN
        RAISE EXCEPTION 'recipient delta % expected 2500', bal2b - bal2;
    END IF;
    IF bal1 - bal1b <> 2500 + r1.fee_minor THEN
        RAISE EXCEPTION 'sender delta % expected %', bal1 - bal1b, 2500 + r1.fee_minor;
    END IF;

    -- Intent conflict: same key, different amount
    BEGIN
        PERFORM * FROM ledger_transfer_p2p(
            u1, u2, 9999, 'RUB', 'smoke-key-1', 'smoke', u1
        );
        RAISE EXCEPTION 'expected idempotency conflict';
    EXCEPTION
        WHEN SQLSTATE 'P0001' THEN
            IF SQLERRM NOT ILIKE '%idempotency conflict%' THEN
                RAISE;
            END IF;
    END;

    -- Double-entry invariant for that entry
    SELECT COALESCE(SUM(amount_minor), 0) INTO n
      FROM postings WHERE entry_id = r1.entry_id;
    IF n <> 0 THEN
        RAISE EXCEPTION 'unbalanced journal % sum=%', r1.entry_id, n;
    END IF;

    -- Materialized balances match sum of posted postings
    IF account_balance(a1) IS DISTINCT FROM (
        SELECT COALESCE(SUM(p.amount_minor), 0)
          FROM postings p
          JOIN journal_entries je ON je.id = p.entry_id
         WHERE p.account_id = a1 AND je.status = 'posted'
    ) THEN
        RAISE EXCEPTION 'materialized balance drift on a1';
    END IF;

    RAISE NOTICE 'ledger paranoia smoke OK (entry=%, fee=%)', r1.entry_id, r1.fee_minor;
END $$;

ROLLBACK;
