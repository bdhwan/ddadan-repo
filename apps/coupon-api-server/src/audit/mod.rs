//! 관리자·고위험 행위 감사 (§10.2 `audit`, §12.5, SEC-005).
//!
//! `coupon.audit_logs` is append-only at the database level — a trigger rejects UPDATE and
//! DELETE — so this module only ever inserts.
//!
//! Entries are hash-chained *per resource* rather than globally. A global chain would
//! serialise every administrative action behind one row to read the predecessor, and the
//! question an investigation actually asks is "has the history of *this* transaction been
//! altered", which a per-resource chain answers just as well (§12.5: 변조 탐지).

use chrono::{DateTime, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::Tx;
use crate::error::ApiResult;
use crate::http::request_id;

/// `coupon.actor_type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ActorType {
    User,
    StoreOwner,
    SystemAdmin,
    System,
    Provider,
}

impl ActorType {
    pub fn as_db(self) -> &'static str {
        match self {
            ActorType::User => "USER",
            ActorType::StoreOwner => "STORE_OWNER",
            ActorType::SystemAdmin => "SYSTEM_ADMIN",
            ActorType::System => "SYSTEM",
            ActorType::Provider => "PROVIDER",
        }
    }
}

/// One thing that happened, ready to be written.
#[derive(Debug, Clone)]
pub struct AuditEntry {
    pub actor_type: ActorType,
    pub actor_user_id: Option<Uuid>,
    /// Verb, e.g. `stamp_transaction.confirmed`.
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<Uuid>,
    pub store_id: Option<Uuid>,
    pub case_id: Option<Uuid>,
    /// Why. Required by §11.5 for administrative changes; optional for system events.
    pub reason: Option<String>,
    pub metadata: serde_json::Value,
    pub before_hash: Option<String>,
    pub after_hash: Option<String>,
}

impl AuditEntry {
    pub fn new(actor_type: ActorType, action: impl Into<String>, resource_type: impl Into<String>) -> Self {
        Self {
            actor_type,
            actor_user_id: None,
            action: action.into(),
            resource_type: resource_type.into(),
            resource_id: None,
            store_id: None,
            case_id: None,
            reason: None,
            metadata: serde_json::json!({}),
            before_hash: None,
            after_hash: None,
        }
    }

    pub fn actor(mut self, user_id: Uuid) -> Self {
        self.actor_user_id = Some(user_id);
        self
    }

    pub fn resource(mut self, resource_id: Uuid) -> Self {
        self.resource_id = Some(resource_id);
        self
    }

    pub fn store(mut self, store_id: Uuid) -> Self {
        self.store_id = Some(store_id);
        self
    }

    pub fn case(mut self, case_id: Uuid) -> Self {
        self.case_id = Some(case_id);
        self
    }

    pub fn reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    pub fn metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }

    /// Record what a resource looked like before and after, without copying the values
    /// themselves into the audit trail (§12.5 stores hashes, not payloads).
    pub fn transition(mut self, before: &serde_json::Value, after: &serde_json::Value) -> Self {
        self.before_hash = Some(state_hash(before));
        self.after_hash = Some(state_hash(after));
        self
    }
}

/// Stable hash of a resource snapshot.
pub fn state_hash(value: &serde_json::Value) -> String {
    // `serde_json::Value` orders object keys deterministically (a BTreeMap), so the same
    // logical state always hashes the same way.
    hex::encode(Sha256::digest(value.to_string().as_bytes()))
}

/// Link one entry to its predecessor.
pub fn chain_hash(previous: Option<&str>, entry: &AuditEntry, occurred_at: DateTime<Utc>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(previous.unwrap_or("genesis").as_bytes());
    hasher.update(b"\0");
    hasher.update(entry.action.as_bytes());
    hasher.update(b"\0");
    hasher.update(entry.resource_type.as_bytes());
    hasher.update(b"\0");
    hasher.update(
        entry
            .resource_id
            .map(|id| id.to_string())
            .unwrap_or_default()
            .as_bytes(),
    );
    hasher.update(b"\0");
    hasher.update(
        entry
            .actor_user_id
            .map(|id| id.to_string())
            .unwrap_or_default()
            .as_bytes(),
    );
    hasher.update(b"\0");
    hasher.update(entry.before_hash.as_deref().unwrap_or_default().as_bytes());
    hasher.update(b"\0");
    hasher.update(entry.after_hash.as_deref().unwrap_or_default().as_bytes());
    hasher.update(b"\0");
    hasher.update(occurred_at.timestamp_micros().to_be_bytes());
    hex::encode(hasher.finalize())
}

pub struct AuditService;

impl Default for AuditService {
    fn default() -> Self {
        Self::new()
    }
}

impl AuditService {
    pub fn new() -> Self {
        Self
    }

