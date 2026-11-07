//! Shared composed filesystems, persistent overlays and their exposure to
//! guests: the [`wire`] protocol for intercepting backends (Water, the
//! Windows file shim), FUSE on Linux and the PRoot tracer on Android.
mod view;
pub mod wire;
pub use view::*;
#[cfg(target_os = "linux")]
mod linux;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};
pub struct MountLease {
    pub path: PathBuf,
    #[cfg(target_os = "linux")]
    _host: Option<fuser::BackgroundSession>,
    #[cfg(target_os = "android")]
    _rootless: rootless::Server,
    _lock: Option<File>,
    _lock_path: Option<PathBuf>,
}
impl Drop for MountLease {
    fn drop(&mut self) {
        #[cfg(target_os = "linux")]
        self._host.take();
        let _ = fs::remove_dir(&self.path);
        self._lock.take();
        if let Some(path) = self._lock_path.take() {
            let _ = fs::remove_file(path);
        }
    }
}
/// Reclaim only empty abandoned mountpoints whose ownership lock is available.
pub fn cleanup_stale_mounts(parent: &Path) -> io::Result<()> {
    if !parent.exists() {
        return Ok(());
    }
    for e in fs::read_dir(parent)? {
        let e = e?;
        if e.path()
            .extension()
            .is_none_or(|e| e != "banana-mount-lock")
            || !e.file_type()?.is_file()
        {
            continue;
        }
        let mut f = OpenOptions::new().read(true).write(true).open(e.path())?;
        if f.try_lock().is_err() {
            continue;
        }
        let mut marker = String::new();
        std::io::Read::by_ref(&mut f)
            .take(64)
            .read_to_string(&mut marker)?;
        if marker != "banana-mount-v1\n" {
            continue;
        }
        let point = e.path().with_extension("");
        #[cfg(target_os = "linux")]
        unmount_stale(&point)?;
        let removed = match fs::remove_dir(&point) {
            Ok(()) => true,
            Err(err) if err.kind() == io::ErrorKind::NotFound => true,
            Err(_) => false,
        };
        drop(f);
        if removed {
            #[cfg(unix)]
            {
                use std::os::unix::fs::FileTypeExt;
                let socket = point.with_extension("sock");
                if fs::symlink_metadata(&socket).is_ok_and(|m| m.file_type().is_socket()) {
                    fs::remove_file(socket)?;
                }
            }
            fs::remove_file(e.path())?;
        }
    }
    Ok(())
}
pub fn mount(view: Arc<View>, point: &Path) -> io::Result<MountLease> {
    let parent = point
        .parent()
        .ok_or_else(|| io::Error::other("Mountpoint has no parent"))?;
    fs::create_dir_all(parent)?;
    cleanup_stale_mounts(parent)?;
    let lock_path = point.with_extension("banana-mount-lock");
    let mut lock = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(&lock_path)?;
    lock.try_lock().map_err(io::Error::other)?;
    lock.write_all(b"banana-mount-v1\n")?;
    lock.sync_all()?;
    #[cfg(target_os = "linux")]
    let result = linux::mount(view, point);
    #[cfg(target_os = "android")]
    let result = (|| {
        fs::create_dir(point)?;
        let socket = point.with_extension("sock");
        let server = rootless::Server::start(view, &socket, &point.to_string_lossy())?;
        Ok::<_, io::Error>(MountLease {
            path: point.to_path_buf(),
            _rootless: server,
            _lock: None,
            _lock_path: None,
        })
    })();
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    let result = {
        let _ = view;
        Err::<MountLease, _>(io::Error::new(
            io::ErrorKind::Unsupported,
            "This host has no OS mount; its backends are served the view directly",
        ))
    };
    match result {
        Ok(mut lease) => {
            lease._lock = Some(lock);
            lease._lock_path = Some(lock_path);
            Ok(lease)
        }
        Err(e) => {
            drop(lock);
            let _ = fs::remove_file(lock_path);
            let _ = fs::remove_dir(point);
            Err(e)
        }
    }
}
#[cfg(test)]
mod tests;

#[cfg(target_os = "linux")]
fn unmount_stale(point: &Path) -> io::Result<()> {
    use std::os::unix::ffi::OsStringExt;
    fn decode(s: &str) -> std::ffi::OsString {
        let bytes = s.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\\'
                && i + 3 < bytes.len()
                && bytes[i + 1..i + 4]
                    .iter()
                    .all(|b| (b'0'..=b'7').contains(b))
            {
                out.push(
                    (bytes[i + 1] - b'0') * 64 + (bytes[i + 2] - b'0') * 8 + bytes[i + 3] - b'0',
                );
                i += 4;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        }
        std::ffi::OsString::from_vec(out)
    }
    let canonical = point
        .parent()
        .ok_or_else(|| io::Error::other("Mountpoint has no parent"))?
        .canonicalize()?
        .join(
            point
                .file_name()
                .ok_or_else(|| io::Error::other("Mountpoint has no name"))?,
        );
    let mounts = fs::read_to_string("/proc/self/mountinfo")?;
    let owned = mounts.lines().any(|line| {
        let Some((left, right)) = line.split_once(" - ") else {
            return false;
        };
        let fields = left.split_whitespace().collect::<Vec<_>>();
        let kind = right.split_whitespace().collect::<Vec<_>>();
        fields.len() > 4
            && kind.len() > 1
            && kind[0].starts_with("fuse")
            && kind[1] == "banana-pepper"
            && PathBuf::from(decode(fields[4])) == canonical
    });
    if owned {
        let status = match std::process::Command::new("fusermount3")
            .args(["-u", "-z"])
            .arg(&canonical)
            .status()
        {
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                std::process::Command::new("fusermount")
                    .args(["-u", "-z"])
                    .arg(&canonical)
                    .status()
            }
            result => result,
        }?;
        if !status.success() {
            return Err(io::Error::other(
                "Could not release an abandoned Banana Pepper mount",
            ));
        }
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "android"))]
pub mod rootless;

impl MountLease {
    pub fn flush(&self) -> io::Result<()> {
        #[cfg(target_os = "android")]
        self._rootless.flush()?;
        Ok(())
    }
    /// Expose view path `source` ("" for the root) at absolute guest path
    /// `guest`. Only the interception adapter (Android) supports mappings;
    /// OS mounts expose the view at [`MountLease::path`] alone.
    pub fn map_namespace(&self, guest: &str, source: &str, readonly: bool) -> io::Result<()> {
        #[cfg(target_os = "android")]
        return self._rootless.map(guest, source, readonly);
        #[cfg(not(target_os = "android"))]
        {
            let _ = (guest, source, readonly);
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "This host's mount does not support guest path mappings",
            ))
        }
    }
    /// The socket a guest-side interception adapter connects to, if any.
    pub fn namespace_endpoint(&self) -> Option<&Path> {
        #[cfg(target_os = "android")]
        return Some(self._rootless.socket.as_path());
        #[cfg(not(target_os = "android"))]
        None
    }
}
