//! Ambient request id.
//!
//! `IntoResponse` has no access to the request, but every error body has to carry the
//! request id (§11.1). A task-local set by the request-id middleware bridges that gap,
//! and works from handlers, extractors and error conversions alike.

use uuid::Uuid;

tokio::task_local! {
    static REQUEST_ID: String;
}

/// Human-scannable, greppable, and obviously not a UUID a client should reuse.
pub fn generate() -> String {
    format!("req_{}", Uuid::new_v4().simple())
}

/// The current request's id, if we are inside a request.
pub fn current() -> Option<String> {
    REQUEST_ID.try_with(|id| id.clone()).ok()
}

/// Run `future` with `request_id` visible to [`current`].
pub async fn scope<F: Future>(request_id: String, future: F) -> F::Output {
    REQUEST_ID.scope(request_id, future).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_ids_are_prefixed_and_unique() {
        let first = generate();
        let second = generate();

        assert!(first.starts_with("req_"));
        assert_ne!(first, second);
    }

    #[tokio::test]
    async fn current_is_none_outside_a_request() {
        assert_eq!(current(), None);
    }

    #[tokio::test]
    async fn current_reads_the_scoped_id() {
        let seen = scope("req_abc".to_owned(), async { current() }).await;
        assert_eq!(seen.as_deref(), Some("req_abc"));
    }
}
