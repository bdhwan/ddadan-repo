-- Phase 3 — discount campaigns, redemption, and the asynchronous job queue.
--
-- The Phase 1 migration already created `campaigns`, `campaign_counters`,
-- `coupon_instances`, `redemption_*`, `issuance_deduplications`, `outbox_events` and
-- `job_registry`. What is missing there are the columns and invariants that only matter
-- once a campaign is actually published and a coupon is actually spent, so this migration
-- adds those rather than reshaping anything.
--
-- sqlx wraps each migration in its own transaction, so this file must not open one.

-- ---------------------------------------------------------------------------
-- Campaigns (§8.4, §8.5, §11.4, CAMPAIGN-001…008)
-- ---------------------------------------------------------------------------

-- Who the campaign is for, frozen at publish time so a later profile change cannot
-- retroactively alter who was eligible (CAMPAIGN-003 step 2).
ALTER TABLE coupon.campaigns
    ADD COLUMN IF NOT EXISTS audience_type varchar(32) NOT NULL DEFAULT 'ALL_CUSTOMERS',
    ADD COLUMN IF NOT EXISTS audience_criteria jsonb NOT NULL DEFAULT '{}'::jsonb,
    ADD COLUMN IF NOT EXISTS audience_snapshot_at timestamptz,
    ADD COLUMN IF NOT EXISTS audience_size integer,
    -- §8.4: 총수량 무제한은 *운영 상한을 가진 별도 표현*이며 DB 정수 최대값을 쓰지 않는다.
    -- `total_quantity IS NULL` alone would be "no ceiling at all"; the cap is what makes
    -- unlimited an operational decision rather than an unbounded liability.
    ADD COLUMN IF NOT EXISTS unlimited_total_cap bigint,
    -- §8.4: fixed on the campaign, default false. Whether a revoked coupon frees its slot.
    ADD COLUMN IF NOT EXISTS revoke_policy varchar(32) NOT NULL DEFAULT 'KEEP_ISSUED',
    ADD COLUMN IF NOT EXISTS cancellation_reason text,
    ADD COLUMN IF NOT EXISTS emergency_stopped_at timestamptz,
    ADD COLUMN IF NOT EXISTS emergency_stopped_by_user_id uuid
        REFERENCES coupon.users(id) ON DELETE SET NULL,
    -- The `operation_version` component of an issuing job's unique key (§14.3). Bumped
    -- whenever a *new* issuing run is legitimately wanted (resume after pause, admin
    -- reprocess), so the old key's completed job is never confused with the new run.
    ADD COLUMN IF NOT EXISTS issue_generation integer NOT NULL DEFAULT 1,
    ADD COLUMN IF NOT EXISTS notification_channels jsonb NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS notification_message text;

ALTER TABLE coupon.campaigns
    DROP CONSTRAINT IF EXISTS ck_campaign_audience_type;
ALTER TABLE coupon.campaigns
    ADD CONSTRAINT ck_campaign_audience_type CHECK (
        audience_type IN (
            'ALL_CUSTOMERS', 'FAVORITE_CUSTOMERS', 'RECENT_VISITORS',
            'STAMP_THRESHOLD', 'SPECIFIC_USERS'
        )
    );

ALTER TABLE coupon.campaigns
    DROP CONSTRAINT IF EXISTS ck_campaign_revoke_policy;
ALTER TABLE coupon.campaigns
    ADD CONSTRAINT ck_campaign_revoke_policy CHECK (
        revoke_policy IN ('KEEP_ISSUED', 'REVOKE_UNUSED')
    );

ALTER TABLE coupon.campaigns
    DROP CONSTRAINT IF EXISTS ck_campaign_issue_generation;
ALTER TABLE coupon.campaigns
    ADD CONSTRAINT ck_campaign_issue_generation CHECK (issue_generation >= 1);

-- Exactly one of the two quantity expressions. A campaign that names neither has no
-- ceiling of any kind, which §8.4 does not allow.
ALTER TABLE coupon.campaigns
    DROP CONSTRAINT IF EXISTS ck_campaign_quantity_expression;
