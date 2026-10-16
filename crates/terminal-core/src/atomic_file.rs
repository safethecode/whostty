use anyhow::{Context, Result};
use std::{io::Write, path::Path};

pub(crate) fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    // Keep user-managed symlinks instead of replacing the link itself.
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error).context("Inspect configuration path"),
    };
    let target = if metadata.is_some() {
        path.canonicalize().context("Resolve configuration path")?
    } else {
        path.to_path_buf()
    };
    let previous = if metadata.is_some() {
        Some(std::fs::metadata(&target)?)
    } else {
        None
    };
    if let Some(previous) = &previous {
        anyhow::ensure!(previous.is_file(), "Configuration path is not a file");
        anyhow::ensure!(
            !previous.permissions().readonly(),
            "Configuration is read-only"
        );
    }
    let parent = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::Builder::new()
        .prefix(".whostty-config-")
        .tempfile_in(parent)?;
    file.write_all(bytes)
        .context("Write temporary configuration")?;
    #[cfg(unix)]
    if let Some(previous) = &previous {
        file.as_file().set_permissions(previous.permissions())?;
    }
    file.as_file()
        .sync_all()
        .context("Flush temporary configuration")?;
    #[cfg(windows)]
    if previous.is_some() {
        replace_windows(file, &target, parent)?;
    } else {
        file.persist(&target).context("Commit configuration")?;
    }
    #[cfg(not(windows))]
    file.persist(&target).context("Commit configuration")?;
    #[cfg(unix)]
    if let Ok(directory) = std::fs::File::open(parent) {
        let _ = directory.sync_all();
    }
    Ok(())
}

#[cfg(windows)]
fn replace_windows(file: tempfile::NamedTempFile, target: &Path, parent: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        ReplaceFileW, SetFileAttributesW, FILE_ATTRIBUTE_NORMAL,
    };
    // ReplaceFile preserves the original DACL and attributes. Keep a uniquely named backup
    // for its documented partial-failure cases; never discard the only remaining original.
    let mut backup = tempfile::Builder::new()
        .prefix(".whostty-config-backup-")
        .tempfile_in(parent)?
        .into_temp_path();
    std::fs::remove_file(&backup)?;
    let (handle, replacement) = file.into_parts();
    drop(handle); // ReplaceFile opens the replacement exclusively.
    let target_wide: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    let replacement_wide: Vec<u16> = replacement
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let backup_wide: Vec<u16> = backup.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: All paths are terminated, live for the calls, and refer to owned temp files.
    if unsafe { SetFileAttributesW(replacement_wide.as_ptr(), FILE_ATTRIBUTE_NORMAL) } == 0 {
        return Err(std::io::Error::last_os_error()).context("Prepare configuration replacement");
    }
    if unsafe {
        ReplaceFileW(
            target_wide.as_ptr(),
            replacement_wide.as_ptr(),
            backup_wide.as_ptr(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } != 0
    {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    if backup.exists() {
        if !target.exists() && std::fs::rename(&backup, target).is_ok() {
            return Err(error).context("Replace configuration failed; the original was restored");
        }
        let saved = backup.to_path_buf();
        // Do not risk deleting recovery data even when attributes cannot be changed.
        backup.disable_cleanup(true);
        anyhow::bail!(
            "Replace configuration: {error}. Previous configuration retained at {}",
            saved.display()
        );
    }
    Err(error).context("Replace configuration")
}
