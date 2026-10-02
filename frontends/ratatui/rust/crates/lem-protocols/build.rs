//! Generates Lem's protocols from the schemas in `frontends/ratatui/proto`,
//! the ones both sides of each link are built from (ADR 0010, 0019).
//!
//! protox compiles the `.proto` files in pure Rust, so building this crate
//! needs no `protoc` and no C++ toolchain; only the Lisp side does.

const PROTO_ROOT: &str = "../../../proto";
const SCHEMAS: &[&str] = &["lem/display/v1/display.proto"];

fn main() {
    for schema in SCHEMAS {
        println!("cargo:rerun-if-changed={PROTO_ROOT}/{schema}");
    }
    let descriptors = protox::compile(SCHEMAS, [PROTO_ROOT]).expect("compiling the schemas");
    prost_build::Config::new()
        .compile_fds(descriptors)
        .expect("generating Rust from the schemas");
}
