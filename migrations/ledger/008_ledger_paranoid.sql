-- Paranoid ledger hardening:
-- * advisory lock per idempotency key (no concurrent double-post races)
-- * intent_hash on journal (key reuse with different payload → hard fail)
-- * pending → posted lifecycle; all legs in one INSERT
-- * materialized accounts.balance_minor updated atomically with postings
-- * deferred commit check: posted journal must balance
-- * crypto withdraw claim (queued→sending) + refund on fail

-- ── Schema ─────────────────────────────────────────────────

ALTER TABLE journal_entries DROP CONSTRAINT IF EXISTS journal_status_chk;
ALTER TABLE journal_entries
    ADD CONSTRAINT journal_status_chk
    CHECK (status IN ('pending', 'posted', 'void'));

ALTER TABLE journal_entries
    ADD COLUMN IF NOT EXISTS intent_hash TEXT,
    ADD COLUMN IF NOT EXISTS op_kind TEXT;

ALTER TABLE accounts
    ADD COLUMN IF NOT EXISTS balance_minor BIGINT NOT NULL DEFAULT 0;

-- Backfill from postings (posted only).
UPDATE accounts a
   SET balance_minor = COALESCE((
        SELECT SUM(p.amount_minor)
          FROM postings p
          JOIN journal_entries je ON je.id = p.entry_id
         WHERE p.account_id = a.id
           AND je.status = 'posted'
   ), 0);

CREATE OR REPLACE FUNCTION account_balance(p_account UUID)
RETURNS BIGINT
LANGUAGE sql
STABLE
AS $$
    SELECT COALESCE(
        (SELECT balance_minor FROM accounts WHERE id = p_account),
        0
    );
$$;

-- ── Advisory lock helper ───────────────────────────────────

CREATE OR REPLACE FUNCTION ledger_advisory_xact_lock(p_key TEXT)
RETURNS VOID
LANGUAGE plpgsql
AS $$
DECLARE
    v_h BIGINT;
BEGIN
    -- Stable 64-bit lock id from key (two int4 halves of md5).
    v_h := (('x' || substr(md5(p_key), 1, 16))::bit(64)::bigint);
    PERFORM pg_advisory_xact_lock(v_h);
END;
$$;

-- Lock two accounts in UUID order (deadlock-safe).
CREATE OR REPLACE FUNCTION ledger_lock_accounts_2(p_a UUID, p_b UUID)
RETURNS VOID
LANGUAGE plpgsql
AS $$
BEGIN
    IF p_a = p_b THEN
        PERFORM 1 FROM accounts WHERE id = p_a FOR UPDATE;
        RETURN;
    END IF;
    IF p_a < p_b THEN
        PERFORM 1 FROM accounts WHERE id = p_a FOR UPDATE;
        PERFORM 1 FROM accounts WHERE id = p_b FOR UPDATE;
    ELSE
        PERFORM 1 FROM accounts WHERE id = p_b FOR UPDATE;
        PERFORM 1 FROM accounts WHERE id = p_a FOR UPDATE;
    END IF;
END;
$$;

CREATE OR REPLACE FUNCTION ledger_lock_accounts_3(p_a UUID, p_b UUID, p_c UUID)
RETURNS VOID
LANGUAGE plpgsql
AS $$
DECLARE
    x UUID;
    y UUID;
    z UUID;
BEGIN
    -- Sort three UUIDs.
    IF p_a <= p_b AND p_a <= p_c THEN
        x := p_a;
        IF p_b <= p_c THEN y := p_b; z := p_c; ELSE y := p_c; z := p_b; END IF;
    ELSIF p_b <= p_a AND p_b <= p_c THEN
        x := p_b;
        IF p_a <= p_c THEN y := p_a; z := p_c; ELSE y := p_c; z := p_a; END IF;
    ELSE
        x := p_c;
        IF p_a <= p_b THEN y := p_a; z := p_b; ELSE y := p_b; z := p_a; END IF;
    END IF;
    PERFORM 1 FROM accounts WHERE id = x FOR UPDATE;
    IF y <> x THEN
        PERFORM 1 FROM accounts WHERE id = y FOR UPDATE;
    END IF;
    IF z <> y AND z <> x THEN
        PERFORM 1 FROM accounts WHERE id = z FOR UPDATE;
    END IF;
