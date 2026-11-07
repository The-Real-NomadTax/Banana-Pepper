//! Request/response protocol exposing a session view to another process over
//! any byte stream. `banana_vfs::wire::serve` is the host side; [`Client`] is
//! the guest side and has no dependencies, so it fits inside guest DLLs. Backends whose guest file calls are intercepted (rather than
//! served by an OS mount) use it: the session host owns the view, overlay and
//! locks; the guest-side client only names paths and handles.
//!
//! Frames are `u32` little-endian length + payload. A request payload is an
//! opcode byte followed by its fields; a reply is an `i32` status (0, or a
//! Linux errno) followed by the result fields. Strings are `u32` length +
//! UTF-8. One request is outstanding per connection.
use std::{
    io::{self, Read, Write},
    sync::Mutex,
};

pub const VERSION: u32 = 1;
/// Largest frame either side accepts.
const MAX_FRAME: usize = 16 * 1024 * 1024;
/// Largest single read or write.
pub const MAX_IO: usize = 8 * 1024 * 1024;
#[doc(hidden)]
pub const MAX_HANDLES: usize = 4096;

pub mod open {
    pub const READ: u32 = 1;
    pub const WRITE: u32 = 2;
    pub const CREATE: u32 = 4;
    pub const TRUNCATE: u32 = 8;
    pub const EXCLUSIVE: u32 = 16;
}

