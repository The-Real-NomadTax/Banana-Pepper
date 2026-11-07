use super::{MountLease, View};
use std::{
    ffi::c_void,
    io,
    path::Path,
    sync::{Arc, Mutex, Weak},
};
use windows::Win32::Foundation::*;
use winfsp::{
    Result, U16CStr,
    filesystem::*,
    host::{FileSystemHost, VolumeParams},
};
pub type Host = FileSystemHost<Fs>;
pub struct Fs {
    view: Arc<View>,
    handles: Mutex<Vec<Weak<Mutex<String>>>>,
}
pub struct Handle {
    file: super::FileHandle,
    path: Arc<Mutex<String>>,
    buffer: DirBuffer,
}
fn name(s: &U16CStr) -> String {
    s.to_string_lossy()
        .trim_start_matches('\\')
        .replace('\\', "/")
}
impl Fs {
    fn info(&self, p: &str, out: &mut FileInfo) -> Result<()> {
        fill_info(
            self.view.metadata(p)?,
            self.view.attributes(p)?,
            self.view.identity(p)?,
            out,
        );
        Ok(())
    }
    fn handle_info(&self, h: &Handle, out: &mut FileInfo) -> Result<()> {
        fill_info(
            h.file.metadata()?,
            h.file.attributes()?,
            h.file.identity(),
            out,
        );
        Ok(())
    }
    fn handle(&self, p: String) -> Result<Handle> {
        let file = self.view.open_file(&p)?;
        let path = Arc::new(Mutex::new(p));
        self.handles.lock().unwrap().push(Arc::downgrade(&path));
        Ok(Handle {
            file,
            path,
            buffer: DirBuffer::new(),
        })
    }
}
impl FileSystemContext for Fs {
    type FileContext = Handle;
    fn get_security_by_name(
        &self,
        file_name: &U16CStr,
        _: Option<&mut [c_void]>,
        _: impl FnOnce(&U16CStr) -> Option<FileSecurity>,
    ) -> Result<FileSecurity> {
        let m = self.view.metadata(&name(file_name))?;
        Ok(FileSecurity {
            reparse: false,
            sz_security_descriptor: 0,
            attributes: if m.directory { 0x10 } else { 0x20 },
        })
    }
    fn open(&self, file_name: &U16CStr, _: u32, _: u32, info: &mut OpenFileInfo) -> Result<Handle> {
        let p = name(file_name);
        self.info(&p, info.as_mut())?;
        self.handle(p)
    }
    fn close(&self, _: Handle) {}
    fn create(
        &self,
        file_name: &U16CStr,
        options: u32,
        _: u32,
        _: u32,
        _: Option<&[c_void]>,
        _: u64,
        _: Option<&[u8]>,
        _: bool,
        info: &mut OpenFileInfo,
    ) -> Result<Handle> {
        let p = name(file_name);
        self.view.create(&p, options & 1 != 0)?;
        self.info(&p, info.as_mut())?;
        self.handle(p)
    }
    fn get_file_info(&self, h: &Handle, info: &mut FileInfo) -> Result<()> {
        self.handle_info(h, info)
    }
    fn read(&self, h: &Handle, b: &mut [u8], o: u64) -> Result<u32> {
        Ok(h.file.read(o, b)? as u32)
    }
    fn write(
        &self,
        h: &Handle,
        b: &[u8],
        mut o: u64,
        eof: bool,
        constrained: bool,
        info: &mut FileInfo,
    ) -> Result<u32> {
        let len = h.file.metadata()?.len;
        if eof {
            o = len;
        }
        let b = if constrained {
            &b[..b.len().min(len.saturating_sub(o) as usize)]
        } else {
            b
        };
        let n = h.file.write(o, b)?;
        self.handle_info(h, info)?;
        Ok(n as u32)
    }
    fn flush(&self, h: Option<&Handle>, info: &mut FileInfo) -> Result<()> {
        self.view.flush()?;
        if let Some(h) = h {
            self.info(&h.path.lock().unwrap(), info)?;
        }
        Ok(())
    }
    fn overwrite(
        &self,
        h: &Handle,
        _: u32,
        _: bool,
        _: u64,
        _: Option<&[u8]>,
        info: &mut FileInfo,
    ) -> Result<()> {
        let p = h.path.lock().unwrap();
        self.view.truncate(&p, 0)?;
        self.info(&p, info)
    }
    fn set_file_size(
        &self,
        h: &Handle,
        n: u64,
        allocation: bool,
        info: &mut FileInfo,
    ) -> Result<()> {
        let p = h.path.lock().unwrap();
        if !allocation || n < self.view.metadata(&p)?.len {
            self.view.truncate(&p, n)?;
        }
        self.info(&p, info)
    }
    fn set_basic_info(
        &self,
        h: &Handle,
        flags: u32,
        created: u64,
        accessed: u64,
        modified: u64,
        _: u64,
        info: &mut FileInfo,
    ) -> Result<()> {
        let p = h.path.lock().unwrap();
        self.view.set_attributes(
            &p,
            super::Attributes {
                flags: (flags != u32::MAX).then_some(flags),
                created_ns: unix_time(created),
                accessed_ns: unix_time(accessed),
                modified_ns: unix_time(modified),
                ..Default::default()
            },
        )?;
        self.info(&p, info)
    }
    fn set_delete(&self, h: &Handle, _: &U16CStr, delete: bool) -> Result<()> {
        if delete {
            let p = h.path.lock().unwrap();
            if self.view.metadata(&p)?.directory && !self.view.read_dir(&p)?.is_empty() {
                return Err(STATUS_DIRECTORY_NOT_EMPTY.into());
            }
        }
        Ok(())
    }
    fn cleanup(&self, h: &Handle, _: Option<&U16CStr>, flags: u32) {
        if flags & 1 != 0 {
            let _ = self.view.remove(&h.path.lock().unwrap());
        }
    }
    fn rename(&self, _: &Handle, old: &U16CStr, new: &U16CStr, replace: bool) -> Result<()> {
        let a = name(old);
        let b = name(new);
        self.view.rename(&a, &b, replace)?;
        let mut handles = self.handles.lock().unwrap();
        handles.retain(|h| h.strong_count() > 0);
        for h in handles.iter().filter_map(Weak::upgrade) {
            let mut p = h.lock().unwrap();
            if *p == a || p.starts_with(&format!("{a}/")) {
                *p = format!("{b}{}", &p[a.len()..]);
            }
        }
        Ok(())
    }
    fn read_directory(
        &self,
        h: &Handle,
        _: Option<&U16CStr>,
        marker: DirMarker,
        b: &mut [u8],
    ) -> Result<u32> {
        let p = h.path.lock().unwrap();
        if let Ok(lock) = h.buffer.acquire(marker.is_none(), None) {
            for e in self.view.read_dir(&p)? {
                let mut d = DirInfo::<255>::default();
                d.set_name(&e.name)?;
                let full = if p.is_empty() {
                    e.name
                } else {
                    format!("{p}/{}", e.name)
                };
                self.info(&full, d.file_info_mut())?;
                lock.write(&mut d)?;
            }
        }
        Ok(h.buffer.read(marker, b))
    }
    fn get_volume_info(&self, info: &mut VolumeInfo) -> Result<()> {
        info.total_size = 2 * 1024 * 1024 * 1024 * 1024;
        info.free_size = 1024 * 1024 * 1024 * 1024;
        Ok(())
    }
}
pub fn mount(view: Arc<View>, point: &Path) -> io::Result<MountLease> {
    winfsp::winfsp_init().map_err(|e|io::Error::other(format!("WinFsp is required to mount game content. Install WinFsp 2.1 or later, then restart Banana Pepper: {e}")))?;
    let mut params = VolumeParams::new();
    params
        .filesystem_name("BananaFS")
        .sector_size(4096)
        .sectors_per_allocation_unit(1)
        .case_sensitive_search(view.case_sensitive())
        .case_preserved_names(true)
        .persistent_acls(false)
        .file_info_timeout(0)
        .flush_and_purge_on_cleanup(true);
    let mut host = Host::new(
        params,
        Fs {
            view,
            handles: Mutex::new(Vec::new()),
        },
    )
    .map_err(io::Error::other)?;
    host.mount(point).map_err(io::Error::other)?;
    host.start().map_err(io::Error::other)?;
    Ok(MountLease {
        path: point.into(),
        _host: Some(host),
        _lock: None,
        _lock_path: None,
    })
}

fn nt_time(ns: u64) -> u64 {
    116444736000000000 + ns / 100
}
fn unix_time(t: u64) -> u64 {
    t.saturating_sub(116444736000000000).saturating_mul(100)
}

fn fill_info(m: banana_content::Metadata, a: super::Attributes, id: u64, out: &mut FileInfo) {
    *out = FileInfo {
        file_attributes: a.flags.unwrap_or(if m.directory { 0x10 } else { 0x20 }),
        allocation_size: m.len.div_ceil(4096) * 4096,
        file_size: m.len,
        index_number: id,
        creation_time: nt_time(a.created_ns),
        last_access_time: nt_time(a.accessed_ns),
        last_write_time: nt_time(a.modified_ns),
        change_time: nt_time(a.modified_ns),
        ..Default::default()
    };
}
