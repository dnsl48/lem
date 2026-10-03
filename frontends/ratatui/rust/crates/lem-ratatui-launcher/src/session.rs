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

/// Where logs live, reused across sessions (unlike the directory above,
/// which is private to one): `$XDG_STATE_HOME/lem-ratatui/` when it is
/// set, otherwise `$HOME/.local/state/lem-ratatui/`. `None` when neither
/// is set.
pub fn state_dir(xdg_state_home: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    match xdg_state_home.filter(|dir| !dir.is_empty()) {
        Some(dir) => Some(PathBuf::from(dir).join("lem-ratatui")),
        None => home.map(|home| {
            PathBuf::from(home)
                .join(".local")
                .join("state")
                .join("lem-ratatui")
        }),
    }
}

/// Create `dir` and its parents, owner-only, unless it exists already:
/// unlike `create_private`, an existing directory (and its mode) is left
/// alone, since this one is kept and reused across sessions. Reusing one
/// is refused, though, unless it is actually a directory (not a symlink
/// standing in for one, which `DirBuilder::create` would follow) with no
/// group or other permissions: `recursive` creation leaves an existing
/// directory's mode as it finds it, so a shared `$XDG_STATE_HOME` could
/// otherwise steer where a log file lands. Not checked: who owns it — a
/// directory owned by someone else already fails the `File::create`
/// that follows, which is reported the same way as this refusal is.
pub fn ensure_state_dir(dir: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir)?;

    let metadata = fs::symlink_metadata(dir)?;
    if !metadata.is_dir() {
        return Err(io::Error::other(
            "not a directory (a symlink standing in for one?)",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mode = metadata.permissions().mode();
        if mode & 0o077 != 0 {
            return Err(io::Error::other(format!(
                "accessible to its group or others (mode {:03o})",
                mode & 0o777
            )));
        }
    }
    Ok(())
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

    #[test]
    fn the_state_dir_prefers_xdg() {
        assert_eq!(
            state_dir(Some("/home/u/.state".into()), Some("/home/u".into())),
            Some(PathBuf::from("/home/u/.state/lem-ratatui"))
        );
    }

    #[test]
    fn without_xdg_the_state_dir_is_under_home() {
        let expected = Some(PathBuf::from("/home/u/.local/state/lem-ratatui"));
        assert_eq!(state_dir(None, Some("/home/u".into())), expected);
        assert_eq!(state_dir(Some("".into()), Some("/home/u".into())), expected);
    }

    #[test]
    fn without_xdg_or_home_there_is_no_state_dir() {
        assert_eq!(state_dir(None, None), None);
    }

    #[cfg(unix)]
    #[test]
    fn the_state_dir_is_owner_only_and_reused() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("lem-ratatui");
        ensure_state_dir(&dir).unwrap();
        let mode = fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
        ensure_state_dir(&dir).unwrap(); // reused, not an error
    }

    #[cfg(unix)]
    #[test]
    fn a_pre_existing_directory_open_to_others_is_refused() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("lem-ratatui");
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(ensure_state_dir(&dir).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_standing_in_for_the_directory_is_refused() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let real = root.path().join("real");
        fs::create_dir(&real).unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o700)).unwrap();
        let link = root.path().join("lem-ratatui");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(ensure_state_dir(&link).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_pre_existing_owner_only_directory_is_accepted() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("lem-ratatui");
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(ensure_state_dir(&dir).is_ok());
    }
}