ALTER TABLE coupon.campaigns
    ADD CONSTRAINT ck_campaign_quantity_expression CHECK (
        (total_quantity IS NOT NULL AND unlimited_total_cap IS NULL)
        OR (total_quantity IS NULL AND unlimited_total_cap IS NOT NULL)
    );

ALTER TABLE coupon.campaigns
    DROP CONSTRAINT IF EXISTS ck_campaign_unlimited_cap;
ALTER TABLE coupon.campaigns
    ADD CONSTRAINT ck_campaign_unlimited_cap CHECK (
        unlimited_total_cap IS NULL OR unlimited_total_cap BETWEEN 1 AND 1000000
    );

-- §12.6-4, restated so the operational cap binds an "unlimited" campaign too. This is the
-- database's own backstop: even if every application guard were bypassed, the counters
-- cannot climb past the ceiling the campaign was published with.
ALTER TABLE coupon.campaigns
    DROP CONSTRAINT IF EXISTS ck_campaign_global_counts;
ALTER TABLE coupon.campaigns
    ADD CONSTRAINT ck_campaign_global_counts CHECK (
        global_reserved_count >= 0 AND global_issued_count >= 0 AND global_revoked_count >= 0
        AND global_reserved_count + global_issued_count
            <= COALESCE(total_quantity, unlimited_total_cap, 1000000)
    );

-- The claim path reads `(campaign_id, business_day)` and the issuing worker reads the
-- campaign's whole counter history; both are covered by `uq_campaign_counters_day`.
-- What is not covered is the owner's progress view, which walks days in order.
CREATE INDEX IF NOT EXISTS ix_campaign_counters_progress
    ON coupon.campaign_counters (campaign_id, business_day DESC);

-- CAMPAIGN-003 step 3: the issuing worker pages targets in a stable id order and resumes
-- from its checkpoint, so the scan is `(campaign_id, status, user_id)` — already indexed —
-- plus a way to find what one job produced.
ALTER TABLE coupon.campaign_audience_members
    ADD COLUMN IF NOT EXISTS issued_job_id uuid REFERENCES coupon.job_registry(id) ON DELETE SET NULL;

-- ---------------------------------------------------------------------------
-- Coupon instances (§8.5)
-- ---------------------------------------------------------------------------

-- Which job created this instance. A partially finished bulk issuance has to be
-- explainable after the fact, and "which run made this coupon" is the first question.
ALTER TABLE coupon.coupon_instances
    ADD COLUMN IF NOT EXISTS source_job_id uuid REFERENCES coupon.job_registry(id) ON DELETE SET NULL;

-- §12.6-8, the other half. `trg_coupon_status_events_chain` already checks that an event
-- describes a transition that really happened; this refuses the transition itself when it
-- is not one the state machine allows — a used coupon coming back to life, an expired one
-- being reserved. The Rust `CouponStatus::can_transition_to` says the same thing, and this
-- is the copy that cannot be forgotten by a new call site.
CREATE OR REPLACE FUNCTION coupon.enforce_coupon_status_transition()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.status = OLD.status THEN
        RETURN NEW;
    END IF;

    IF NOT (
        (OLD.status = 'PENDING' AND NEW.status IN ('AVAILABLE', 'ISSUE_FAILED'))
        OR (OLD.status = 'AVAILABLE'
            AND NEW.status IN ('RESERVED', 'USED', 'EXPIRED', 'REVOKED', 'VOIDED'))
        OR (OLD.status = 'RESERVED'
            AND NEW.status IN ('AVAILABLE', 'USED', 'EXPIRED', 'REVOKED', 'VOIDED'))
        -- REDEEM-004: a voided use may restore the coupon while it is still valid.
        OR (OLD.status = 'USED' AND NEW.status IN ('AVAILABLE', 'VOIDED'))
    ) THEN
        RAISE EXCEPTION 'coupon % may not move from % to %', OLD.id, OLD.status, NEW.status
            USING ERRCODE = '23514';
    END IF;

    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS trg_coupon_instances_transition ON coupon.coupon_instances;
CREATE TRIGGER trg_coupon_instances_transition
BEFORE UPDATE OF status ON coupon.coupon_instances
FOR EACH ROW EXECUTE FUNCTION coupon.enforce_coupon_status_transition();

