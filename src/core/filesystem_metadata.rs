//! Guards and filesystem metadata for replacing an inode. All fallible work
//! happens on the private replacement before a backup or destination rename.
use std::fs::{self, File};
use std::io;
use std::path::Path;

fn refused(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}

pub(super) fn open_destination(path: &Path) -> io::Result<File> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(refused("metadata writes refuse symbolic links"));
    }
    if !metadata.is_file() || metadata.permissions().readonly() {
        return Err(refused("metadata writes require a writable regular file"));
    }
    let mut options = fs::OpenOptions::new();
    options.read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?; // No truncation; checks actual ACL/access rights too.
    check_identity(path, &file)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if file.metadata()?.mode() & 0o6000 != 0 {
            return Err(refused("metadata writes refuse setuid/setgid files"));
        }
    }
    Ok(file)
}

pub(super) fn check_identity(path: &Path, original: &File) -> io::Result<()> {
    let current = fs::symlink_metadata(path)?;
    if current.file_type().is_symlink() || current.permissions().readonly() || !current.is_file() {
        return Err(refused(
            "destination became protected during metadata write",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let opened = original.metadata()?;
        if (
            current.dev(),
            current.ino(),
            current.mode(),
            current.uid(),
            current.gid(),
        ) != (
            opened.dev(),
            opened.ino(),
            opened.mode(),
            opened.uid(),
            opened.gid(),
        ) {
            return Err(refused("destination inode changed during metadata write"));
        }
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod platform {
    use super::*;
    use std::collections::BTreeMap;
    use std::ffi::CString;
    #[cfg(target_os = "macos")]
    use std::os::darwin::fs::MetadataExt as DarwinMetadataExt;
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::MetadataExt;

    pub(crate) struct Snapshot {
        mode: u32,
        uid: u32,
        gid: u32,
        attrs: BTreeMap<CString, Vec<u8>>,
        #[cfg(target_os = "macos")]
        acl: Vec<u8>,
        #[cfg(target_os = "macos")]
        flags: u32,
    }

    fn checked(result: libc::c_int) -> io::Result<()> {
        if result == -1 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    // Read installed libc declarations: Linux has no position/options arguments;
    // Darwin exposes both. A size change/error fails closed, never skips an attr.
    fn list(file: &File, buffer: &mut [u8]) -> io::Result<usize> {
        // SAFETY: live fd and writable buffer (zero length is the size query).
        let n = unsafe {
            #[cfg(target_os = "linux")]
            {
                libc::flistxattr(file.as_raw_fd(), buffer.as_mut_ptr().cast(), buffer.len())
            }
            #[cfg(target_os = "macos")]
            {
                libc::flistxattr(
                    file.as_raw_fd(),
                    buffer.as_mut_ptr().cast(),
                    buffer.len(),
                    0,
                )
            }
        };
        if n < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(n as usize)
        }
    }
    fn get(file: &File, name: &CString, buffer: &mut [u8]) -> io::Result<usize> {
        // SAFETY: live fd, terminated name, writable buffer of specified length.
        let n = unsafe {
            #[cfg(target_os = "linux")]
            {
                libc::fgetxattr(
                    file.as_raw_fd(),
                    name.as_ptr(),
                    buffer.as_mut_ptr().cast(),
                    buffer.len(),
                )
            }
            #[cfg(target_os = "macos")]
            {
                libc::fgetxattr(
                    file.as_raw_fd(),
                    name.as_ptr(),
                    buffer.as_mut_ptr().cast(),
                    buffer.len(),
                    0,
                    0,
                )
            }
        };
        if n < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(n as usize)
        }
    }
    fn attrs(file: &File) -> io::Result<BTreeMap<CString, Vec<u8>>> {
        let mut names = vec![0; list(file, &mut [])?];
        let n = list(file, &mut names)?;
        if n > names.len() {
            return Err(io::Error::other("xattr list changed during snapshot"));
        }
        names.truncate(n);
        let mut result = BTreeMap::new();
        for name in names.split(|b| *b == 0).filter(|s| !s.is_empty()) {
            let name = CString::new(name).map_err(io::Error::other)?;
            let mut value = vec![0; get(file, &name, &mut [])?];
            let n = get(file, &name, &mut value)?;
            if n > value.len() {
                return Err(io::Error::other("xattr value changed during snapshot"));
            }
            value.truncate(n);
            result.insert(name, value);
        }
        Ok(result)
    }
    fn set(file: &File, name: &CString, value: &[u8]) -> io::Result<()> {
        // SAFETY: live fd, terminated name, initialized value and exact length.
        checked(unsafe {
            #[cfg(target_os = "linux")]
            {
                libc::fsetxattr(
                    file.as_raw_fd(),
                    name.as_ptr(),
                    value.as_ptr().cast(),
                    value.len(),
                    0,
                )
            }
            #[cfg(target_os = "macos")]
            {
                libc::fsetxattr(
                    file.as_raw_fd(),
                    name.as_ptr(),
                    value.as_ptr().cast(),
                    value.len(),
                    0,
                    0,
                )
            }
        })
    }
    fn remove(file: &File, name: &CString) -> io::Result<()> {
        // SAFETY: live fd and terminated name.
        checked(unsafe {
            #[cfg(target_os = "linux")]
            {
                libc::fremovexattr(file.as_raw_fd(), name.as_ptr())
            }
            #[cfg(target_os = "macos")]
            {
                libc::fremovexattr(file.as_raw_fd(), name.as_ptr(), 0)
            }
        })
    }

    #[cfg(target_os = "macos")]
    fn acl(file: &File) -> io::Result<Vec<u8>> {
        // Signatures/type from the installed SDK's sys/acl.h. libc does not
        // expose Darwin ACL functions. The returned objects require acl_free.
        unsafe extern "C" {
            fn acl_get_fd_np(fd: libc::c_int, kind: libc::c_int) -> *mut libc::c_void;
            fn acl_to_text(acl: *mut libc::c_void, len: *mut libc::ssize_t) -> *mut libc::c_char;
            fn acl_free(object: *mut libc::c_void) -> libc::c_int;
        }
        // SAFETY: live fd, SDK ACL_TYPE_EXTENDED; both allocated objects are
        // checked before access, read at the returned length, and freed once.
        unsafe {
            let object = acl_get_fd_np(file.as_raw_fd(), 0x100);
            if object.is_null() {
                let error = io::Error::last_os_error();
                // Darwin returns ENOENT for a live fd without an extended ACL.
                return if error.raw_os_error() == Some(libc::ENOENT) {
                    Ok(Vec::new())
                } else {
                    Err(error)
                };
            }
            let mut len = 0;
            let text = acl_to_text(object, &mut len);
            let result = if text.is_null() {
                Err(io::Error::last_os_error())
            } else {
                let bytes = std::slice::from_raw_parts(text.cast::<u8>(), len as usize).to_vec();
                acl_free(text.cast());
                Ok(bytes)
            };
            acl_free(object);
            result
        }
    }

    impl Snapshot {
        pub(crate) fn read(file: &File) -> io::Result<Self> {
            let m = file.metadata()?;
            #[cfg(target_os = "macos")]
            if m.st_flags()
                & (libc::UF_IMMUTABLE | libc::UF_APPEND | libc::SF_IMMUTABLE | libc::SF_APPEND)
                != 0
            {
                return Err(refused(
                    "metadata writes refuse immutable/append-only files",
                ));
            }
            let attrs = attrs(file)?;
            // Restoring capabilities to changed bytes can confer privileges.
            if attrs.contains_key(c"security.capability") {
                return Err(refused("metadata writes refuse files with capabilities"));
            }
            Ok(Self {
                mode: m.mode(),
                uid: m.uid(),
                gid: m.gid(),
                attrs,
                #[cfg(target_os = "macos")]
                acl: acl(file)?,
                #[cfg(target_os = "macos")]
                flags: m.st_flags(),
            })
        }
        pub(crate) fn restore(&self, original: &File, replacement: &File) -> io::Result<()> {
            // Recheck the held source inode's security metadata before copying.
            let current = Self::read(original)?;
            if !self.matches(&current) {
                return Err(refused("source metadata changed during write"));
            }
            let m = replacement.metadata()?;
            if (m.uid(), m.gid()) != (self.uid, self.gid) {
                // SAFETY: live private fd and source uid/gid; never affects source.
                checked(unsafe { libc::fchown(replacement.as_raw_fd(), self.uid, self.gid) })?;
            }
            // chown can clear mode bits: restore mode only afterwards.
            replacement.set_permissions(std::os::unix::fs::PermissionsExt::from_mode(self.mode))?;
            #[cfg(target_os = "macos")]
            {
                // SAFETY: two live descriptors; ACL-only copy, no file data.
                // SDK copyfile(3): COPYFILE_ACL copies the source's extended ACL.
                checked(unsafe {
                    libc::fcopyfile(
                        original.as_raw_fd(),
                        replacement.as_raw_fd(),
                        std::ptr::null_mut(),
                        libc::COPYFILE_ACL,
                    )
                })?;
            }
            // Linux POSIX ACLs and security labels are xattrs; apply after mode.
            // Remove inherited/created attrs absent on source (including forks).
            let existing = attrs(replacement)?;
            for name in existing
                .keys()
                .filter(|name| !self.attrs.contains_key(*name))
            {
                remove(replacement, name)?;
            }
            for (name, value) in &self.attrs {
                if existing.get(name) != Some(value) {
                    set(replacement, name, value)?;
                }
            }
            #[cfg(target_os = "macos")]
            if replacement.metadata()?.st_flags() != self.flags {
                // SAFETY: live private fd; immutable/append flags were refused.
                checked(unsafe { libc::fchflags(replacement.as_raw_fd(), self.flags) })?;
            }
            if !self.matches(&Self::read(replacement)?) {
                return Err(refused("cannot preserve destination filesystem metadata"));
            }
            Ok(())
        }
        fn matches(&self, other: &Self) -> bool {
            let same = (self.mode, self.uid, self.gid, &self.attrs)
                == (other.mode, other.uid, other.gid, &other.attrs);
            #[cfg(target_os = "macos")]
            {
                same && self.acl == other.acl && self.flags == other.flags
            }
            #[cfg(target_os = "linux")]
            {
                same
            }
        }
    }
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) use platform::Snapshot;

// Windows retains the existing atomic writer's permissions-only behavior.
// Full security-descriptor/alternate-stream preservation is inherited debt;
// do not turn this Unix preservation repair into a blanket Windows refusal.
#[cfg(windows)]
pub(super) struct Snapshot {
    permissions: fs::Permissions,
}
#[cfg(windows)]
impl Snapshot {
    pub(super) fn read(file: &File) -> io::Result<Self> {
        Ok(Self {
            permissions: file.metadata()?.permissions(),
        })
    }
    pub(super) fn restore(&self, original: &File, replacement: &File) -> io::Result<()> {
        if original.metadata()?.permissions() != self.permissions {
            return Err(refused("source permissions changed during write"));
        }
        replacement.set_permissions(self.permissions.clone())?;
        if replacement.metadata()?.permissions() != self.permissions {
            return Err(refused("cannot preserve destination permissions"));
        }
        Ok(())
    }
}

// Do not claim to preserve metadata on a platform without an implementation.
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub(super) struct Snapshot;
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
impl Snapshot {
    pub(super) fn read(_: &File) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "filesystem metadata preservation is not implemented on this platform",
        ))
    }
    pub(super) fn restore(&self, _: &File, _: &File) -> io::Result<()> {
        unreachable!()
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    #[test]
    fn changed_source_security_refuses_before_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.jpg");
        let backup = dir.path().join("source.jpg.bak");
        fs::write(&path, b"original").unwrap();
        fs::write(&backup, b"existing backup").unwrap();
        let inode = fs::metadata(&path).unwrap().ino();
        let result = crate::core::write_transaction::transact_with(
            &path,
            |scratch| -> io::Result<()> {
                fs::write(scratch, b"rewritten")?;
                // Simulate an external protection change after the snapshot.
                fs::set_permissions(&path, fs::Permissions::from_mode(0o444))
            },
            || -> io::Result<()> { panic!("backup callback must not run when preservation fails") },
            |_, error| io::Error::other(error),
        );
        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), b"original");
        assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o444);
        assert_eq!(fs::read(&backup).unwrap(), b"existing backup");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    use crate::core::write_transaction::{WriteOutcome, transact_with};

    fn write(path: &Path) -> io::Result<WriteOutcome> {
        transact_with(
            path,
            |scratch| {
                // Exercise the reopening seam: format writers can replace the
                // scratch inode rather than modifying the initially held fd.
                fs::remove_file(scratch)?;
                fs::write(scratch, b"rewritten")
            },
            || -> io::Result<()> { Ok(()) },
            |_, error| io::Error::other(error),
        )
    }

    #[test]
    fn ordinary_windows_write_remains_supported() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.jpg");
        fs::write(&path, b"original").unwrap();
        let permissions = fs::metadata(&path).unwrap().permissions();
        assert_eq!(write(&path).unwrap(), WriteOutcome::Updated);
        assert_eq!(fs::read(&path).unwrap(), b"rewritten");
        assert_eq!(fs::metadata(&path).unwrap().permissions(), permissions);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn readonly_windows_write_refuses_before_scratch_or_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.jpg");
        let backup = dir.path().join("source.jpg.bak");
        fs::write(&path, b"original").unwrap();
        fs::write(&backup, b"existing backup").unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&path, permissions.clone()).unwrap();
        let result = transact_with(
            &path,
            |_| -> io::Result<()> { panic!("readonly destination must refuse before scratch") },
            || -> io::Result<()> { panic!("readonly destination must refuse before backup") },
            |_, error| io::Error::other(error),
        );
        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), b"original");
        assert_eq!(fs::metadata(&path).unwrap().permissions(), permissions);
        assert_eq!(fs::read(&backup).unwrap(), b"existing backup");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
        // Windows cannot clean up readonly files automatically.
        permissions.set_readonly(false);
        fs::set_permissions(&path, permissions).unwrap();
    }
}
