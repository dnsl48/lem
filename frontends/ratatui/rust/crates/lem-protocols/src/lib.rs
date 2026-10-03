//! Lem's protocols, one protobuf package per link (ADR 0019).
//!
//! This crate deliberately knows nothing about terminals. Everything here
//! is decoding and shape: given the bytes a peer sends, produce typed
//! messages. That keeps each protocol testable against captured fixtures
//! with no TTY attached, which is why it is a crate of its own, apart from
//! the processes that speak them.
//!
//! - [`display::v1`] is `lem.display.v1`, generated from
//!   `frontends/ratatui/proto/lem/display/v1/display.proto` (ADR 0010); the
//!   other side of the wire is `frontends/ratatui/display-relay/protobuf/`.
//!   How it behaves is described in `../../../docs/protocol.md`.
//! - [`os::v1`] is `lem.os.v1`, generated from
//!   `frontends/ratatui/proto/lem/os/v1/os.proto` (ADR 0019); the other
//!   side of the wire is `frontends/ratatui/os-agent/`.
//! - [`framing`] is the length-delimited framing every link uses.

pub mod display;
pub mod framing;
pub mod os;
