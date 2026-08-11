-- Phase 2 — stamp core: catalogue, policy versions, rotating QR, accrual ledger.
--
-- The Phase 1 migration already created every table this phase writes to. What is missing
-- there are the invariants and access paths that only matter once accrual actually runs,
-- so this migration adds those rather than reshaping anything.
--
-- sqlx wraps each migration in its own transaction, so this file must not open one.

-- §12.6-7: a nonce is linked to at most one successful transaction.
--
-- The column already allows only one link per nonce. This closes the other direction — a
-- single transaction can never be credited with having consumed two different nonces,
-- which is what a replay would have to look like to be profitable.
CREATE UNIQUE INDEX IF NOT EXISTS uq_qr_nonces_consumed_transaction
    ON coupon.qr_nonces (consumed_transaction_id)
    WHERE consumed_transaction_id IS NOT NULL;

-- Consumption is a one-way door. The accrual path already guards with
-- `WHERE consumed_at IS NULL`, but that guard lives in application code; this one cannot
-- be forgotten or refactored away.
CREATE OR REPLACE FUNCTION coupon.reject_qr_nonce_reconsumption()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF OLD.consumed_at IS NOT NULL
       AND (NEW.consumed_at IS DISTINCT FROM OLD.consumed_at
            OR NEW.consumed_transaction_id IS DISTINCT FROM OLD.consumed_transaction_id)
    THEN
        RAISE EXCEPTION 'qr nonce % has already been consumed', OLD.id
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS trg_qr_nonces_consume_once ON coupon.qr_nonces;
CREATE TRIGGER trg_qr_nonces_consume_once
BEFORE UPDATE ON coupon.qr_nonces
FOR EACH ROW EXECUTE FUNCTION coupon.reject_qr_nonce_reconsumption();

-- §12.6-8: a status event's `from_status` must be the instance's status at the time.
--
-- The writer updates the coupon and then appends the event, so by the time the event
-- lands the instance must already read `to_status` and the event's `from_status` must be
-- what it moved away from. Checking `to_status` against the live row catches an event
-- written for a transition that never happened, and `from_status` is validated against
-- the previous event's `to_status` so the chain cannot fork.
CREATE OR REPLACE FUNCTION coupon.enforce_coupon_status_event_chain()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    live_status coupon.coupon_status;
    previous_status coupon.coupon_status;
BEGIN
    SELECT status INTO live_status
    FROM coupon.coupon_instances
    WHERE id = NEW.coupon_id;

    IF live_status IS DISTINCT FROM NEW.to_status THEN
        RAISE EXCEPTION 'coupon % is %, but a status event claims it became %',
            NEW.coupon_id, live_status, NEW.to_status
            USING ERRCODE = '23514';
    END IF;

    SELECT to_status INTO previous_status
    FROM coupon.coupon_status_events
    WHERE coupon_id = NEW.coupon_id AND id <> NEW.id
    ORDER BY occurred_at DESC, id DESC
    LIMIT 1;

    IF previous_status IS NOT NULL AND NEW.from_status IS DISTINCT FROM previous_status THEN
        RAISE EXCEPTION 'coupon % last moved to %, but a status event starts from %',
            NEW.coupon_id, previous_status, NEW.from_status
            USING ERRCODE = '23514';
    END IF;

    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS trg_coupon_status_events_chain ON coupon.coupon_status_events;
CREATE CONSTRAINT TRIGGER trg_coupon_status_events_chain
AFTER INSERT ON coupon.coupon_status_events
DEFERRABLE INITIALLY IMMEDIATE
FOR EACH ROW EXECUTE FUNCTION coupon.enforce_coupon_status_event_chain();

-- §6.3 shows one current version and one *next* version. Two scheduled versions would
-- make "what happens tomorrow" ambiguous, so the database allows only one — the same way
-- `uq_loyalty_policies_active_store` allows only one ACTIVE version (§12.6-2).
CREATE UNIQUE INDEX IF NOT EXISTS uq_loyalty_policies_scheduled_store
    ON coupon.loyalty_policies (store_id)
    WHERE status = 'SCHEDULED';

-- A scheduled version is meaningless without the instant it takes effect.
ALTER TABLE coupon.loyalty_policies
    DROP CONSTRAINT IF EXISTS ck_loyalty_policy_scheduled_start;
ALTER TABLE coupon.loyalty_policies
    ADD CONSTRAINT ck_loyalty_policy_scheduled_start
    CHECK (status <> 'SCHEDULED' OR starts_at IS NOT NULL);

-- Publishing must record when it happened, whichever way it was published.
ALTER TABLE coupon.loyalty_policies
    DROP CONSTRAINT IF EXISTS ck_loyalty_policy_published_at;
ALTER TABLE coupon.loyalty_policies
    ADD CONSTRAINT ck_loyalty_policy_published_at
    CHECK (status IN ('DRAFT') OR published_at IS NOT NULL);

-- STAMP-003: the duplicate-request warning looks back over one store's recent accruals
-- for one customer.
CREATE INDEX IF NOT EXISTS ix_stamp_transactions_recent
    ON coupon.stamp_transactions (store_id, user_id, confirmed_at DESC);

-- STAMP-007 reverses one transaction: find every ledger row it produced, on any lot.
CREATE INDEX IF NOT EXISTS ix_stamp_ledger_source_transaction
    ON coupon.stamp_ledger (source_stamp_transaction_id, event_type)
    WHERE source_stamp_transaction_id IS NOT NULL;

-- STAMP-006: the expiry sweep looks for lots whose absolute expiry has passed. Online
-- reads do not depend on this — they compare `expires_at` themselves — so the index only
-- has to serve the batch (§18.1).
CREATE INDEX IF NOT EXISTS ix_stamp_lots_expiry_sweep
    ON coupon.stamp_lots (expires_at, id);

-- The wallet lists a consumer's rewards per store, newest first.
CREATE INDEX IF NOT EXISTS ix_coupon_instances_wallet_store
    ON coupon.coupon_instances (user_id, store_id, status, created_at DESC);

-- The admin transaction explorer walks from a coupon back to the ledger rows that paid
-- for it (§11.5).
CREATE INDEX IF NOT EXISTS ix_stamp_ledger_reward_lookup
    ON coupon.stamp_ledger (reward_coupon_id, occurred_at)
    WHERE reward_coupon_id IS NOT NULL;

-- Balance is derived, never stored. This view is the one definition of "how many stamps
-- does this lot still have", so a reader and the expiry sweep cannot drift apart, and it
-- is by construction rebuildable from the ledger (§12.3).
CREATE OR REPLACE VIEW coupon.stamp_lot_balances AS
SELECT
    l.id AS lot_id,
    l.store_id,
    l.user_id,
    l.policy_id,
    l.source_transaction_id,
    l.original_quantity,
    l.earned_at,
    l.expires_at,
    COALESCE(SUM(e.quantity_delta), 0)::bigint AS balance
FROM coupon.stamp_lots l
LEFT JOIN coupon.stamp_ledger e ON e.lot_id = l.id
GROUP BY l.id;

COMMENT ON VIEW coupon.stamp_lot_balances IS
    'Derived stamp balance per lot; a lot only counts as available while expires_at is in the future';

INSERT INTO coupon.schema_metadata (key, value)
VALUES ('schema_version', '20260811000200')
ON CONFLICT (key) DO UPDATE
SET value = EXCLUDED.value,
    updated_at = clock_timestamp();
