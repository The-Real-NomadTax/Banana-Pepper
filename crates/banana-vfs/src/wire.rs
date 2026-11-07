//! Host side of the view protocol: serve a [`View`] to one guest connection.
//! The protocol and its guest-side client live in `banana-wire`.
pub use banana_wire::*;

use crate::{FileHandle, View};
use std::{
    collections::BTreeMap,
    io::{self, Read, Write},
    sync::Arc,
};

fn meta(m: banana_content::Metadata) -> Meta {
    Meta {
        directory: m.directory,
        executable: m.executable,
        len: m.len,
    }
}

struct Open {
    file: FileHandle,
    path: String,
    writable: bool,
}

/// Serve one connection until the client closes it. Handles still open when
/// it ends are released.
pub fn serve(view: Arc<View>, mut r: impl Read, mut w: impl Write) -> io::Result<()> {
    let mut handles: BTreeMap<u64, Open> = BTreeMap::new();
    let mut next = 0u64;
    let mut greeted = false;
    while let Some(frame) = read_frame(&mut r)? {
        let mut input = In(&frame);
        let op = Op::parse(input.u8()?)?;
        if !greeted && op != Op::Hello {
            return Err(invalid("View client did not negotiate a version"));
        }
        let mut out = Out::default();
        let result = (|| -> io::Result<()> {
            match op {
                Op::Hello => {
                    if input.u32()? != VERSION {
                        return Err(invalid("Unsupported view protocol version"));
                    }
                    greeted = true;
                    out.u32(VERSION);
                }
                Op::Open => {
                    let flags = input.u32()?;
                    let path = input.str()?;
                    let writable = flags & open::WRITE != 0;
                    match view.metadata(path) {
                        Ok(m) if m.directory => {
                            if writable || flags & open::TRUNCATE != 0 {
                                return Err(io::ErrorKind::IsADirectory.into());
                            }
                        }
                        Ok(_) if flags & open::EXCLUSIVE != 0 && flags & open::CREATE != 0 => {
                            return Err(io::ErrorKind::AlreadyExists.into());
                        }
                        Ok(_) => {}
                        Err(e) if e.kind() == io::ErrorKind::NotFound
                            && flags & open::CREATE != 0 =>
                        {
                            view.create(path, false)?
                        }
                        Err(e) => return Err(e),
                    }
                    if flags & open::TRUNCATE != 0 {
                        if !writable {
                            return Err(io::ErrorKind::PermissionDenied.into());
                        }
                        view.truncate(path, 0)?;
                    }
                    if handles.len() >= MAX_HANDLES {
                        return Err(io::Error::new(io::ErrorKind::Other, Errno(errno::EMFILE)));
                    }
                    let file = view.open_file(path)?;
                    let m = meta(file.metadata()?);
                    next += 1;
                    handles.insert(
                        next,
                        Open {
                            file,
                            path: path.to_owned(),
                            writable,
                        },
                    );
                    out.u64(next).meta(m);
                }
                Op::Read => {
                    let h = input.u64()?;
                    let offset = input.u64()?;
                    let len = (input.u32()? as usize).min(MAX_IO);
                    let file = &handles.get(&h).ok_or(Errno(errno::EBADF)).map_err(other)?.file;
                    let mut b = vec![0; len];
                    let mut n = 0;
                    while n < len {
                        let got = file.read(offset + n as u64, &mut b[n..])?;
                        if got == 0 {
                            break;
                        }
                        n += got;
                    }
                    out.bytes(&b[..n]);
                }
                Op::Write => {
                    let h = input.u64()?;
                    let offset = input.u64()?;
                    let data = input.bytes()?;
                    let open = handles.get(&h).ok_or(Errno(errno::EBADF)).map_err(other)?;
                    if !open.writable {
                        return Err(other(Errno(errno::EBADF)));
                    }
                    let n = open.file.write(offset, data)?;
                    out.u32(n as u32);
                }
                Op::Close => {
                    let h = input.u64()?;
                    handles
                        .remove(&h)
                        .ok_or(Errno(errno::EBADF))
                        .map_err(other)?;
                }
                Op::Stat => {
                    out.meta(meta(view.metadata(input.str()?)?));
                }
                Op::Fstat => {
                    let h = input.u64()?;
                    let file = &handles.get(&h).ok_or(Errno(errno::EBADF)).map_err(other)?.file;
                    out.meta(meta(file.metadata()?));
                }
                Op::Truncate => {
                    let h = input.u64()?;
                    let len = input.u64()?;
                    let open = handles.get(&h).ok_or(Errno(errno::EBADF)).map_err(other)?;
                    if !open.writable {
                        return Err(other(Errno(errno::EINVAL)));
                    }
                    view.truncate(&open.path, len)?;
                }
                Op::ReadDir => {
                    let entries = view.read_dir(input.str()?)?;
                    out.u32(entries.len() as u32);
                    for e in entries {
                        out.str(&e.name).meta(meta(e.metadata));
                    }
                }
                Op::Mkdir => view.create(input.str()?, true)?,
                Op::Remove => view.remove(input.str()?)?,
                Op::Rename => {
                    let from = input.str()?;
                    let to = input.str()?;
                    view.rename(from, to, true)?;
                }
                Op::Flush => view.flush()?,
            }
            Ok(())
        })();
        let reply = match result {
            Ok(()) => {
                let mut reply = Out::default();
                reply.i32(0).0.extend_from_slice(&out.0);
                reply.0
            }
            Err(e) if op == Op::Hello => return Err(e),
            Err(e) => Out::default().i32(errno_of(&e)).0.clone(),
        };
        write_frame(&mut w, &reply)?;
    }
    Ok(())
}
fn other(e: Errno) -> io::Error {
    io::Error::new(io::ErrorKind::Other, e)
}