#[doc(hidden)]
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Hello = 0,
    Open = 1,
    Read = 2,
    Write = 3,
    Close = 4,
    Stat = 5,
    Fstat = 6,
    Truncate = 7,
    ReadDir = 8,
    Mkdir = 9,
    Remove = 10,
    Rename = 11,
    Flush = 12,
}
impl Op {
    pub fn parse(b: u8) -> io::Result<Self> {
        use Op::*;
        Ok(match b {
            0 => Hello,
            1 => Open,
            2 => Read,
            3 => Write,
            4 => Close,
            5 => Stat,
            6 => Fstat,
            7 => Truncate,
            8 => ReadDir,
            9 => Mkdir,
            10 => Remove,
            11 => Rename,
            12 => Flush,
            _ => return Err(invalid("Unknown view request")),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Meta {
    pub directory: bool,
    pub executable: bool,
    pub len: u64,
}

/// Linux errno values carried by the protocol (both current clients emulate
/// Linux guests; Windows-side clients map them to Win32 errors).
pub mod errno {
    pub const ENOENT: i32 = 2;
    pub const EIO: i32 = 5;
    pub const EBADF: i32 = 9;
    pub const EACCES: i32 = 13;
    pub const EEXIST: i32 = 17;
    pub const ENOTDIR: i32 = 20;
    pub const EISDIR: i32 = 21;
    pub const EINVAL: i32 = 22;
    pub const EMFILE: i32 = 24;
    pub const ENOTEMPTY: i32 = 39;
}

/// The errno an I/O error stands for.
pub fn errno_of(e: &io::Error) -> i32 {
    if let Some(code) = e.get_ref().and_then(|r| r.downcast_ref::<Errno>()) {
        return code.0;
    }
    use io::ErrorKind::*;
    match e.kind() {
        NotFound => errno::ENOENT,
        PermissionDenied | ReadOnlyFilesystem => errno::EACCES,
        AlreadyExists => errno::EEXIST,
        NotADirectory => errno::ENOTDIR,
        IsADirectory => errno::EISDIR,
        InvalidInput | InvalidData | InvalidFilename => errno::EINVAL,
        DirectoryNotEmpty => errno::ENOTEMPTY,
        _ => errno::EIO,
    }
}

/// An error reported by the other side, keeping its exact errno.
#[derive(Debug)]
pub struct Errno(pub i32);
impl std::fmt::Display for Errno {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "view error {}", self.0)
    }
}
impl std::error::Error for Errno {}
pub fn from_errno(code: i32) -> io::Error {
    use io::ErrorKind::*;
    let kind = match code {
        errno::ENOENT => NotFound,
        errno::EACCES => PermissionDenied,
        errno::EEXIST => AlreadyExists,
        errno::ENOTDIR => NotADirectory,
        errno::EISDIR => IsADirectory,
        errno::EINVAL => InvalidInput,
        errno::ENOTEMPTY => DirectoryNotEmpty,
        _ => Other,
    };
    io::Error::new(kind, Errno(code))
}

#[doc(hidden)]
pub fn invalid(m: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, m.to_owned())
}

#[doc(hidden)]
pub fn read_frame(r: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut len = [0; 4];
    match r.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_FRAME {
        return Err(invalid("View frame exceeds its limit"));
    }
    let mut b = vec![0; len];
    r.read_exact(&mut b)?;
    Ok(Some(b))
}
#[doc(hidden)]
pub fn write_frame(w: &mut impl Write, b: &[u8]) -> io::Result<()> {
    if b.len() > MAX_FRAME {
        return Err(invalid("View frame exceeds its limit"));
    }
    let mut frame = Vec::with_capacity(4 + b.len());
    frame.extend_from_slice(&(b.len() as u32).to_le_bytes());
    frame.extend_from_slice(b);
    w.write_all(&frame)?;
    w.flush()
}

#[doc(hidden)]
#[derive(Default)]
pub struct Out(pub Vec<u8>);
impl Out {
    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.0.push(v);
        self
    }
    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn i32(&mut self, v: i32) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn u64(&mut self, v: u64) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn bytes(&mut self, v: &[u8]) -> &mut Self {
        self.u32(v.len() as u32);
        self.0.extend_from_slice(v);
        self
    }
    pub fn str(&mut self, v: &str) -> &mut Self {
        self.bytes(v.as_bytes())
    }
    pub fn meta(&mut self, m: Meta) -> &mut Self {
        self.u8(u8::from(m.directory))
            .u8(u8::from(m.executable))
            .u64(m.len)
    }
}
#[doc(hidden)]
pub struct In<'a>(pub &'a [u8]);
impl<'a> In<'a> {
    pub fn take(&mut self, n: usize) -> io::Result<&'a [u8]> {
        if self.0.len() < n {
            return Err(invalid("Truncated view message"));
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
    pub fn u8(&mut self) -> io::Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn i32(&mut self) -> io::Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    pub fn bytes(&mut self) -> io::Result<&'a [u8]> {
        let n = self.u32()? as usize;
        self.take(n)
    }
    pub fn str(&mut self) -> io::Result<&'a str> {
        std::str::from_utf8(self.bytes()?).map_err(|_| invalid("View paths must be UTF-8"))
    }
    pub fn meta(&mut self) -> io::Result<Meta> {
        Ok(Meta {
            directory: self.u8()? != 0,
            executable: self.u8()? != 0,
            len: self.u64()?,
        })
    }
}

type Stream = (Box<dyn Read + Send>, Box<dyn Write + Send>);