    /// Append an entry inside the caller's transaction.
    ///
    /// Transactional on purpose: an audit record that can be lost while the change it
    /// describes survives is worse than no audit trail at all, because it looks complete.
    pub async fn record(&self, tx: &mut Tx<'_>, entry: AuditEntry) -> ApiResult<Uuid> {
        let occurred_at = Utc::now();

        // Read the predecessor under the transaction so two entries for one resource
        // cannot both chain from the same parent.
        let previous: Option<String> = sqlx::query_scalar!(
            r#"
            SELECT entry_hash
            FROM coupon.audit_logs
            WHERE resource_type = $1 AND resource_id IS NOT DISTINCT FROM $2
            ORDER BY occurred_at DESC, id DESC
            LIMIT 1
            "#,
            entry.resource_type,
            entry.resource_id,
        )
        .fetch_optional(&mut **tx)
        .await?
        .flatten();

        let entry_hash = chain_hash(previous.as_deref(), &entry, occurred_at);

        let id = sqlx::query_scalar!(
            r#"
            INSERT INTO coupon.audit_logs
                (actor_type, actor_user_id, action, resource_type, resource_id, store_id,
                 case_id, reason, request_id, metadata, before_hash, after_hash,
                 previous_entry_hash, entry_hash, occurred_at)
            VALUES ($1::text::coupon.actor_type, $2, $3, $4, $5, $6, $7, $8, $9, $10,
                    $11, $12, $13, $14, $15)
            RETURNING id
            "#,
            entry.actor_type.as_db(),
            entry.actor_user_id,
            entry.action,
            entry.resource_type,
            entry.resource_id,
            entry.store_id,
            entry.case_id,
            entry.reason,
            request_id::current(),
            entry.metadata,
            entry.before_hash,
            entry.after_hash,
            previous,
            entry_hash,
            occurred_at,
        )
        .fetch_one(&mut **tx)
        .await?;

        Ok(id)
    }

    /// Append an entry that has no surrounding transaction — an administrative *read*,
    /// which still has to be audited (SEC-005) but changes nothing.
    pub async fn record_standalone(&self, pool: &PgPool, entry: AuditEntry) -> ApiResult<Uuid> {
        let mut tx = pool.begin().await?;
        let id = self.record(&mut tx, entry).await?;
        tx.commit().await?;
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> AuditEntry {
        AuditEntry::new(ActorType::SystemAdmin, "transaction.viewed", "stamp_transaction")
            .resource(Uuid::from_u128(1))
            .actor(Uuid::from_u128(2))
    }

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).expect("valid timestamp")
    }

    #[test]
    fn the_same_state_always_hashes_the_same() {
        let first = serde_json::json!({ "status": "CONFIRMED", "quantity": 1 });
        let second = serde_json::json!({ "quantity": 1, "status": "CONFIRMED" });

        assert_eq!(
            state_hash(&first),
            state_hash(&second),
            "key order must not change the hash"
        );
        assert_ne!(
            state_hash(&first),
            state_hash(&serde_json::json!({ "status": "VOIDED", "quantity": 1 }))
        );
    }

    #[test]
    fn a_transition_records_both_sides_without_the_values() {
        let before = serde_json::json!({ "status": "CONFIRMED" });
        let after = serde_json::json!({ "status": "VOIDED" });
        let recorded = entry().transition(&before, &after);

        assert_eq!(recorded.before_hash.as_deref(), Some(state_hash(&before).as_str()));
        assert_ne!(recorded.before_hash, recorded.after_hash);
    }

    #[test]
    fn changing_anything_changes_the_chain_hash() {
        let baseline = chain_hash(Some("prev"), &entry(), at(1_000));

        assert_ne!(baseline, chain_hash(Some("other"), &entry(), at(1_000)));
        assert_ne!(baseline, chain_hash(Some("prev"), &entry(), at(1_001)));
        assert_ne!(
            baseline,
            chain_hash(
                Some("prev"),
                &AuditEntry::new(ActorType::SystemAdmin, "transaction.voided", "stamp_transaction")
                    .resource(Uuid::from_u128(1))
                    .actor(Uuid::from_u128(2)),
                at(1_000),
            )
        );
    }

    #[test]
    fn the_first_entry_for_a_resource_chains_from_a_fixed_anchor() {
        assert_eq!(
            chain_hash(None, &entry(), at(1_000)),
            chain_hash(Some("genesis"), &entry(), at(1_000)),
        );
    }

    #[test]
    fn actor_types_use_the_database_spelling() {
        assert_eq!(ActorType::SystemAdmin.as_db(), "SYSTEM_ADMIN");
        assert_eq!(ActorType::StoreOwner.as_db(), "STORE_OWNER");
    }
}
