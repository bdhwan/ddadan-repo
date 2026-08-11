//! 회전형 QR 발급과 소비 (§10.2 `qr`, §16.2).
//!
//! Phase 2. Owns `qr_nonces`.
//!
//! Reserved now so that cross-module wiring and the §10.2 boundary are settled before the
//! phase lands. This module has no public surface yet.
//!
//! - TODO(phase-2): 60-second rotating tokens carrying only version, nonce, opaque subject,
//!   audience and timestamps — never the consumer key or any personal data.
//! - TODO(phase-2): Ed25519/ES256 signing with a key id, and storing only the nonce *hash*.
//! - TODO(phase-2): single-use consumption, linking a nonce to at most one successful
//!   transaction (§12.6-7).
