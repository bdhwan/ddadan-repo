//! 작업 전달 (§14.2, JOB-005).
//!
//! Apalis over Redis carries messages, scheduled delivery and re-queue delay. It carries
//! **nothing else**: a message is a bare `job_id`, and every worker re-reads the registry
//! before acting on it (§14.5-4). That is deliberate — it is what lets JOB-005 be true.
//! When Redis is unreachable the outbox row simply stays unpublished, the API's own
//! response never claimed the work had started, and the worker's registry poll picks the
//! job up anyway. Losing Redis costs latency, not correctness.

use std::pin::Pin;
use std::time::Duration;

use apalis::prelude::Storage;
use apalis_redis::RedisStorage;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A boxed future, so [`JobTransport`] stays object-safe and the worker can hold whichever
/// transport it was configured with behind one `dyn`.
pub type BoxFuture<'a, T> = Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

/// Everything that travels over the wire. §14.5-3: `job_id` only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobMessage {
    pub job_id: Uuid,
}

/// The Redis queue namespace, so a shared Redis cannot mix our jobs with anyone else's.
pub const QUEUE_NAMESPACE: &str = "ddadan:coupon:jobs";

pub trait JobTransport: Send + Sync {
    /// Deliver as soon as a worker is free.
    fn publish(&self, job_id: Uuid) -> BoxFuture<'_, anyhow::Result<()>>;

    /// Deliver after `delay`. Used for §14.5-6 (lock contention: re-queue with jitter, and
    /// do *not* count it as a failed attempt) and for §14.7's retry schedule.
    fn publish_after(&self, job_id: Uuid, delay: Duration) -> BoxFuture<'_, anyhow::Result<()>>;
}

/// The real transport.
#[derive(Clone)]
pub struct RedisJobTransport {
    storage: RedisStorage<JobMessage>,
}

impl std::fmt::Debug for RedisJobTransport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("RedisJobTransport").finish()
    }
}

impl RedisJobTransport {
    pub async fn connect(redis_url: &str) -> anyhow::Result<Self> {
        let connection = apalis_redis::connect(redis_url).await?;
        let config = apalis_redis::Config::default().set_namespace(QUEUE_NAMESPACE);
        Ok(Self {
            storage: RedisStorage::new_with_config(connection, config),
        })
    }

    /// The backend the worker's consuming side polls.
    pub fn storage(&self) -> RedisStorage<JobMessage> {
        self.storage.clone()
    }
}

impl JobTransport for RedisJobTransport {
    fn publish(&self, job_id: Uuid) -> BoxFuture<'_, anyhow::Result<()>> {
        // `push` needs `&mut`, and the storage is cheap to clone (the connection manager
        // behind it is shared), so each publish takes its own handle rather than putting
        // a mutex in front of the queue.
        let mut storage = self.storage.clone();
        Box::pin(async move {
            storage.push(JobMessage { job_id }).await?;
            Ok(())
        })
    }

    fn publish_after(&self, job_id: Uuid, delay: Duration) -> BoxFuture<'_, anyhow::Result<()>> {
        let mut storage = self.storage.clone();
        Box::pin(async move {
            let at = chrono::Utc::now() + chrono::Duration::from_std(delay).unwrap_or_default();
            storage.schedule(JobMessage { job_id }, at.timestamp()).await?;
            Ok(())
        })
    }
}

/// The degraded transport (JOB-005).
///
/// Publishing is a no-op that succeeds, because the job is already durably registered in
/// PostgreSQL and the worker's own registry poll will find it. It is used when no Redis is
/// configured at all, and it is why the tests can exercise the whole queue without one.
#[derive(Debug, Clone, Copy, Default)]
pub struct RegistryOnlyTransport;

impl JobTransport for RegistryOnlyTransport {
    fn publish(&self, job_id: Uuid) -> BoxFuture<'_, anyhow::Result<()>> {
        Box::pin(async move {
            tracing::debug!(%job_id, "no job transport configured; the registry poll will pick this up");
            Ok(())
        })
    }

    fn publish_after(&self, job_id: Uuid, delay: Duration) -> BoxFuture<'_, anyhow::Result<()>> {
        Box::pin(async move {
            tracing::debug!(
                %job_id,
                delay_secs = delay.as_secs(),
                "no job transport configured; the delay is honoured by next_attempt_at"
            );
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_carries_nothing_but_the_job_id() {
        // §14.5-3. If this ever grows a field, a worker could start acting on the message
        // instead of on the registry, which is the whole failure mode this prevents.
        let message = JobMessage {
            job_id: Uuid::from_u128(7),
        };
        let json = serde_json::to_value(&message).expect("serialises");

        assert_eq!(json.as_object().expect("object").len(), 1);
        assert_eq!(json["job_id"], Uuid::from_u128(7).to_string());
    }

    #[tokio::test]
    async fn the_registry_only_transport_never_fails_a_publish() {
        // JOB-005: 큐 등록 실패를 사용자 성공 응답과 분리한다. With no queue at all the
        // registry is still the source of truth, so publishing must not become an error
        // that unwinds a committed domain change.
        let transport = RegistryOnlyTransport;
        assert!(transport.publish(Uuid::new_v4()).await.is_ok());
        assert!(
            transport
                .publish_after(Uuid::new_v4(), Duration::from_secs(30))
                .await
                .is_ok()
        );
    }
}
