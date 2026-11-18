//! Windows profile lock and same-directory, non-replacing activation commit.
use crate::{Error, Result};
use std::{
    fs::{File, OpenOptions},
    path::Path,
};
pub(crate) const LOCK: &str = "profile.lock";
pub(crate) const STAGED: &str = "activation.next";

#[cfg(windows)]
pub(crate) fn replace(directory: &Path, source: &str, target: &str) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" { fn MoveFileExW(source: *const u16, target: *const u16, flags: u32) -> i32; }
    let from: Vec<u16> = directory.join(source).as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = directory.join(target).as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 1 | 8) } == 0 { return Err(Error::Storage); }
    Ok(())
}
#[cfg(not(windows))]
pub(crate) fn replace(_: &Path, _: &str, _: &str) -> Result<()> { Err(Error::Storage) }

#[cfg(windows)]
pub(crate) fn acquire(directory: &Path) -> Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(directory.join(LOCK))
        .map_err(|e| {
            if e.raw_os_error() == Some(32) {
                Error::Busy
            } else {
                Error::Storage
            }
        })
}

#[cfg(windows)]
pub(crate) fn commit(directory: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(source: *const u16, destination: *const u16, flags: u32) -> i32;
    }
    fn wide(path: &Path) -> Result<Vec<u16>> {
        let mut s: Vec<_> = path.as_os_str().encode_wide().collect();
        if s.contains(&0) {
            return Err(Error::Storage);
        }
        s.push(0);
        Ok(s)
    }
    let source = wide(&directory.join(STAGED))?;
    let target = wide(&directory.join(crate::ACTIVE))?;
    // Same directory/volume, no COPY_ALLOWED, no REPLACE_EXISTING.
    // Safety: both buffers remain valid, NUL-terminated for the duration.
    if unsafe { MoveFileExW(source.as_ptr(), target.as_ptr(), 8) } == 0 {
        let error = std::io::Error::last_os_error();
        return Err(if matches!(error.raw_os_error(), Some(80 | 183)) {
            Error::AlreadyExists
        } else {
            Error::Storage
        });
    }
    Ok(())
}

#[cfg(not(windows))]
pub(crate) fn acquire(_: &Path) -> Result<File> {
    Err(Error::Storage)
}
#[cfg(not(windows))]
pub(crate) fn commit(_: &Path) -> Result<()> {
    Err(Error::Storage)
}
