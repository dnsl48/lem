//! What identifies this session to its processes, and where it keeps its
//! sockets (ADR 0019).

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// A new session id: a UUIDv7 in its canonical text form (ADR 0013).
pub fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// The directory for this session's sockets: under `XDG_RUNTIME_DIR` when
/// it is set (private to the user, by the XDG spec), otherwise in the
/// system's temporary directory.
pub fn runtime_dir(xdg_runtime_dir: Option<OsString>, temp_dir: &Path, id: &str) -> PathBuf {
    match xdg_runtime_dir.filter(|dir| !dir.is_empty()) {
        Some(dir) => PathBuf::from(dir).join("lem-ratatui").join(id),
        None => temp_dir.join(format!("lem-ratatui-{id}")),
    }
}

/// Create `dir`, readable by its owner only, and its parent if missing.
/// Fails if `dir` exists: a session never shares one.
pub fn create_private(dir: &Path) -> io::Result<()> {
    if let Some(parent) = dir.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_uuid_v7_text() {
        let id = new_id();
        let parsed = uuid::Uuid::parse_str(&id).unwrap();
        assert_eq!(parsed.get_version_num(), 7);
        assert_eq!(parsed.to_string(), id, "canonical form");
        assert_ne!(new_id(), id, "one per session");
    }

    #[test]
    fn the_runtime_dir_prefers_xdg() {
        assert_eq!(
            runtime_dir(Some("/run/user/1000".into()), Path::new("/tmp"), "abc"),
            PathBuf::from("/run/user/1000/lem-ratatui/abc")
        );
    }

    #[test]
    fn without_xdg_it_is_in_the_temporary_directory() {
        let expected = PathBuf::from("/tmp/lem-ratatui-abc");
        assert_eq!(runtime_dir(None, Path::new("/tmp"), "abc"), expected);
        assert_eq!(
            runtime_dir(Some("".into()), Path::new("/tmp"), "abc"),
            expected
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_private_directory_is_owner_only_and_new() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("lem-ratatui").join("abc");
        create_private(&dir).unwrap();
        let mode = fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
        assert!(create_private(&dir).is_err(), "never reused");
    }
}
