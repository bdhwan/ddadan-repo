//! Success envelope (§11.1).
//!
//! Every success carries `request_id`; every successful mutation also carries
//! `transaction_id`, so a client's retry story and our logs share one identifier.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::http::request_id;

/// Identifier for one logical change. Written to the domain tables and echoed to the
/// client so a support ticket can be traced end to end (§18.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransactionId(pub Uuid);

impl TransactionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl Default for TransactionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for TransactionId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Envelope<T> {
    pub data: T,
    pub request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction_id: Option<String>,
}

/// A read that succeeded.
#[derive(Debug)]
pub struct ApiOk<T>(pub T);

/// A change that succeeded. `status` distinguishes 200 from 201.
#[derive(Debug)]
pub struct ApiMutation<T> {
    pub data: T,
    pub status: StatusCode,
    pub transaction_id: TransactionId,
}

impl<T> ApiMutation<T> {
    pub fn ok(data: T, transaction_id: TransactionId) -> Self {
        Self {
            data,
            status: StatusCode::OK,
            transaction_id,
        }
    }

    pub fn created(data: T, transaction_id: TransactionId) -> Self {
        Self {
            data,
            status: StatusCode::CREATED,
            transaction_id,
        }
    }
}

impl<T: Serialize> IntoResponse for ApiOk<T> {
    fn into_response(self) -> Response {
        Json(Envelope {
            data: self.0,
            request_id: request_id::current().unwrap_or_default(),
            transaction_id: None,
        })
        .into_response()
    }
}

impl<T: Serialize> IntoResponse for ApiMutation<T> {
    fn into_response(self) -> Response {
        let body = Envelope {
            data: self.data,
            request_id: request_id::current().unwrap_or_default(),
            transaction_id: Some(self.transaction_id.to_string()),
        };
        (self.status, Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize)]
    struct Dummy {
        id: &'static str,
    }

    #[test]
    fn reads_omit_transaction_id_entirely() {
        let json = serde_json::to_value(Envelope {
            data: Dummy { id: "u1" },
            request_id: "req_1".to_owned(),
            transaction_id: None,
        })
        .expect("serialises");

        assert_eq!(json["request_id"], "req_1");
        assert!(
            json.get("transaction_id").is_none(),
            "a read must not advertise a transaction"
        );
    }

    #[test]
    fn mutations_carry_a_transaction_id() {
        let transaction = TransactionId::new();
        let json = serde_json::to_value(Envelope {
            data: Dummy { id: "u1" },
            request_id: "req_1".to_owned(),
            transaction_id: Some(transaction.to_string()),
        })
        .expect("serialises");

        assert_eq!(json["transaction_id"], transaction.to_string());
    }
}
