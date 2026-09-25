//! With the `bundle` feature, compress the saved Lisp image and the display
//! binary into OUT_DIR for `src/bundle.rs` to embed, and export a hash of
//! both as the cache key. Without the feature this does nothing.

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

    /// What gets embedded: the variable naming each file, where it is
    /// looked for otherwise (relative to this crate), and the name it is
    /// written under in OUT_DIR.
    const PAYLOADS: [(&str, &str, &str); 2] = [
        (
            "LEM_RATATUI_LISP",
            "../../../dist/lem-ratatui-lisp",
            "lem-ratatui-lisp.zst",
        ),
        (
            "LEM_RATATUI_TERMINAL",
            "../../target/dist/lem-ratatui",
            "lem-ratatui.zst",
        ),
    ];

    pub fn compress() {
        let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
        let out = PathBuf::from(env::var("OUT_DIR").unwrap());

        // Only has to agree with itself within one build, so std's hasher
        // being unstable across Rust releases doesn't matter.
        let mut hasher = DefaultHasher::new();
        for (var, default, name) in PAYLOADS {
            println!("cargo::rerun-if-env-changed={var}");
            let path = env::var_os(var).map_or_else(|| manifest.join(default), PathBuf::from);
            println!("cargo::rerun-if-changed={}", path.display());

            let raw = fs::read(&path).unwrap_or_else(|e| {
                panic!(
                    "reading {}: {e}\n\
                     build it first (`make dist` in frontends/ratatui does) or set {var}",
                    path.display()
                )
            });
            hasher.write(&raw);
            fs::write(out.join(name), pack(&raw)).unwrap();
        }
        println!(
            "cargo::rustc-env=LEM_RATATUI_BUNDLE_HASH={:016x}",
            hasher.finish()
        );
    }

    fn pack(raw: &[u8]) -> Vec<u8> {
        let mut encoder = zstd::Encoder::new(Vec::new(), LEVEL).unwrap();
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get() as u32);
        encoder.multithread(threads).unwrap();
        std::io::Write::write_all(&mut encoder, raw).unwrap();
        encoder.finish().unwrap()
    }
}