-- ---------------------------------------------------------------------------
-- Redemption (§13.3, §8.6, REDEEM-001…006)
-- ---------------------------------------------------------------------------

ALTER TABLE coupon.redemption_reservations
    -- How the expected discount was arrived at, so `confirm` can be compared against what
    -- the owner was shown rather than only against a single number.
    ADD COLUMN IF NOT EXISTS discount_snapshot jsonb NOT NULL DEFAULT '{}'::jsonb,
    ADD COLUMN IF NOT EXISTS external_order_ref varchar(200),
    ADD COLUMN IF NOT EXISTS cancelled_reason text;

ALTER TABLE coupon.redemption_transactions
    ADD COLUMN IF NOT EXISTS discount_snapshot jsonb NOT NULL DEFAULT '{}'::jsonb,
    -- REDEEM-004: whether voiding put the coupon back, or left it for an administrator.
    ADD COLUMN IF NOT EXISTS coupon_restored boolean NOT NULL DEFAULT false;

-- §8.6 / §5.4: MVP 는 주문당 혜택 1개. Where the owner supplied a POS reference, the
-- database is what makes "one benefit per order" true rather than a rule the UI remembers.
CREATE UNIQUE INDEX IF NOT EXISTS uq_redemption_transactions_order_ref
    ON coupon.redemption_transactions (store_id, external_order_ref)
    WHERE status = 'CONFIRMED' AND external_order_ref IS NOT NULL;

-- REDEEM-002: 같은 점주 세션은 동시에 하나의 사용 예약만 가질 수 있다.
--
-- The Phase 1 index keyed this on `owner_session_id` alone, which reads the rule as
-- "one reservation per session string in the whole system". Session ids come from the
-- client, so two unrelated stores that both call their till `till-1` would block each
-- other — one shop's checkout failing because of another shop's. The rule is about one
-- owner's till, so the key is the owner and their session.
DROP INDEX IF EXISTS coupon.uq_redemption_reservations_active_owner_session;
CREATE UNIQUE INDEX IF NOT EXISTS uq_redemption_reservations_active_owner_session
    ON coupon.redemption_reservations (owner_user_id, owner_session_id)
    WHERE status = 'ACTIVE';

-- REDEEM-002: the sweep looks for reservations whose two minutes are up.
CREATE INDEX IF NOT EXISTS ix_redemption_reservations_store_status
    ON coupon.redemption_reservations (store_id, status, reserved_at DESC);

-- ---------------------------------------------------------------------------
-- Jobs (§14)
-- ---------------------------------------------------------------------------

