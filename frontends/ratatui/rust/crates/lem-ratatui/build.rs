//! With the `bundle` feature, compress the saved Lisp image into OUT_DIR
//! for `src/bundle.rs` to embed, and export a hash of it as the cache key.
//! Without the feature this does nothing.

fn main() {
    #[cfg(feature = "bundle")]
    bundle::compress();
}

#[cfg(feature = "bundle")]
mod bundle {
    use std::hash::{DefaultHasher, Hasher};
    use std::path::PathBuf;
    use std::{env, fs};

    /// High enough to shrink the image well, low enough that a dist build
    /// isn't dominated by it.
    const LEVEL: i32 = 15;

    pub fn compress() {
        println!("cargo::rerun-if-env-changed=LEM_RATATUI_LISP");
        let image = env::var_os("LEM_RATATUI_LISP").map_or_else(
            || {
                PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
                    .join("../../../dist/lem-ratatui-lisp")
            },
            PathBuf::from,
        );
        println!("cargo::rerun-if-changed={}", image.display());

        let raw = fs::read(&image).unwrap_or_else(|e| {
            panic!(
                "reading Lisp image {}: {e}\n\
                 build it first (`make lisp` in frontends/ratatui) or set LEM_RATATUI_LISP",
                image.display()
            )
        });

        // Only has to agree with itself within one build, so std's hasher
        // being unstable across Rust releases doesn't matter.
        let mut hasher = DefaultHasher::new();
        hasher.write(&raw);
        println!(
            "cargo::rustc-env=LEM_RATATUI_BUNDLE_HASH={:016x}",
            hasher.finish()
        );

        let mut encoder = zstd::Encoder::new(Vec::new(), LEVEL).unwrap();
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get() as u32);
        encoder.multithread(threads).unwrap();
        std::io::Write::write_all(&mut encoder, &raw).unwrap();
        let packed = encoder.finish().unwrap();

        let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("lem-ratatui-lisp.zst");
        fs::write(&out, packed).unwrap();
    }
}
