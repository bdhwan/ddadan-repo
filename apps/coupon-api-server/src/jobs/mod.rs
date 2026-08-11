//! 작업 등록·실행·체크포인트·DLQ (§10.2 `jobs`, §14).
//!
//! Phase 4. Owns `job_registry` and `outbox_events`.
//!
//! Reserved now so that cross-module wiring and the §10.2 boundary are settled before the
//! phase lands. This module has no public surface yet.
//!
//! - TODO(phase-4): deterministic unique keys with at most one QUEUED/RUNNING/RETRY_WAIT job
//!   per key (§12.6-10).
//! - TODO(phase-4): Redis as transport only — PostgreSQL stays the source of truth so a lost
//!   Redis rebuilds from the outbox (§18.5).
//! - TODO(phase-4): checkpointed resume, capped retries, and a dead-letter queue that alerts
//!   on first entry (§14.7).
