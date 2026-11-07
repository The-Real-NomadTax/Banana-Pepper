//! Guest file access. The session host serves the composed view (source,
//! generated files and overlay) over a private pipe; Water translates guest
//! paths and keeps POSIX file positions on this side.
use banana_wire::{self as wire, Client, Meta};
use std::{
    io::{self, SeekFrom},
    sync::OnceLock,
};

static VIEW: OnceLock<Client> = OnceLock::new();

/// Connect to the view the host passed as `--view READ,WRITE` (inherited
/// pipe handles). Called once, before any guest code runs.
pub fn connect(spec: &str) -> Result<(), String> {
    let (read, write) = spec
        .split_once(',')
        .and_then(|(r, w)| Some((r.parse::<usize>().ok()?, w.parse::<usize>().ok()?)))
        .ok_or("--view expects READ,WRITE handles")?;
    #[cfg(windows)]
    let client = unsafe {
        use std::os::windows::io::{FromRawHandle, RawHandle};
        Client::new(
            std::fs::File::from_raw_handle(read as RawHandle),
            std::fs::File::from_raw_handle(write as RawHandle),
        )
    };
    #[cfg(not(windows))]
    let client: io::Result<Client> = {
        let _ = (read, write);
        Err(io::Error::other("Water view pipes are Windows-only"))
    };
    VIEW.set(client.map_err(|e| format!("Cannot reach the session view: {e}"))?)
        .map_err(|_| "Session view already connected".into())
}

pub fn view() -> io::Result<&'static Client> {
    VIEW.get()
        .ok_or_else(|| io::Error::other("Session view is not connected"))
}

/// The view path for a guest path. Absolute guest paths are rooted at
/// `BANANA_GUEST_ROOT` (or `/`); relative ones at the view root. Paths that
/// leave the namespace are refused.
pub fn resolve(name: &str) -> io::Result<String> {
    if name.contains(['\\', ':', '\0']) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Guest path leaves its namespace",
        ));
    }
    let guest = std::env::var("BANANA_GUEST_ROOT").unwrap_or_else(|_| "/game".into());
    let name = name
        .strip_prefix(&format!("{guest}/"))
        .unwrap_or(name)
        .trim_start_matches('/');
    let mut components = Vec::new();
    for part in name.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if components.pop().is_none() {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "Guest path leaves its namespace",
                    ));
                }
            }
            p => components.push(p),
        }
    }
    let normalized = components.join("/");
    let name = if normalized == "proc/cpuinfo" {
        ".banana/proc/cpuinfo".to_owned()
    } else {
        normalized
    };
    banana_content::path(&name)?;
    Ok(name)
}

/// POSIX-style open options.
#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    pub read: bool,
    pub write: bool,
    pub create: bool,
    pub truncate: bool,
    pub exclusive: bool,
    pub append: bool,
}
impl Options {
    /// From Linux `open` flags (O_ACCMODE, CREAT, EXCL, TRUNC, APPEND).
    pub fn linux(flags: u64) -> Self {
        Self {
            read: flags & 3 != 1,
            write: flags & 3 != 0,
            create: flags & 0x40 != 0,
            exclusive: flags & 0x80 != 0,
            truncate: flags & 0x200 != 0,
            append: flags & 0x400 != 0,
        }
    }
    /// From a C `fopen` mode string.
    pub fn stdio(mode: &str) -> Option<Self> {
        let plus = mode.contains('+');
        Some(match mode.chars().next()? {
            'r' => Self {
                read: true,
                write: plus,
                ..Self::default()
            },
            'w' => Self {
                read: plus,
                write: true,
                create: true,
                truncate: true,
                exclusive: mode.contains('x'),
                ..Self::default()
            },
            'a' => Self {
                read: plus,
                write: true,
                create: true,
                append: true,
                ..Self::default()
            },
            _ => return None,
        })
    }
}

/// An open view file with a POSIX file position.
pub struct File {
    handle: u64,
    position: u64,
    append: bool,
    readable: bool,
    writable: bool,
}
impl File {
    pub fn open(name: &str, o: Options) -> io::Result<Self> {
        let path = resolve(name)?;
        let mut flags = 0;
        if o.read || !o.write {
            flags |= wire::open::READ;
        }
        if o.write || o.append {
            flags |= wire::open::WRITE;
        }
        if o.create {
            flags |= wire::open::CREATE;
            // Saves into a missing directory: like the host files before,
            // create the parents inside the overlay.
            if let Some((parent, _)) = path.rsplit_once('/') {
                mkdirs(parent)?;
            }
        }
        if o.truncate {
            flags |= wire::open::TRUNCATE;
        }
        if o.exclusive {
            flags |= wire::open::EXCLUSIVE;
        }
        let (handle, _) = view()?.open(&path, flags)?;
        Ok(Self {
            handle,
            position: 0,
            append: o.append,
            readable: flags & wire::open::READ != 0,
            writable: flags & wire::open::WRITE != 0,
        })
    }
    pub fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        if !self.readable {
            return Err(io::Error::new(io::ErrorKind::Other, wire::Errno(wire::errno::EBADF)));
        }
        let n = view()?.read(self.handle, self.position, b)?;
        self.position += n as u64;
        Ok(n)
    }
    pub fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        if !self.writable {
            return Err(io::Error::new(io::ErrorKind::Other, wire::Errno(wire::errno::EBADF)));
        }
        if self.append {
            self.position = self.metadata()?.len;
        }
        let n = view()?.write(self.handle, self.position, b)?;
        self.position += n as u64;
        Ok(n)
    }
    pub fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let base = match to {
            SeekFrom::Start(n) => {
                self.position = n;
                return Ok(n);
            }
            SeekFrom::Current(d) => (self.position, d),
            SeekFrom::End(d) => (self.metadata()?.len, d),
        };
        let target = base
            .0
            .checked_add_signed(base.1)
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        self.position = target;
        Ok(target)
    }
    pub fn position(&self) -> u64 {
        self.position
    }
    pub fn metadata(&self) -> io::Result<Meta> {
        view()?.fstat(self.handle)
    }
}
impl Drop for File {
    fn drop(&mut self) {
        if let Ok(view) = view() {
            let _ = view.close(self.handle);
        }
    }
}

pub fn stat(name: &str) -> io::Result<Meta> {
    view()?.stat(&resolve(name)?)
}
pub fn remove(name: &str) -> io::Result<()> {
    view()?.remove(&resolve(name)?)
}
/// POSIX rename: replaces an existing destination.
pub fn rename(from: &str, to: &str) -> io::Result<()> {
    view()?.rename(&resolve(from)?, &resolve(to)?)
}
fn mkdirs(path: &str) -> io::Result<()> {
    let view = view()?;
    let mut at = String::new();
    for part in path.split('/') {
        if !at.is_empty() {
            at.push('/');
        }
        at.push_str(part);
        match view.stat(&at) {
            Ok(m) if m.directory => {}
            Ok(_) => return Err(io::ErrorKind::NotADirectory.into()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => view.mkdir(&at)?,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
/// Linux errno for an I/O error.
pub fn errno(e: &io::Error) -> i32 {
    wire::errno_of(e)
}