END;
$$;

-- Apply posting delta to materialized balance; refuse negative user checking.
CREATE OR REPLACE FUNCTION ledger_apply_delta(p_acct UUID, p_delta BIGINT)
RETURNS VOID
LANGUAGE plpgsql
AS $$
DECLARE
    v_kind TEXT;
    v_new BIGINT;
BEGIN
    SELECT kind, balance_minor + p_delta
      INTO v_kind, v_new
      FROM accounts
     WHERE id = p_acct
     FOR UPDATE;

    IF NOT FOUND THEN
        RAISE EXCEPTION 'account % missing', p_acct USING ERRCODE = 'P0001';
    END IF;

    IF v_kind = 'user_checking' AND v_new < 0 THEN
        RAISE EXCEPTION 'insufficient funds' USING ERRCODE = 'P0001';
    END IF;

    UPDATE accounts SET balance_minor = v_new WHERE id = p_acct;
END;
$$;

CREATE OR REPLACE FUNCTION ledger_intent_hash(p_parts TEXT)
RETURNS TEXT
LANGUAGE sql
IMMUTABLE
AS $$
    SELECT encode(digest(p_parts, 'sha256'), 'hex');
$$;

-- Reserve journal row under advisory lock. Returns (entry_id, is_replay).
CREATE OR REPLACE FUNCTION ledger_reserve_entry(
    p_idempotency_key TEXT,
    p_intent_hash TEXT,
    p_op_kind TEXT,
    p_created_by UUID,
    p_memo TEXT
) RETURNS TABLE (entry_id UUID, is_replay BOOLEAN)
LANGUAGE plpgsql
AS $$
DECLARE
    v_id UUID;
    v_hash TEXT;
    v_status TEXT;
BEGIN
    IF length(trim(p_idempotency_key)) = 0 OR length(p_idempotency_key) > 200 THEN
        RAISE EXCEPTION 'idempotency_key required (1–200 chars)' USING ERRCODE = 'P0001';
    END IF;
    IF length(trim(p_intent_hash)) < 32 THEN
        RAISE EXCEPTION 'intent_hash required' USING ERRCODE = 'P0001';
    END IF;

    PERFORM ledger_advisory_xact_lock(p_idempotency_key);

    INSERT INTO journal_entries (
        idempotency_key, created_by, memo, status, intent_hash, op_kind
    ) VALUES (
        p_idempotency_key, p_created_by, COALESCE(p_memo, ''), 'pending',
        p_intent_hash, p_op_kind
    )
    ON CONFLICT (idempotency_key) DO NOTHING
    RETURNING id INTO v_id;

    IF v_id IS NOT NULL THEN
        entry_id := v_id;
        is_replay := FALSE;
        RETURN NEXT;
        RETURN;
    END IF;

    SELECT je.id, je.intent_hash, je.status
      INTO v_id, v_hash, v_status
      FROM journal_entries je
     WHERE je.idempotency_key = p_idempotency_key
     FOR UPDATE;

    IF v_id IS NULL THEN
        RAISE EXCEPTION 'idempotency race' USING ERRCODE = 'P0001';
    END IF;

    IF v_hash IS NOT NULL AND v_hash <> p_intent_hash THEN
        RAISE EXCEPTION 'idempotency conflict: key reused with different intent'
            USING ERRCODE = 'P0001';
    END IF;

    IF v_status = 'void' THEN
        RAISE EXCEPTION 'journal void' USING ERRCODE = 'P0001';
    END IF;

    -- Defensive crash recovery: if a pending row was somehow committed with
    -- orphan postings / balance deltas, reverse deltas then wipe legs.
    IF v_status = 'pending' THEN
        DECLARE
            r RECORD;
        BEGIN
            FOR r IN
                SELECT account_id, amount_minor
                  FROM postings
                 WHERE entry_id = v_id
            LOOP
                PERFORM 1 FROM accounts WHERE id = r.account_id FOR UPDATE;
                UPDATE accounts
                   SET balance_minor = balance_minor - r.amount_minor
                 WHERE id = r.account_id;
            END LOOP;
            DELETE FROM postings WHERE entry_id = v_id;
        END;
        entry_id := v_id;
        is_replay := FALSE;
        RETURN NEXT;
        RETURN;
    END IF;

    -- posted → pure replay
    entry_id := v_id;
    is_replay := TRUE;
    RETURN NEXT;
