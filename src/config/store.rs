//! Optional on-disk persistence of the Deezer ARL.
//!
//! The credential is written to a regular file with owner-only permissions
//! (0600) inside an owner-only directory (0700), created atomically via a
//! same-directory temporary file and rename. Reads refuse symlinks and files
//! whose permissions have been loosened. These checks reduce exposure to other
//! local users; same-user or privileged processes remain outside this boundary.
//! Persistence is disabled on non-Unix platforms and entirely
//! when `MELIMO_NO_STORE` is set to a non-empty value.

use super::Arl;
use std::{
    env, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};

const FILE_NAME: &str = "session";
const DISABLE_ENV: &str = "MELIMO_NO_STORE";

/// True when the user asked Mélimo not to persist credentials.
pub fn disabled() -> bool {
    !cfg!(unix) || env::var_os(DISABLE_ENV).is_some_and(|value| !value.is_empty())
}

#[cfg(target_os = "macos")]
fn data_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Application Support/melimo"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn data_dir() -> Option<PathBuf> {
    env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .map(|base| base.join("melimo"))
}

#[cfg(windows)]
fn data_dir() -> Option<PathBuf> {
    env::var_os("APPDATA").map(|base| PathBuf::from(base).join("melimo"))
}

#[cfg(not(any(unix, windows)))]
fn data_dir() -> Option<PathBuf> {
    None
}

/// Where a saved login lives, or `None` when no home is known.
pub fn session_path() -> Option<PathBuf> {
    data_dir().map(|dir| dir.join(FILE_NAME))
}

/// Load a saved login, if persistence is enabled and one exists.
pub fn load() -> Result<Option<Arl>, String> {
    if disabled() {
        return Ok(None);
    }
    match session_path() {
        Some(path) => load_at(&path),
        None => Ok(None),
    }
}

/// Persist a login. A failed or disabled save is never fatal.
pub fn save(arl: &Arl) -> Result<(), String> {
    if disabled() {
        return Ok(());
    }
    let path = session_path().ok_or("Cannot find a home directory for the saved login.")?;
    save_at(&path, arl)
}

/// Remove any saved login. Always attempts removal, even when storing is disabled.
pub fn clear() -> Result<(), String> {
    match session_path() {
        Some(path) => clear_at(&path),
        None => Ok(()),
    }
}

fn load_at(path: &Path) -> Result<Option<Arl>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Cannot read the saved login.".into()),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("The saved login must be a regular file. Run `melimo --forget`.".into());
    }
    validate_private_dir(path.parent().ok_or("Invalid saved-login path.")?)?;
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let file = options
        .open(path)
        .map_err(|_| "Cannot open the saved login safely.")?;
    let metadata = file
        .metadata()
        .map_err(|_| "Cannot inspect the saved login.")?;
    if !metadata.is_file() || metadata.len() > 193 {
        return Err("The saved login is not a valid credential file.".into());
    }
    #[cfg(unix)]
    if metadata.permissions().mode() & 0o077 != 0
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.nlink() != 1
    {
        return Err(
            "The saved login must be private and owned by you. Run `melimo --forget`.".into(),
        );
    }
    let mut bytes = Vec::new();
    file.take(194)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read the saved login.")?;
    if bytes.len() > 193 {
        return Err("The saved login is too large.".into());
    }
    let value = std::str::from_utf8(&bytes)
        .map_err(|_| "The saved login is corrupted. Run `melimo --forget`.".to_string())?
        .trim();
    if value.is_empty() {
        return Ok(None);
    }
    Arl::parse(value.to_owned())
        .map(Some)
        .map_err(|_| "The saved login is corrupted. Run `melimo --forget`.".to_string())
}

