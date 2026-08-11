//! Request middleware.
//!
//! Order matters, and the router applies them so they run:
//!
//! 1. [`request_id`] — every later layer and every log line needs the id.
//! 2. [`origin`]     — reject a cross-site state change before doing any work.
//! 3. [`auth`]       — establish who the caller is, from the database.
//! 4. [`idempotency`] — needs the actor from (3) to key the stored response.

pub mod auth;
pub mod idempotency;
pub mod origin;
pub mod request_id;
