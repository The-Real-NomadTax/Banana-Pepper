use super::{MountLease, View};
use fuser::*;
use std::{
    collections::BTreeMap,
    ffi::OsStr,
    io,
    path::Path,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
const TTL: Duration = Duration::from_secs(0);
struct Fs {
    files: BTreeMap<u64, super::FileHandle>,
    next_file: u64,
    view: Arc<View>,
    paths: BTreeMap<u64, String>,
}
fn errno(e: io::Error) -> i32 {
    match e.kind() {
        io::ErrorKind::NotFound => libc::ENOENT,
        io::ErrorKind::AlreadyExists => libc::EEXIST,
        io::ErrorKind::PermissionDenied => libc::EACCES,
        io::ErrorKind::DirectoryNotEmpty => libc::ENOTEMPTY,
        io::ErrorKind::IsADirectory => libc::EISDIR,
        io::ErrorKind::NotADirectory => libc::ENOTDIR,
        io::ErrorKind::InvalidInput => libc::EINVAL,
        _ => libc::EIO,
    }
}
impl Fs {
    fn p(&self, i: u64) -> io::Result<&str> {
        self.paths
            .get(&i)
            .map(String::as_str)
            .ok_or(io::ErrorKind::NotFound.into())
    }
    fn child(&self, i: u64, n: &OsStr) -> io::Result<String> {
        let n = n
            .to_str()
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        let p = self.p(i)?;
        let p = if p.is_empty() {
            n.into()
        } else {
            format!("{p}/{n}")
        };
        banana_content::path(&p)?;
        Ok(p)
    }
    fn inode(&mut self, p: String) -> u64 {
        if p.is_empty() {
            return 1;
        }
        let id = self.view.identity(&p).expect("Enumerated inode must exist");
        self.paths.insert(id, p);
        id
    }
    fn file(&mut self, i: u64) -> io::Result<u64> {
        let handle = self.view.open_file(self.p(i)?)?;
        self.next_file += 1;
        self.files.insert(self.next_file, handle);
        Ok(self.next_file)
    }
    fn attr(&self, i: u64) -> io::Result<FileAttr> {
        let p = self.p(i)?;
        let linked = i == 1 || self.view.identity(p).ok() == Some(i);
        let (m, a) = if linked {
            (self.view.metadata(p)?, self.view.attributes(p)?)
        } else {
            let file = self
                .files
                .values()
                .find(|f| f.identity() == i)
                .ok_or(io::ErrorKind::NotFound)?;
            (file.metadata()?, file.attributes()?)
        };
        Ok(FileAttr {
            ino: i,
            size: m.len,
            blocks: m.len.div_ceil(512),
            atime: UNIX_EPOCH + Duration::from_nanos(a.accessed_ns),
            mtime: UNIX_EPOCH + Duration::from_nanos(a.modified_ns),
            ctime: UNIX_EPOCH + Duration::from_nanos(a.modified_ns),
            crtime: UNIX_EPOCH + Duration::from_nanos(a.created_ns),
            kind: if m.directory {
                FileType::Directory
            } else {
                FileType::RegularFile
            },
            perm: a.mode.unwrap_or(if m.directory || m.executable {
                0o755
            } else {
                0o644
            }),
            nlink: if !linked {
                0
            } else if m.directory {
                2
            } else {
                1
            },
            uid: unsafe { libc::getuid() },
            gid: unsafe { libc::getgid() },
            rdev: 0,
            blksize: 4096,
            flags: 0,
        })
    }
    fn new_entry(&mut self, parent: u64, name: &OsStr, dir: bool) -> io::Result<FileAttr> {
        let p = self.child(parent, name)?;
        self.view.create(&p, dir)?;
        let i = self.inode(p);
        self.attr(i)
    }
}
impl Filesystem for Fs {
    fn lookup(&mut self, _: &Request<'_>, parent: u64, name: &OsStr, r: ReplyEntry) {
        let result = self.child(parent, name).and_then(|p| {
            self.view.metadata(&p)?;
            let i = self.inode(p);
            self.attr(i)
        });
        match result {
            Ok(a) => r.entry(&TTL, &a, 0),
            Err(e) => r.error(errno(e)),
        }
    }
    fn getattr(&mut self, _: &Request<'_>, i: u64, r: ReplyAttr) {
        match self.attr(i) {
            Ok(a) => r.attr(&TTL, &a),
            Err(e) => r.error(errno(e)),
        }
    }
    fn open(&mut self, _: &Request<'_>, i: u64, flags: i32, r: ReplyOpen) {
        let result = (|| {
            let p = self.p(i)?;
            self.view.metadata(p)?;
            if flags & libc::O_TRUNC != 0 {
                self.view.truncate(p, 0)?;
            }
            Ok::<_, io::Error>(())
        })();
        match result {
            Ok(()) => match self.file(i) {
                Ok(fh) => r.opened(fh, 0),
                Err(e) => r.error(errno(e)),
            },
            Err(e) => r.error(errno(e)),
        }
    }
    fn read(
        &mut self,
        _: &Request<'_>,
        i: u64,
        fh: u64,
        o: i64,
        size: u32,
        _: i32,
        _: Option<u64>,
        r: ReplyData,
    ) {
        if o < 0 {
            r.error(libc::EINVAL);
            return;
        }
        let mut b = vec![0; size as usize];
        match if let Some(file) = self.files.get(&fh) {
            file.read(o as u64, &mut b)
        } else {
            self.p(i).and_then(|p| self.view.read(p, o as u64, &mut b))
        } {
            Ok(n) => r.data(&b[..n]),
            Err(e) => r.error(errno(e)),
        }
    }
    fn write(
        &mut self,
        _: &Request<'_>,
        i: u64,
        fh: u64,
        o: i64,
        b: &[u8],
        _: u32,
        _: i32,
        _: Option<u64>,
        r: ReplyWrite,
    ) {
        if o < 0 {
            r.error(libc::EINVAL);
            return;
        }
        match if let Some(file) = self.files.get(&fh) {
            file.write(o as u64, b)
        } else {
            self.p(i).and_then(|p| self.view.write(p, o as u64, b))
        } {
            Ok(n) => r.written(n as u32),
            Err(e) => r.error(errno(e)),
        }
    }
    fn readdir(&mut self, _: &Request<'_>, i: u64, _: u64, offset: i64, mut r: ReplyDirectory) {
        let result = self
            .p(i)
            .and_then(|p| self.view.read_dir(p).map(|entries| (p.to_owned(), entries)));
        match result {
            Ok((p, entries)) => {
                let parent = p.rsplit_once('/').map_or("", |v| v.0).to_owned();
                let pi = self.inode(parent);
                let mut out = vec![
                    (i, FileType::Directory, ".".to_owned()),
                    (pi, FileType::Directory, "..".to_owned()),
                ];
                for e in entries {
                    let path = if p.is_empty() {
                        e.name.clone()
                    } else {
                        format!("{p}/{}", e.name)
                    };
                    let ino = self.inode(path);
                    out.push((
                        ino,
                        if e.metadata.directory {
                            FileType::Directory
                        } else {
                            FileType::RegularFile
                        },
                        e.name,
                    ));
                }
                for (index, (ino, kind, n)) in
                    out.into_iter().enumerate().skip(offset.max(0) as usize)
                {
                    if r.add(ino, (index + 1) as i64, kind, n) {
                        break;
                    }
                }
                r.ok()
            }
            Err(e) => r.error(errno(e)),
        }
    }
    fn create(
        &mut self,
        _: &Request<'_>,
        p: u64,
        n: &OsStr,
        _: u32,
        _: u32,
        _: i32,
        r: ReplyCreate,
    ) {
        match self.new_entry(p, n, false) {
            Ok(a) => match self.file(a.ino) {
                Ok(fh) => r.created(&TTL, &a, 0, fh, 0),
                Err(e) => r.error(errno(e)),
            },
            Err(e) => r.error(errno(e)),
        }
    }
    fn mkdir(&mut self, _: &Request<'_>, p: u64, n: &OsStr, _: u32, _: u32, r: ReplyEntry) {
        match self.new_entry(p, n, true) {
            Ok(a) => r.entry(&TTL, &a, 0),
            Err(e) => r.error(errno(e)),
        }
    }
    fn unlink(&mut self, _: &Request<'_>, p: u64, n: &OsStr, r: ReplyEmpty) {
        match self.child(p, n).and_then(|p| self.view.remove(&p)) {
            Ok(()) => r.ok(),
            Err(e) => r.error(errno(e)),
        }
    }
    fn rmdir(&mut self, req: &Request<'_>, p: u64, n: &OsStr, r: ReplyEmpty) {
        self.unlink(req, p, n, r)
    }
    fn rename(
        &mut self,
        _: &Request<'_>,
        p: u64,
        n: &OsStr,
        np: u64,
        nn: &OsStr,
        flags: u32,
        r: ReplyEmpty,
    ) {
        let result = (|| {
            if flags & !1 != 0 {
                return Err(io::Error::from(io::ErrorKind::Unsupported));
            }
            let a = self.child(p, n)?;
            let b = self.child(np, nn)?;
            self.view.rename(&a, &b, flags & 1 == 0)?;
            for path in self.paths.values_mut() {
                if *path == a || path.starts_with(&format!("{a}/")) {
                    *path = format!("{b}{}", &path[a.len()..]);
                }
            }
            Ok(())
        })();
        match result {
            Ok(()) => r.ok(),
            Err(e) => r.error(errno(e)),
        }
    }
    fn setattr(
        &mut self,
        _: &Request<'_>,
        i: u64,
        mode: Option<u32>,
        _: Option<u32>,
        _: Option<u32>,
        size: Option<u64>,
        atime: Option<TimeOrNow>,
        mtime: Option<TimeOrNow>,
        _: Option<SystemTime>,
        _: Option<u64>,
        _: Option<SystemTime>,
        _: Option<SystemTime>,
        _: Option<SystemTime>,
        _: Option<u32>,
        r: ReplyAttr,
    ) {
        let result = (|| {
            if let Some(n) = size {
                self.view.truncate(self.p(i)?, n)?;
            }
            self.view.set_attributes(
                self.p(i)?,
                super::Attributes {
                    mode: mode.map(|m| m as u16),
                    accessed_ns: time_ns(atime),
                    modified_ns: time_ns(mtime),
                    ..Default::default()
                },
            )?;
            self.attr(i)
        })();
        match result {
            Ok(a) => r.attr(&TTL, &a),
            Err(e) => r.error(errno(e)),
        }
    }
    fn flush(&mut self, _: &Request<'_>, _: u64, _: u64, _: u64, r: ReplyEmpty) {
        match self.view.flush() {
            Ok(()) => r.ok(),
            Err(e) => r.error(errno(e)),
        }
    }
    fn release(
        &mut self,
        _: &Request<'_>,
        _: u64,
        fh: u64,
        _: i32,
        _: Option<u64>,
        _: bool,
        r: ReplyEmpty,
    ) {
        self.files.remove(&fh);
        r.ok()
    }
    fn fsync(&mut self, _: &Request<'_>, _: u64, _: u64, _: bool, r: ReplyEmpty) {
        match self.view.flush() {
            Ok(()) => r.ok(),
            Err(e) => r.error(errno(e)),
        }
    }
}
pub fn mount(view: Arc<View>, point: &Path) -> io::Result<MountLease> {
    std::fs::create_dir_all(point)?;
    let fs = Fs {
        files: BTreeMap::new(),
        next_file: 0,
        view,
        paths: BTreeMap::from([(1, String::new())]),
    };
    let host = fuser::spawn_mount2(
        fs,
        point,
        &[
            MountOption::FSName("banana-pepper".into()),
            MountOption::RW,
            MountOption::NoSuid,
            MountOption::NoDev,
        ],
    )
    .map_err(|e| {
        io::Error::other(format!(
            "Cannot mount game filesystem. Enable FUSE and install fusermount3: {e}"
        ))
    })?;
    Ok(MountLease {
        path: point.into(),
        _host: Some(host),
        _lock: None,
        _lock_path: None,
    })
}

fn time_ns(t: Option<TimeOrNow>) -> u64 {
    let time = match t {
        Some(TimeOrNow::SpecificTime(t)) => t,
        Some(TimeOrNow::Now) => SystemTime::now(),
        None => return 0,
    };
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .min(u64::MAX as u128) as u64
}