END;
$$;

CREATE OR REPLACE FUNCTION ledger_finalize_entry(p_entry UUID)
RETURNS VOID
LANGUAGE plpgsql
AS $$
BEGIN
    PERFORM validate_journal_entry(p_entry);
    UPDATE journal_entries
       SET status = 'posted'
     WHERE id = p_entry AND status = 'pending';
    IF NOT FOUND THEN
        RAISE EXCEPTION 'journal % not pending', p_entry USING ERRCODE = 'P0001';
    END IF;
END;
$$;

-- Deferred safety net: every posted entry must balance at COMMIT.
CREATE OR REPLACE FUNCTION ledger_assert_posted_balanced()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    bal BIGINT;
    n INT;
BEGIN
    IF NEW.status <> 'posted' THEN
        RETURN NEW;
    END IF;
    SELECT COALESCE(SUM(amount_minor), 0), COUNT(*)
      INTO bal, n
      FROM postings
     WHERE entry_id = NEW.id;
    IF n < 2 OR bal <> 0 THEN
        RAISE EXCEPTION 'commit refused: journal % unbalanced (sum=%, legs=%)',
            NEW.id, bal, n
            USING ERRCODE = 'P0001';
    END IF;
    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS trg_journal_posted_balanced ON journal_entries;
CREATE CONSTRAINT TRIGGER trg_journal_posted_balanced
AFTER UPDATE OF status ON journal_entries
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
WHEN (NEW.status = 'posted')
EXECUTE FUNCTION ledger_assert_posted_balanced();

-- ── Core posting (atomic multi-leg) ────────────────────────

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
    v_ccy CHAR(3) := upper(p_currency);
    v_intent TEXT;
    v_res RECORD;
    v_entry UUID;
BEGIN
    IF p_amount_minor <= 0 THEN
        RAISE EXCEPTION 'amount must be positive' USING ERRCODE = 'P0001';
    END IF;
    IF p_from_acct = p_to_acct THEN
        RAISE EXCEPTION 'same account' USING ERRCODE = 'P0001';
    END IF;

    v_intent := ledger_intent_hash(
        'two|' || p_from_acct::text || '|' || p_to_acct::text || '|' ||
        p_amount_minor::text || '|' || v_ccy
    );

    SELECT * INTO v_res
      FROM ledger_reserve_entry(
          p_idempotency_key, v_intent, 'two', p_created_by, p_memo
      );

    IF v_res.is_replay THEN
        RETURN v_res.entry_id;
    END IF;

    v_entry := v_res.entry_id;
    PERFORM ledger_lock_accounts_2(p_from_acct, p_to_acct);

    -- Debit first (funds check), then credit — still one transaction.
    PERFORM ledger_apply_delta(p_from_acct, -p_amount_minor);
    PERFORM ledger_apply_delta(p_to_acct, p_amount_minor);

    INSERT INTO postings (entry_id, account_id, amount_minor, currency)
    VALUES
        (v_entry, p_from_acct, -p_amount_minor, v_ccy),
        (v_entry, p_to_acct,    p_amount_minor, v_ccy);

    PERFORM ledger_finalize_entry(v_entry);
    RETURN v_entry;
END;
$$;

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
    v_ccy CHAR(3) := upper(p_currency);
    v_total BIGINT;
    v_intent TEXT;
    v_res RECORD;
    v_entry UUID;
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
    v_intent := ledger_intent_hash(
        'xfer|' || p_from_acct::text || '|' || p_to_acct::text || '|' ||
        COALESCE(p_fee_acct::text, '-') || '|' || p_amount_minor::text || '|' ||
        p_fee_minor::text || '|' || v_ccy
    );

    SELECT * INTO v_res
      FROM ledger_reserve_entry(
          p_idempotency_key, v_intent, 'xfer_fee', p_created_by, p_memo
      );

    IF v_res.is_replay THEN
        RETURN v_res.entry_id;
    END IF;

    v_entry := v_res.entry_id;

    IF p_fee_minor = 0 THEN
        PERFORM ledger_lock_accounts_2(p_from_acct, p_to_acct);
        PERFORM ledger_apply_delta(p_from_acct, -p_amount_minor);
        PERFORM ledger_apply_delta(p_to_acct, p_amount_minor);
        INSERT INTO postings (entry_id, account_id, amount_minor, currency)
        VALUES
            (v_entry, p_from_acct, -p_amount_minor, v_ccy),
            (v_entry, p_to_acct,    p_amount_minor, v_ccy);
    ELSE
        PERFORM ledger_lock_accounts_3(p_from_acct, p_to_acct, p_fee_acct);
        PERFORM ledger_apply_delta(p_from_acct, -v_total);
        PERFORM ledger_apply_delta(p_to_acct, p_amount_minor);
        PERFORM ledger_apply_delta(p_fee_acct, p_fee_minor);
        INSERT INTO postings (entry_id, account_id, amount_minor, currency)
        VALUES
            (v_entry, p_from_acct, -v_total, v_ccy),
            (v_entry, p_to_acct,    p_amount_minor, v_ccy),
            (v_entry, p_fee_acct,   p_fee_minor, v_ccy);
    END IF;

    PERFORM ledger_finalize_entry(v_entry);
    RETURN v_entry;
