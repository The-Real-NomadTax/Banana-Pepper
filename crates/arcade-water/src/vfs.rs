//! Explicit virtual-root and writable-work mounts. No host PATH resolution.
use std::{io, path::{Path,PathBuf}};

pub struct Vfs { root: PathBuf, work: PathBuf }
impl Vfs {
    pub fn new(root:&Path,work:&Path)->io::Result<Self> {
        std::fs::create_dir_all(work)?;
        Ok(Self{root:root.canonicalize()?,work:work.canonicalize()?})
    }
    pub fn resolve(&self,name:&str,write:bool)->io::Result<PathBuf> {
        if name.contains(['\\',':','\0']) { return Err(io::Error::new(io::ErrorKind::InvalidInput,"Unsupported Linux filename")); }
        let mut parts=Vec::new();
        for part in name.split('/') {
            match part { ""|"."=>{},".."=>{parts.pop();},other=>parts.push(other) }
        }
        let is_work=parts.first()==Some(&"work");
        if write && !is_work { return Err(io::Error::new(io::ErrorKind::PermissionDenied,"Root mount is read-only")); }
        let mount=if is_work { &self.work } else { &self.root };
        let mut target=mount.clone();
        for part in &parts[usize::from(is_work)..] { target.push(part); }
        let resolved=if target.exists() { target.canonicalize()? } else {
            let parent=target.parent().ok_or_else(||io::Error::new(io::ErrorKind::InvalidInput,"Missing parent"))?.canonicalize()?;
            parent.join(target.file_name().ok_or_else(||io::Error::new(io::ErrorKind::InvalidInput,"Missing filename"))?)
        };
        if !resolved.starts_with(mount) { return Err(io::Error::new(io::ErrorKind::PermissionDenied,"Path leaves its configured mount")); }
        Ok(resolved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mounts_reject_windows_paths_and_read_only_writes() {
        let root=std::env::temp_dir().join(format!("arcade-vfs-{}",std::process::id()));
        let vfs=Vfs::new(&root,&root.join("work-data")).unwrap();
        for path in ["C:/Windows/a","/work/x:stream","/work/..\\outside"] { assert!(vfs.resolve(path,true).is_err()); }
        assert!(vfs.resolve("/readonly",true).is_err());
        assert!(vfs.resolve("/work/../../readonly",true).is_err());
        assert_eq!(vfs.resolve("/work/new-file",true).unwrap(),root.canonicalize().unwrap().join("work-data/new-file"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
