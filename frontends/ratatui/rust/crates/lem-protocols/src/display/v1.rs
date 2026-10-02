//! `lem.display.v1`: the protocol between `lem-display-relay` and a display.
//!
//! The types are generated from
//! `frontends/ratatui/proto/lem/display/v1/display.proto` by `build.rs`; the
//! schema, with its comments, is the reference. Framing on a stream is
//! [`crate::framing`], shared by every link.

mod generated {
    // Generated code: its style is prost's, not ours.
    #![allow(clippy::all)]
    include!(concat!(env!("OUT_DIR"), "/lem.display.v1.rs"));
}
pub use generated::*;

/// The revision of this schema this crate was built against (`Hello`).
pub const PROTOCOL_VERSION: u32 = 2;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_schemas_enums_lose_their_prefixes() {
        // display.proto prefixes enum values with their type, as protobuf
        // advises; prost strips the prefix.
        assert_eq!(ViewKind::Floating as i32, 3);
        assert_eq!(NamedKey::PageUp as i32, 13);
        assert_eq!(CursorShape::Bar as i32, 2);
    }
}