END;
$$;

-- ── P2P with intent-checked replay ─────────────────────────

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
    v_scoped TEXT;
    v_existing_id UUID;
    v_existing_status TEXT;
    v_credit BIGINT;
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

    -- Scope key to sender so client keys cannot collide across users.
    v_scoped := 'p2p:' || p_from_user::text || ':' || p_idempotency_key;
    PERFORM ledger_advisory_xact_lock(v_scoped);

    -- Lock accounts in user-id order first (stable).
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

    SELECT je.id, je.status
      INTO v_existing_id, v_existing_status
      FROM journal_entries je
     WHERE je.idempotency_key = v_scoped
     FOR UPDATE;

    IF v_existing_id IS NOT NULL AND v_existing_status = 'posted' THEN
        SELECT p.amount_minor INTO v_credit
          FROM postings p
         WHERE p.entry_id = v_existing_id
           AND p.account_id = v_to
           AND p.amount_minor > 0
         LIMIT 1;
        IF v_credit IS DISTINCT FROM p_amount_minor THEN
            RAISE EXCEPTION 'idempotency conflict: key reused with different amount'
                USING ERRCODE = 'P0001';
        END IF;
        entry_id := v_existing_id;
        idempotent_replay := TRUE;
        from_balance_minor := account_balance(v_from);
        to_balance_minor := account_balance(v_to);
        SELECT COALESCE((
            SELECT p.amount_minor
              FROM postings p
              JOIN accounts a ON a.id = p.account_id
             WHERE p.entry_id = v_existing_id
               AND a.kind = 'system_fee'
               AND p.amount_minor > 0
             LIMIT 1
        ), 0) INTO fee_minor;
        RETURN NEXT;
        RETURN;
    END IF;

    v_fee := calc_fee('p2p_transfer', v_ccy, p_amount_minor);
    v_fee_acct := ledger_system_account('system_fee', v_ccy);

    v_entry := ledger_post_transfer_with_fee(
        v_scoped,
        p_created_by,
        COALESCE(NULLIF(trim(p_memo), ''), 'P2P transfer'),
        v_from,
        v_to,
        v_fee_acct,
        p_amount_minor,
        v_fee,
        v_ccy
    );

    UPDATE journal_entries SET op_kind = 'p2p' WHERE id = v_entry;

    entry_id := v_entry;
    idempotent_replay := FALSE;
    from_balance_minor := account_balance(v_from);
    to_balance_minor := account_balance(v_to);
    fee_minor := v_fee;
    RETURN NEXT;
END;
$$;

-- Welcome credit: atomic reserve (no TOCTOU double bonus).
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

    INSERT INTO accounts (owner_user_id, kind, currency, status, balance_minor)
    VALUES (p_user, 'user_checking', v_ccy, 'open', 0)
    RETURNING id INTO v_id;

    IF p_welcome_minor > 0 AND v_ccy = 'RUB' THEN
        v_float := ledger_system_account('system_float', v_ccy);
        v_key := 'welcome:' || p_user::text || ':' || v_ccy;
        -- ledger_post_two is idempotent under advisory lock + unique key.
        PERFORM ledger_post_two(
            v_key, p_user, 'Приветственный бонус',
            v_float, v_id, p_welcome_minor, v_ccy
        );
    END IF;

    RETURN v_id;