fn save_at(path: &Path, arl: &Arl) -> Result<(), String> {
    let dir = path.parent().ok_or("Invalid saved-login path.")?;
    ensure_private_dir(dir)?;
    // Never replace anything that is not a regular file: a symlink here could
    // redirect the credential elsewhere.
    if let Ok(metadata) = fs::symlink_metadata(path)
        && !metadata.file_type().is_file()
    {
        return Err("The saved login path exists and is not a regular file.".into());
    }
    let tmp = dir.join(format!(".{FILE_NAME}.{:016x}", rand::random::<u64>()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options
        .open(&tmp)
        .map_err(|_| "Cannot write the saved login.".to_string())?;
    if file
        .write_all(arl.expose().as_bytes())
        .and_then(|()| file.sync_all())
        .is_err()
    {
        drop(file);
        let _ = fs::remove_file(&tmp);
        return Err("Cannot write the saved login.".into());
    }
    drop(file);
    fs::rename(&tmp, path).map_err(|_| {
        let _ = fs::remove_file(&tmp);
        "Cannot save the login.".to_string()
    })
}

fn validate_private_dir(dir: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(dir).map_err(|_| "Cannot inspect the login directory.")?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("The login directory must not be a symlink.".into());
    }
    #[cfg(unix)]
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.permissions().mode() & 0o077 != 0 {
        return Err("The login directory must be private and owned by you.".into());
    }
    Ok(())
}

fn ensure_private_dir(dir: &Path) -> Result<(), String> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    builder.mode(0o700);
    builder
        .create(dir)
        .map_err(|_| "Cannot create the login directory.")?;
    // Refuse symlinks and unsafe existing directories instead of chmodding targets.
    validate_private_dir(dir)
}

fn clear_at(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("Cannot remove the saved login.".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_path(tag: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        env::temp_dir()
            .join(format!(
                "melimo-store-test-{}-{tag}-{n}",
                std::process::id()
            ))
            .join(FILE_NAME)
    }

    fn valid() -> Arl {
        Arl::parse("a".repeat(192)).unwrap()
    }

    #[test]
    fn round_trips_then_removes() {
        let path = temp_path("round");
        let dir = path.parent().unwrap();
        let _ = fs::remove_dir_all(dir);
        save_at(&path, &valid()).unwrap();
        assert_eq!(load_at(&path).unwrap().unwrap().expose(), "a".repeat(192));
        clear_at(&path).unwrap();
        assert!(load_at(&path).unwrap().is_none());
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_parent_symlink_large_files_and_hard_links() {
        use std::os::unix::fs::symlink;
        let path = temp_path("unsafe");
        let dir = path.parent().unwrap();
        let other = temp_path("target");
        save_at(&other, &valid()).unwrap();
        symlink(other.parent().unwrap(), dir).unwrap();
        assert!(save_at(&path, &valid()).is_err());
        assert!(load_at(&path).is_err());
        fs::remove_file(dir).unwrap();
        save_at(&path, &valid()).unwrap();
        fs::write(&path, "a".repeat(4096)).unwrap();
        assert!(load_at(&path).is_err());
        fs::remove_file(&path).unwrap();
        fs::hard_link(&other, &path).unwrap();
        assert!(load_at(&path).is_err());
        fs::remove_dir_all(dir).unwrap();
        fs::remove_dir_all(other.parent().unwrap()).unwrap();
    }

    #[test]
    fn missing_file_loads_as_none() {
        let path = temp_path("missing");
        let _ = fs::remove_dir_all(path.parent().unwrap());
        assert!(load_at(&path).unwrap().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn file_and_directory_are_owner_only() {
        let path = temp_path("perms");
        let dir = path.parent().unwrap();
        let _ = fs::remove_dir_all(dir);
        save_at(&path, &valid()).unwrap();
        let file_mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        let dir_mode = fs::metadata(dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(file_mode, 0o600);
        assert_eq!(dir_mode, 0o700);
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn refuses_loosened_file_permissions() {
        let path = temp_path("loose");
        let dir = path.parent().unwrap();
        let _ = fs::remove_dir_all(dir);
        save_at(&path, &valid()).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(load_at(&path).is_err());
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlink_for_read_and_write() {
        use std::os::unix::fs::symlink;
        let path = temp_path("symlink");
        let dir = path.parent().unwrap();
        let _ = fs::remove_dir_all(dir);
        fs::create_dir_all(dir).unwrap();
        symlink("/etc/hosts", &path).unwrap();
        assert!(save_at(&path, &valid()).is_err());
        assert!(load_at(&path).is_err());
        let _ = fs::remove_dir_all(dir);
    }
}
