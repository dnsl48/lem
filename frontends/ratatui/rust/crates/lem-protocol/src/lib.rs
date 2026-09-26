//! The display protocol: `lem.relay.v1`.
//!
//! This crate deliberately knows nothing about terminals. Everything here
//! is decoding and shape: given the bytes the relay sends, produce typed
//! messages. That keeps the protocol testable against captured fixtures
//! with no TTY attached, which is why it is a crate of its own, apart from
//! `lem-ratatui`.
//!
//! [`v1`] is generated from `frontends/ratatui/proto/lem/relay/v1/relay.proto`,
//! the schema both halves are built from (ADR 0010); the other side of the
//! wire is `frontends/ratatui/relay/protobuf/`. How the protocol behaves is
//! described in `../../../docs/protocol.md`.

pub mod v1;