END;
$$;

-- FX: lock accounts by id order to avoid deadlocks.
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
    v_key_out TEXT := 'fx:' || p_user::text || ':' || p_idempotency_key || ':out';
    v_key_in TEXT := 'fx:' || p_user::text || ':' || p_idempotency_key || ':in';
    v_from_major NUMERIC;
    v_lock_a UUID;
    v_lock_b UUID;
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

    PERFORM ledger_advisory_xact_lock(v_key_out);

    IF EXISTS (
        SELECT 1 FROM journal_entries WHERE idempotency_key = v_key_out AND status = 'posted'
    ) THEN
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

    v_from_major := p_from_amount_minor::NUMERIC / 100;
    v_to_amt := round(v_from_major * v_q.buy * 100)::BIGINT;
    IF v_to_amt <= 0 THEN
        RAISE EXCEPTION 'converted amount too small' USING ERRCODE = 'P0001';
    END IF;

    v_fee := calc_fee('fx_convert', v_from, p_from_amount_minor);

    PERFORM ledger_ensure_checking(p_user, v_from, 0);
    PERFORM ledger_ensure_checking(p_user, v_to, 0);

    v_from_acct := ledger_checking_for_update(p_user, v_from);
    v_to_acct := ledger_checking_for_update(p_user, v_to);
    -- Re-lock in id order (checking_for_update already locked; re-order via helper).
    IF v_from_acct < v_to_acct THEN
        v_lock_a := v_from_acct; v_lock_b := v_to_acct;
    ELSE
        v_lock_a := v_to_acct; v_lock_b := v_from_acct;
    END IF;
    PERFORM ledger_lock_accounts_2(v_lock_a, v_lock_b);

    v_from_float := ledger_system_account('system_float', v_from);
    v_to_float := ledger_system_account('system_float', v_to);
    v_fee_acct := ledger_system_account('system_fee', v_from);

    IF account_balance(v_from_acct) < (p_from_amount_minor + v_fee) THEN
        RAISE EXCEPTION 'insufficient funds' USING ERRCODE = 'P0001';
    END IF;

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

-- ── Crypto withdraw claim + refund ─────────────────────────

-- Drop any status CHECK so we can allow 'sending' (inline CHECKs get auto-names).
DO $$
DECLARE
    r RECORD;
BEGIN
    FOR r IN
        SELECT c.conname
          FROM pg_constraint c
         WHERE c.conrelid = 'crypto_withdrawals'::regclass
           AND c.contype = 'c'
           AND pg_get_constraintdef(c.oid) ILIKE '%queued%'
    LOOP
        EXECUTE format('ALTER TABLE crypto_withdrawals DROP CONSTRAINT %I', r.conname);
    END LOOP;
END $$;

ALTER TABLE crypto_withdrawals
    ADD COLUMN IF NOT EXISTS claim_token UUID,
    ADD COLUMN IF NOT EXISTS claimed_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS refund_entry_id UUID REFERENCES journal_entries (id);

DO $$
BEGIN
    ALTER TABLE crypto_withdrawals
        ADD CONSTRAINT crypto_withdrawals_status_chk
        CHECK (status IN ('queued', 'sending', 'broadcast', 'confirmed', 'failed', 'cancelled'));
EXCEPTION
    WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION crypto_claim_next_withdrawals(p_limit INT DEFAULT 5)
RETURNS TABLE (
    id UUID,
    to_address TEXT,
    usdt_out_minor BIGINT,
    claim_token UUID
)
LANGUAGE plpgsql
AS $$
BEGIN
    RETURN QUERY
    WITH picked AS (
        SELECT w.id
          FROM crypto_withdrawals w
         WHERE w.status = 'queued'
         ORDER BY w.created_at
         FOR UPDATE SKIP LOCKED
         LIMIT GREATEST(p_limit, 1)
    )
    UPDATE crypto_withdrawals w
       SET status = 'sending',
           claim_token = gen_random_uuid(),
           claimed_at = NOW(),
           updated_at = NOW()
      FROM picked
     WHERE w.id = picked.id
    RETURNING w.id, w.to_address, w.usdt_out_minor, w.claim_token;
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
     WHERE id = p_id AND status = 'sending';
    IF NOT FOUND THEN
        RAISE EXCEPTION 'withdrawal not in sending state' USING ERRCODE = 'P0001';
    END IF;
