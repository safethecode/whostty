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
    #[cfg(windows)]
    if previous.is_some() {
        prepare_windows(&target, file.path())?;
    }
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
        replace_windows(file, &target)?;
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
fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

#[cfg(windows)]
struct Security {
    descriptor: windows_sys::Win32::Security::PSECURITY_DESCRIPTOR,
    owner: windows_sys::Win32::Security::PSID,
    group: windows_sys::Win32::Security::PSID,
    dacl: *mut windows_sys::Win32::Security::ACL,
}

#[cfg(windows)]
impl Drop for Security {
    fn drop(&mut self) {
        // SAFETY: GetNamedSecurityInfo allocated this buffer with LocalAlloc.
        unsafe {
            windows_sys::Win32::Foundation::LocalFree(self.descriptor);
        }
    }
}

#[cfg(windows)]
impl Security {
    const INFO: u32 = windows_sys::Win32::Security::OWNER_SECURITY_INFORMATION
        | windows_sys::Win32::Security::GROUP_SECURITY_INFORMATION
        | windows_sys::Win32::Security::DACL_SECURITY_INFORMATION;

    fn read(path: &[u16]) -> Result<Self> {
        use windows_sys::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
        let mut security = Self {
            descriptor: std::ptr::null_mut(),
            owner: std::ptr::null_mut(),
            group: std::ptr::null_mut(),
            dacl: std::ptr::null_mut(),
        };
        // SAFETY: Path is terminated, outputs are valid pointers; descriptor owns the SID/ACL data.
        let error = unsafe {
            GetNamedSecurityInfoW(
                path.as_ptr(),
                SE_FILE_OBJECT,
                Self::INFO,
                &mut security.owner,
                &mut security.group,
                &mut security.dacl,
                std::ptr::null_mut(),
                &mut security.descriptor,
            )
        };
        if error != 0 {
            return Err(std::io::Error::from_raw_os_error(error as i32))
                .context("Read configuration permissions");
        }
        Ok(security)
    }

    fn apply(&self, path: &[u16]) -> Result<()> {
        use windows_sys::Win32::Security::{
            Authorization::{SetNamedSecurityInfoW, SE_FILE_OBJECT},
            GetSecurityDescriptorControl, PROTECTED_DACL_SECURITY_INFORMATION, SE_DACL_PROTECTED,
            UNPROTECTED_DACL_SECURITY_INFORMATION,
        };
        let mut control = 0;
        let mut revision = 0;
        // SAFETY: Descriptor and all SID/ACL pointers remain owned by self through these calls.
        if unsafe { GetSecurityDescriptorControl(self.descriptor, &mut control, &mut revision) }
            == 0
        {
            return Err(std::io::Error::last_os_error())
                .context("Read configuration ACL inheritance");
        }
        let inheritance = if control & SE_DACL_PROTECTED != 0 {
            PROTECTED_DACL_SECURITY_INFORMATION
        } else {
            UNPROTECTED_DACL_SECURITY_INFORMATION
        };
        let error = unsafe {
            SetNamedSecurityInfoW(
                path.as_ptr(),
                SE_FILE_OBJECT,
                Self::INFO | inheritance,
                self.owner,
                self.group,
                self.dacl,
                std::ptr::null(),
            )
        };
        if error != 0 {
            return Err(std::io::Error::from_raw_os_error(error as i32))
                .context("Preserve configuration permissions");
        }
        Ok(())
    }
}

#[cfg(windows)]
fn prepare_windows(target: &Path, replacement: &Path) -> Result<()> {
    use windows_sys::Win32::Storage::FileSystem::*;
    let target = wide(target);
    let replacement = wide(replacement);
    // SAFETY: Both paths are terminated and valid for these calls.
    let attributes = unsafe { GetFileAttributesW(target.as_ptr()) };
    let temporary_attributes = unsafe { GetFileAttributesW(replacement.as_ptr()) };
    anyhow::ensure!(
        attributes != INVALID_FILE_ATTRIBUTES && temporary_attributes != INVALID_FILE_ATTRIBUTES,
        "Read configuration attributes: {}",
        std::io::Error::last_os_error()
    );
    let special = FILE_ATTRIBUTE_ENCRYPTED | FILE_ATTRIBUTE_COMPRESSED | FILE_ATTRIBUTE_SPARSE_FILE;
    anyhow::ensure!(attributes & special == temporary_attributes & special, "Configuration uses encryption/compression attributes that cannot be preserved by this save");
    // Preserve owner/group/DACL before writing any configuration content into the temporary file.
    Security::read(&target)?.apply(&replacement)?;
    let mask = FILE_ATTRIBUTE_HIDDEN
        | FILE_ATTRIBUTE_SYSTEM
        | FILE_ATTRIBUTE_ARCHIVE
        | FILE_ATTRIBUTE_NOT_CONTENT_INDEXED
        | FILE_ATTRIBUTE_OFFLINE;
    let attributes = if attributes & mask == 0 {
        FILE_ATTRIBUTE_NORMAL
    } else {
        attributes & mask
    };
    if unsafe { SetFileAttributesW(replacement.as_ptr(), attributes) } == 0 {
        return Err(std::io::Error::last_os_error()).context("Preserve configuration attributes");
    }
    Ok(())
}

#[cfg(windows)]
fn replace_windows(file: tempfile::NamedTempFile, target: &Path) -> Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    let (handle, replacement) = file.into_parts();
    drop(handle);
    let source = wide(&replacement);
    let target = wide(target);
    // Avoid ReplaceFile's backup-rename interval where concurrent readers observe NotFound.
    // Both paths are on the same volume; no copy/delete fallback or pre-deletion is allowed.
    // SAFETY: Owned temporary path and destination are terminated and live through the call.
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error()).context("Commit configuration");
    }
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::config::Settings;

    fn sddl(path: &Path) -> String {
        use windows_sys::Win32::{
            Foundation::LocalFree,
            Security::Authorization::{
                ConvertSecurityDescriptorToStringSecurityDescriptorW, SDDL_REVISION_1,
            },
        };
        let security = Security::read(&wide(path)).unwrap();
        let mut text = std::ptr::null_mut();
        assert_ne!(
            unsafe {
                ConvertSecurityDescriptorToStringSecurityDescriptorW(
                    security.descriptor,
                    SDDL_REVISION_1,
                    Security::INFO,
                    &mut text,
                    std::ptr::null_mut(),
                )
            },
            0
        );
        let mut length = 0;
        unsafe {
            while *text.add(length) != 0 {
                length += 1;
            }
        }
        let result =
            String::from_utf16(unsafe { std::slice::from_raw_parts(text, length) }).unwrap();
        unsafe {
            LocalFree(text.cast());
        }
        result
    }

    #[test]
    fn windows_saves_preserve_owner_group_and_dacl() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut settings = Settings::default();
        settings.save(&path).unwrap();
        let original = sddl(&path);
        settings.font_size = 17.0;
        settings.save(&path).unwrap();
        assert_eq!(sddl(&path), original);
    }
}
