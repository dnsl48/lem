//! Generates `lem.relay.v1` from the schema in `frontends/ratatui/proto`,
//! the one both halves are built from (ADR 0010).
//!
//! protox compiles the `.proto` in pure Rust, so building this crate needs
//! no `protoc` and no C++ toolchain; only the Lisp half does.

const PROTO_ROOT: &str = "../../../proto";
const SCHEMA: &str = "lem/relay/v1/relay.proto";

fn main() {
    println!("cargo:rerun-if-changed={PROTO_ROOT}/{SCHEMA}");
    let descriptors = protox::compile([SCHEMA], [PROTO_ROOT]).expect("compiling relay.proto");
    prost_build::Config::new()
        .compile_fds(descriptors)
        .expect("generating Rust from relay.proto");
}