END;
$$;

-- Fail + refund ledger debit (only from sending/queued; never after broadcast).
CREATE OR REPLACE FUNCTION crypto_mark_withdraw_failed(
    p_id UUID,
    p_error TEXT
) RETURNS VOID
LANGUAGE plpgsql
AS $$
DECLARE
    v_w crypto_withdrawals%ROWTYPE;
    v_user_acct UUID;
    v_float UUID;
    v_fee_acct UUID;
    v_refund UUID;
    v_principal BIGINT;
BEGIN
    SELECT * INTO v_w
      FROM crypto_withdrawals
     WHERE id = p_id
     FOR UPDATE;

    IF NOT FOUND THEN
        RAISE EXCEPTION 'withdrawal not found' USING ERRCODE = 'P0001';
    END IF;

    IF v_w.status IN ('failed', 'cancelled', 'confirmed') THEN
        RETURN;
    END IF;

    IF v_w.status = 'broadcast' THEN
        -- On-chain may exist — do not auto-refund; park for ops.
        UPDATE crypto_withdrawals
           SET status = 'failed',
               error_message = left('broadcast_failed_needs_review:' || COALESCE(p_error, ''), 500),
               updated_at = NOW()
         WHERE id = p_id;
        RETURN;
    END IF;

    IF v_w.status NOT IN ('queued', 'sending') THEN
        RAISE EXCEPTION 'cannot fail withdrawal in status %', v_w.status USING ERRCODE = 'P0001';
    END IF;

    -- Refund: crypto float → user (reverse of withdraw posting).
    v_user_acct := ledger_checking_for_update(v_w.user_id, 'RUB');
    v_float := ledger_system_account('system_crypto_float', 'RUB');
    v_fee_acct := ledger_system_account('system_fee', 'RUB');
    v_principal := v_w.rub_debit_minor - v_w.fee_minor;

    -- Original withdraw: user -(principal+fee), float +principal, fee +fee
    -- Refund in one balanced entry: float -principal, fee -fee, user +(principal+fee)
    PERFORM ledger_advisory_xact_lock('crypto:wd:refund:' || v_w.id::text);

    IF v_w.fee_minor = 0 THEN
        v_refund := ledger_post_two(
            'crypto:wd:refund:' || v_w.id::text,
            v_w.user_id,
            'Возврат неуспешного вывода USDT',
            v_float,
            v_user_acct,
            v_w.rub_debit_minor,
            'RUB'
        );
    ELSE
        DECLARE
            v_res RECORD;
            v_intent TEXT;
        BEGIN
            v_intent := ledger_intent_hash(
                'wd_refund|' || v_w.id::text || '|' ||
                v_w.rub_debit_minor::text || '|' || v_w.fee_minor::text
            );
            SELECT * INTO v_res FROM ledger_reserve_entry(
                'crypto:wd:refund:' || v_w.id::text,
                v_intent,
                'wd_refund',
                v_w.user_id,
                'Возврат неуспешного вывода USDT'
            );
            IF v_res.is_replay THEN
                v_refund := v_res.entry_id;
            ELSE
                v_refund := v_res.entry_id;
                PERFORM ledger_lock_accounts_3(v_float, v_fee_acct, v_user_acct);
                PERFORM ledger_apply_delta(v_float, -v_principal);
                PERFORM ledger_apply_delta(v_fee_acct, -v_w.fee_minor);
                PERFORM ledger_apply_delta(v_user_acct, v_w.rub_debit_minor);
                INSERT INTO postings (entry_id, account_id, amount_minor, currency)
                VALUES
                    (v_refund, v_float, -v_principal, 'RUB'),
                    (v_refund, v_fee_acct, -v_w.fee_minor, 'RUB'),
                    (v_refund, v_user_acct, v_w.rub_debit_minor, 'RUB');
                PERFORM ledger_finalize_entry(v_refund);
            END IF;
        END;
    END IF;

    UPDATE crypto_withdrawals
       SET status = 'failed',
           error_message = left(COALESCE(p_error, 'failed'), 500),
           refund_entry_id = v_refund,
           updated_at = NOW()
     WHERE id = p_id;
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
     WHERE id = p_id AND status IN ('broadcast', 'sending');
END;
$$;