/// The guest side of a connection. Calls are serialized; it is safe to share.
pub struct Client {
    stream: Mutex<Stream>,
}
impl Client {
    pub fn new(r: impl Read + Send + 'static, w: impl Write + Send + 'static) -> io::Result<Self> {
        let client = Self {
            stream: Mutex::new((Box::new(r), Box::new(w))),
        };
        let version = client.call(Op::Hello, |o| {
            o.u32(VERSION);
        }, |i| i.u32())?;
        if version != VERSION {
            return Err(invalid("Unsupported view protocol version"));
        }
        Ok(client)
    }
    fn call<T>(
        &self,
        op: Op,
        fields: impl FnOnce(&mut Out),
        result: impl FnOnce(&mut In) -> io::Result<T>,
    ) -> io::Result<T> {
        let mut request = Out::default();
        request.u8(op as u8);
        fields(&mut request);
        let mut stream = self.stream.lock().unwrap_or_else(|e| e.into_inner());
        write_frame(&mut stream.1, &request.0)?;
        let reply = read_frame(&mut stream.0)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "View host closed"))?;
        drop(stream);
        let mut input = In(&reply);
        match input.i32()? {
            0 => result(&mut input),
            code => Err(from_errno(code)),
        }
    }
    /// Open `path` (view-relative) with [`open`] flags.
    pub fn open(&self, path: &str, flags: u32) -> io::Result<(u64, Meta)> {
        self.call(
            Op::Open,
            |o| {
                o.u32(flags).str(path);
            },
            |i| Ok((i.u64()?, i.meta()?)),
        )
    }
    /// Read up to `b.len()` bytes; short only at end of file.
    pub fn read(&self, handle: u64, offset: u64, b: &mut [u8]) -> io::Result<usize> {
        let mut done = 0;
        while done < b.len() {
            let want = (b.len() - done).min(MAX_IO);
            let n = self.call(
                Op::Read,
                |o| {
                    o.u64(handle).u64(offset + done as u64).u32(want as u32);
                },
                |i| {
                    let data = i.bytes()?;
                    b[done..done + data.len()].copy_from_slice(data);
                    Ok(data.len())
                },
            )?;
            done += n;
            if n < want {
                break;
            }
        }
        Ok(done)
    }
    pub fn write(&self, handle: u64, offset: u64, b: &[u8]) -> io::Result<usize> {
        let mut done = 0;
        for chunk in b.chunks(MAX_IO) {
            done += self.call(
                Op::Write,
                |o| {
                    o.u64(handle).u64(offset + done as u64).bytes(chunk);
                },
                |i| Ok(i.u32()? as usize),
            )?;
        }
        Ok(done)
    }
    pub fn close(&self, handle: u64) -> io::Result<()> {
        self.call(
            Op::Close,
            |o| {
                o.u64(handle);
            },
            |_| Ok(()),
        )
    }
    pub fn stat(&self, path: &str) -> io::Result<Meta> {
        self.call(
            Op::Stat,
            |o| {
                o.str(path);
            },
            |i| i.meta(),
        )
    }
    pub fn fstat(&self, handle: u64) -> io::Result<Meta> {
        self.call(
            Op::Fstat,
            |o| {
                o.u64(handle);
            },
            |i| i.meta(),
        )
    }
    pub fn truncate(&self, handle: u64, len: u64) -> io::Result<()> {
        self.call(
            Op::Truncate,
            |o| {
                o.u64(handle).u64(len);
            },
            |_| Ok(()),
        )
    }
    pub fn read_dir(&self, path: &str) -> io::Result<Vec<(String, Meta)>> {
        self.call(
            Op::ReadDir,
            |o| {
                o.str(path);
            },
            |i| {
                let n = i.u32()? as usize;
                let mut entries = Vec::with_capacity(n.min(65536));
                for _ in 0..n {
                    entries.push((i.str()?.to_owned(), i.meta()?));
                }
                Ok(entries)
            },
        )
    }
    pub fn mkdir(&self, path: &str) -> io::Result<()> {
        self.call(
            Op::Mkdir,
            |o| {
                o.str(path);
            },
            |_| Ok(()),
        )
    }
    pub fn remove(&self, path: &str) -> io::Result<()> {
        self.call(
            Op::Remove,
            |o| {
                o.str(path);
            },
            |_| Ok(()),
        )
    }
    pub fn rename(&self, from: &str, to: &str) -> io::Result<()> {
        self.call(
            Op::Rename,
            |o| {
                o.str(from).str(to);
            },
            |_| Ok(()),
        )
    }
    pub fn flush(&self) -> io::Result<()> {
        self.call(Op::Flush, |_| {}, |_| Ok(()))
    }
    /// Read a whole file (bounded by `limit`).
    pub fn read_all(&self, path: &str, limit: u64) -> io::Result<Vec<u8>> {
        let (h, m) = self.open(path, open::READ)?;
        let result = (|| {
            if m.len > limit {
                return Err(invalid("File exceeds read limit"));
            }
            let mut b = vec![0; m.len as usize];
            let n = self.read(h, 0, &mut b)?;
            b.truncate(n);
            Ok(b)
        })();
        let _ = self.close(h);
        result
    }
}
