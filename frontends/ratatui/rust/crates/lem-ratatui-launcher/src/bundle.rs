//! The Lisp image and display binary embedded in this one (`bundle`
//! feature).
//!
//! Neither can be run straight from memory: an SBCL executable finds its
//! core by opening `/proc/self/exe`, which a memfd has no usable path for
//! ("Can't find sbcl.core"). So each is unpacked once to the user's cache,
//! keyed by a hash of both, and spawned from there like an unbundled one.

use std::env::consts::EXE_SUFFIX;
use std::fs::{self, File};
use std::io::{self, BufWriter};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

static LISP: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/lem-ratatui-lisp.zst"));
static TERMINAL: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/lem-ratatui.zst"));
const HASH: &str = env!("LEM_RATATUI_BUNDLE_HASH");

/// Path of the unpacked Lisp image.
pub fn lisp() -> Result<PathBuf> {
    install(&format!("lem-ratatui-lisp{EXE_SUFFIX}"), LISP)
}

/// Path of the unpacked display binary.
pub fn terminal() -> Result<PathBuf> {
    install(&format!("lem-ratatui{EXE_SUFFIX}"), TERMINAL)
}

/// Path of `name` in the cache, unpacking `packed` there first if this
/// build's copy isn't in it yet.
fn install(name: &str, packed: &[u8]) -> Result<PathBuf> {
    let root = cache_root()?.join("lem-ratatui");
    let dir = root.join(HASH);
    let path = dir.join(name);
    if path.is_file() {
        return Ok(path);
    }

    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    // Unpacked beside the target and renamed into place, so a concurrent
    // launch or an interrupted one never sees a partial file.
    let partial = dir.join(format!(".{name}.{}", std::process::id()));
    unpack(packed, &partial).with_context(|| format!("unpacking {}", partial.display()))?;
    fs::rename(&partial, &path).with_context(|| format!("installing {}", path.display()))?;

    prune(&root);
    Ok(path)
}

fn cache_root() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_CACHE_HOME").filter(|d| !d.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    let home = std::env::var_os("HOME").context("neither XDG_CACHE_HOME nor HOME is set")?;
    Ok(PathBuf::from(home).join(".cache"))
}

fn unpack(packed: &[u8], to: &Path) -> Result<()> {
    // Closed before returning: spawning a file this process still holds
    // open for writing fails with ETXTBSY.
    let mut out = BufWriter::new(File::create(to)?);
    zstd::stream::copy_decode(packed, &mut out)?;
    out.into_inner()
        .map_err(io::IntoInnerError::into_error)?
        .sync_all()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(to, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

/// Drop what other builds left. Best effort: a still-running Lem keeps its
/// deleted image alive, and a failure here costs only disk space.
fn prune(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name() != HASH {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}
