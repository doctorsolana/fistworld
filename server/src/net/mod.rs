//! Networking domain.
//!
//! Responsibilities:
//! - Client connection lifecycle.
//! - Peer identity translation.
//! - Input ingress and buffering.
//!
//! Dependency notes:
//! - May depend on `shared`.
//! - Should not depend on gameplay domains.

pub mod connection;
pub mod input;
pub mod peer;