ALTER TABLE coupon.job_registry
    -- The 64-bit advisory-lock key derived from `unique_key` (§14.5-5). Stored rather
    -- than only computed so an operator can see which lock a stuck job is waiting on.
    ADD COLUMN IF NOT EXISTS lock_key bigint,
    ADD COLUMN IF NOT EXISTS store_id uuid REFERENCES coupon.stores(id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS resource_id uuid,
    ADD COLUMN IF NOT EXISTS pause_requested_at timestamptz,
    ADD COLUMN IF NOT EXISTS paused_at timestamptz,
    ADD COLUMN IF NOT EXISTS dead_lettered_at timestamptz,
    -- §14.7: dead-letter 재처리는 원인 해결 확인, 관리자 사유, 새 generation을 요구한다.
    ADD COLUMN IF NOT EXISTS retry_of_job_id uuid REFERENCES coupon.job_registry(id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS retry_reason text,
    -- §14.5-9: how long a `RUNNING` job may go without a heartbeat before another worker
    -- may assume the previous one died and take the (already released) lock.
    ADD COLUMN IF NOT EXISTS visibility_timeout_secs integer NOT NULL DEFAULT 300;

ALTER TABLE coupon.job_registry
    DROP CONSTRAINT IF EXISTS ck_job_visibility_timeout;
ALTER TABLE coupon.job_registry
    ADD CONSTRAINT ck_job_visibility_timeout CHECK (visibility_timeout_secs BETWEEN 10 AND 3600);

CREATE INDEX IF NOT EXISTS ix_job_registry_admin_list
    ON coupon.job_registry (job_type, status, created_at DESC);
CREATE INDEX IF NOT EXISTS ix_job_registry_resource
    ON coupon.job_registry (resource_id, created_at DESC)
    WHERE resource_id IS NOT NULL;

-- §14.7: 각 시도에 동일 job ID와 새 attempt ID를 기록한다.
--
-- Kept separate from `job_registry` so the registry row stays one row per logical job —
-- which is what the active-key unique index depends on — while the history of *tries* can
-- grow without bound.
CREATE TABLE IF NOT EXISTS coupon.job_attempts (
    id uuid PRIMARY KEY DEFAULT public.gen_random_uuid(),
    job_id uuid NOT NULL REFERENCES coupon.job_registry(id) ON DELETE RESTRICT,
    attempt_no integer NOT NULL,
    generation integer NOT NULL,
    worker_id varchar(200) NOT NULL,
    status varchar(32) NOT NULL DEFAULT 'RUNNING',
    started_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    finished_at timestamptz,
    processed_count bigint NOT NULL DEFAULT 0,
    succeeded_count bigint NOT NULL DEFAULT 0,
    failed_count bigint NOT NULL DEFAULT 0,
    checkpoint jsonb NOT NULL DEFAULT '{}'::jsonb,
    error_code varchar(100),
    error_message text,
    -- Whether this attempt's failure was classified as retryable (§14.7).
    retryable boolean,
    next_attempt_at timestamptz,
    CONSTRAINT uq_job_attempt_number UNIQUE (job_id, generation, attempt_no),
    CONSTRAINT ck_job_attempt_no CHECK (attempt_no >= 1),
    CONSTRAINT ck_job_attempt_status CHECK (
        status IN ('RUNNING', 'SUCCEEDED', 'FAILED', 'LOCK_CONTENDED', 'ABANDONED')
    ),
    CONSTRAINT ck_job_attempt_counts CHECK (
        processed_count >= 0 AND succeeded_count >= 0 AND failed_count >= 0
    )
);

CREATE INDEX IF NOT EXISTS ix_job_attempts_job
    ON coupon.job_attempts (job_id, started_at DESC);

-- ---------------------------------------------------------------------------
-- Administrative execution (§11.5, §3.3, ADMIN-003, ADMIN-004)
-- ---------------------------------------------------------------------------

ALTER TABLE coupon.admin_adjustments
    ADD COLUMN IF NOT EXISTS execution_result jsonb NOT NULL DEFAULT '{}'::jsonb,
    ADD COLUMN IF NOT EXISTS approval_reason text;

-- ADMIN-004's 도장 보정 puts stamps on a board with no accrual behind them, so an
-- administrative lot has no source transaction to point at. Inventing a synthetic
-- `stamp_transactions` row would be worse: it would claim a sale that never happened, and
-- every report counting transactions would then be wrong.
--
-- `uq_stamp_lot_source` still holds — NULLs do not collide in a unique index — so an
-- accrual still produces at most one lot.
ALTER TABLE coupon.stamp_lots
    ALTER COLUMN source_transaction_id DROP NOT NULL;

-- The 1–10 ceiling is §8.1's *주문당 적립* range, and it belongs to accruals. A
-- correction is bounded by what an administrator may request (§11.5), not by what a
-- single order may earn.
ALTER TABLE coupon.stamp_lots
    DROP CONSTRAINT IF EXISTS ck_stamp_lot_quantity;
ALTER TABLE coupon.stamp_lots
    ADD CONSTRAINT ck_stamp_lot_quantity CHECK (
        (source_transaction_id IS NOT NULL AND original_quantity BETWEEN 1 AND 10)
        OR (source_transaction_id IS NULL AND original_quantity BETWEEN 1 AND 100)
    );

-- `admin_adjustments` already carries `ck_admin_adjustment_separation`
-- (approver <> requester, §3.3). Nothing to add: the rule is already the database's.

-- ---------------------------------------------------------------------------
-- Timestamps for the new table
-- ---------------------------------------------------------------------------

INSERT INTO coupon.schema_metadata (key, value)
VALUES ('schema_version', '20260812000300')
ON CONFLICT (key) DO UPDATE
SET value = EXCLUDED.value,
    updated_at = clock_timestamp();
