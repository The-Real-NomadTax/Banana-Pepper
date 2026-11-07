use banana_content::{ContentFile, ContentSource, Entry, MemoryFile, Metadata, invalid, path};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
const BLOCK: u64 = 65536;
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Attributes {
    pub mode: Option<u16>,
    pub flags: Option<u32>,
    pub created_ns: u64,
    pub modified_ns: u64,
    pub accessed_ns: u64,
}

#[derive(Clone, Serialize, Deserialize)]
struct Node {
    #[serde(default)]
    attributes: Attributes,
    id: u64,
    base: Option<String>,
    len: u64,
    base_len: u64,
    directory: bool,
    executable: bool,
    blocks: BTreeMap<u64, String>,
}
#[derive(Default, Serialize, Deserialize)]
struct Index {
    #[serde(default = "default_case_sensitive")]
    case_sensitive: bool,
    version: u32,
    fingerprint: String,
    next: u64,
    nodes: BTreeMap<String, Node>,
    deleted: BTreeSet<String>,
}
#[derive(Default)]
struct Handles {
    next: u64,
    records: BTreeMap<u64, OpenRecord>,
    orphans: BTreeMap<u64, Node>,
}
struct OpenRecord {
    path: String,
    inode: u64,
}
pub struct FileHandle {
    view: Arc<View>,
    id: u64,
}
impl FileHandle {
    pub fn read(&self, o: u64, b: &mut [u8]) -> io::Result<usize> {
        self.view.read_handle(self.id, o, b)
    }
    pub fn write(&self, o: u64, b: &[u8]) -> io::Result<usize> {
        self.view.write_handle(self.id, o, b)
    }
    pub fn metadata(&self) -> io::Result<Metadata> {
        self.view.handle_metadata(self.id)
    }
    pub fn attributes(&self) -> io::Result<Attributes> {
        self.view.handle_attributes(self.id)
    }
    pub fn identity(&self) -> u64 {
        self.view.handles.lock().unwrap().records[&self.id].inode
    }
}
impl Drop for FileHandle {
    fn drop(&mut self) {
        let mut h = self.view.handles.lock().unwrap();
        if let Some(r) = h.records.remove(&self.id) {
            if !h.records.values().any(|other| other.inode == r.inode) {
                h.orphans.remove(&r.inode);
            }
        }
    }
}
pub struct View {
    handles: Mutex<Handles>,
    case_sensitive: bool,
    source: Arc<dyn ContentSource>,
    generated: BTreeMap<String, Arc<[u8]>>,
    aliases: BTreeMap<String, String>,
    root: PathBuf,
    index: Mutex<Index>,
    _lock: File,
}
impl View {
    pub fn new(
        source: Arc<dyn ContentSource>,
        root: &Path,
        fingerprint: &str,
        generated: BTreeMap<String, Arc<[u8]>>,
        aliases: BTreeMap<String, String>,
    ) -> io::Result<Self> {
        Self::with_case(source, root, fingerprint, generated, aliases, true)
    }
    pub fn with_case(
        source: Arc<dyn ContentSource>,
        root: &Path,
        fingerprint: &str,
        generated: BTreeMap<String, Arc<[u8]>>,
        aliases: BTreeMap<String, String>,
        case_sensitive: bool,
    ) -> io::Result<Self> {
        let source: Arc<dyn ContentSource> = if case_sensitive {
            source
        } else {
            Arc::new(banana_content::CaseFoldSource(source))
        };
        let fold = |p: String| {
            if case_sensitive {
                p
            } else {
                p.to_ascii_lowercase()
            }
        };
        let generated = generated
            .into_iter()
            .map(|(p, b)| (fold(p), b))
            .collect::<BTreeMap<_, _>>();
        let aliases = aliases
            .into_iter()
            .map(|(a, b)| (fold(a), fold(b)))
            .collect::<BTreeMap<_, _>>();
        fs::create_dir_all(root)?;
        for p in generated.keys() {
            path(p)?;
        }
        for (a, b) in &aliases {
            path(a)?;
            path(b)?;
        }
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(root.join("session.lock"))?;
        lock.try_lock().map_err(|e| {
            io::Error::other(format!("Another session uses this library entry: {e}"))
        })?;
        let index = match fs::read(root.join("index.json")) {
            Ok(b) => serde_json::from_slice::<Index>(&b).map_err(io::Error::other)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Index {
                version: 1,
                case_sensitive,
                fingerprint: fingerprint.into(),
                ..Default::default()
            },
            Err(e) => return Err(e),
        };
        for (p, n) in &index.nodes {
            path(p)?;
            if let Some(base) = &n.base {
                path(base)?;
            }
            for h in n.blocks.values() {
                if h.len() != 64 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err(invalid("Invalid overlay block identifier"));
                }
            }
        }
        for p in &index.deleted {
            path(p)?;
        }
        if index.case_sensitive != case_sensitive {
            return Err(invalid(
                "Overlay case policy changed; migrate or reset this overlay",
            ));
        }
        if index.version != 1 {
            return Err(invalid("Unsupported overlay version"));
        }
        if index.fingerprint != fingerprint {
            return Err(invalid(
                "Source/build changed. Existing overlay retained; validate or reset it before launching this build.",
            ));
        }
        fs::create_dir_all(root.join("blocks"))?;
        Ok(Self {
            handles: Mutex::new(Handles::default()),
            case_sensitive,
            source,
            generated,
            aliases,
            root: root.into(),
            index: Mutex::new(index),
            _lock: lock,
        })
    }
    fn resolve(&self, p: &str) -> io::Result<String> {
        path(p)?;
        let mut p = if self.case_sensitive {
            p.to_owned()
        } else {
            p.to_ascii_lowercase()
        };
        for _ in 0..16 {
            let found = self
                .aliases
                .iter()
                .rev()
                .find(|(a, _)| p == **a || p.starts_with(&format!("{a}/")));
            match found {
                Some((a, b)) => {
                    p = format!("{b}{}", &p[a.len()..]);
                }
                None => return Ok(p),
            }
        }
        Err(invalid("Alias cycle"))
    }
    fn base_meta(&self, p: &str) -> io::Result<Metadata> {
        if let Some(b) = self.generated.get(p) {
            return Ok(Metadata {
                directory: false,
                len: b.len() as u64,
                executable: b.starts_with(b"\x7fELF") || b.starts_with(b"MZ"),
            });
        }
        let prefix = if p.is_empty() {
            String::new()
        } else {
            format!("{p}/")
        };
        if self.generated.keys().any(|k| k.starts_with(&prefix)) {
            return Ok(Metadata {
                directory: true,
                len: 0,
                executable: false,
            });
        }
        self.source.metadata(p)
    }
    fn base_file(&self, p: &str) -> io::Result<Arc<dyn ContentFile>> {
        if let Some(b) = self.generated.get(p) {
            Ok(Arc::new(MemoryFile(b.clone())))
        } else {
            self.source.open(p)
        }
    }
    fn hidden(i: &Index, p: &str) -> bool {
        i.deleted
            .iter()
            .any(|d| p == d || p.starts_with(&format!("{d}/")))
    }
    fn meta(&self, i: &Index, p: &str) -> io::Result<Metadata> {
        if let Some(n) = i.nodes.get(p) {
            return Ok(Metadata {
                directory: n.directory,
                len: n.len,
                executable: n.executable,
            });
        }
        if Self::hidden(i, p) {
            return Err(io::ErrorKind::NotFound.into());
        }
        self.base_meta(p)
    }
    pub fn metadata(&self, p: &str) -> io::Result<Metadata> {
        let p = self.resolve(p)?;
        self.meta(&self.index.lock().unwrap(), &p)
    }
    fn save(&self, i: &Index) -> io::Result<()> {
        let b = serde_json::to_vec(i).map_err(io::Error::other)?;
        atomic_write(&self.root.join("index.json"), &b)
    }
    fn node(&self, i: &mut Index, p: &str) -> io::Result<()> {
        if !i.nodes.contains_key(p) {
            let m = self.meta(i, p)?;
            i.next += 1;
            i.nodes.insert(
                p.into(),
                Node {
                    attributes: Attributes::default(),
                    id: source_id(p),
                    base: Some(p.into()),
                    len: m.len,
                    base_len: m.len,
                    directory: m.directory,
                    executable: m.executable,
                    blocks: BTreeMap::new(),
                },
            );
        }
        Ok(())
    }
    pub fn read(&self, p: &str, offset: u64, b: &mut [u8]) -> io::Result<usize> {
        let p = self.resolve(p)?;
        let i = self.index.lock().unwrap();
        self.read_locked(&i, &p, offset, b)
    }
    fn read_locked(&self, i: &Index, p: &str, offset: u64, b: &mut [u8]) -> io::Result<usize> {
        let m = self.meta(i, p)?;
        if m.directory {
            return Err(io::ErrorKind::IsADirectory.into());
        }
        if offset >= m.len {
            return Ok(0);
        }
        let count = (m.len - offset).min(b.len() as u64) as usize;
        b[..count].fill(0);
        let node = i.nodes.get(p);
        let base = node.and_then(|n| n.base.as_deref()).unwrap_or(p);
        let base_len = node.map_or(m.len, |n| n.base_len);
        let mut done = 0;
        while done < count {
            let o = offset + done as u64;
            let block = o / BLOCK;
            let within = o % BLOCK;
            let n = (BLOCK - within).min((count - done) as u64) as usize;
            if let Some(file) = node.and_then(|n| n.blocks.get(&block)) {
                let bytes = fs::read(self.root.join("blocks").join(file))?;
                let start = within as usize;
                if start < bytes.len() {
                    let take = n.min(bytes.len() - start);
                    b[done..done + take].copy_from_slice(&bytes[start..start + take]);
                }
            } else if o < base_len && (node.is_none() || node.unwrap().base.is_some()) {
                let take = n.min((base_len - o) as usize);
                let f = self.base_file(base)?;
                let mut used = 0;
                while used < take {
                    let got = f.read_at(o + used as u64, &mut b[done + used..done + take])?;
                    if got == 0 {
                        return Err(io::ErrorKind::UnexpectedEof.into());
                    }
                    used += got;
                }
            }
            done += n;
        }
        Ok(count)
    }
    pub fn write(&self, p: &str, offset: u64, b: &[u8]) -> io::Result<usize> {
        if b.is_empty() {
            return Ok(0);
        }
        let p = self.resolve(p)?;
        let mut g = self.index.lock().unwrap();
        let mut i = clone_index(&g)?;
        let n = self.write_index(&mut i, &p, offset, b)?;
        self.save(&i)?;
        *g = i;
        Ok(n)
    }
    fn write_index(&self, i: &mut Index, p: &str, offset: u64, b: &[u8]) -> io::Result<usize> {
        if b.is_empty() {
            return Ok(0);
        }
        let end = offset
            .checked_add(b.len() as u64)
            .ok_or_else(|| invalid("File size overflow"))?;
        if end > 2 * 1024 * 1024 * 1024 * 1024 {
            return Err(invalid("Virtual file exceeds 2 TiB"));
        }
        self.node(i, p)?;
        if i.nodes[p].directory {
            return Err(io::ErrorKind::IsADirectory.into());
        }
        let mut done = 0;
        while done < b.len() {
            let o = offset + done as u64;
            let block = o / BLOCK;
            let within = (o % BLOCK) as usize;
            let n = (BLOCK as usize - within).min(b.len() - done);
            let mut bytes = vec![0; BLOCK as usize];
            self.read_locked(i, p, block * BLOCK, &mut bytes)?;
            bytes[within..within + n].copy_from_slice(&b[done..done + n]);
            let hash = format!("{:x}", Sha256::digest(&bytes));
            let file = self.root.join("blocks").join(&hash);
            if !file.exists() {
                atomic_write(&file, &bytes)?;
            }
            i.nodes.get_mut(p).unwrap().blocks.insert(block, hash);
            done += n;
        }
        i.nodes.get_mut(p).unwrap().len = i.nodes[p].len.max(end);
        Ok(done)
    }
    /// Commit a complete small generated/configuration file in one metadata transaction.
    pub fn replace_content(&self, p: &str, bytes: &[u8]) -> io::Result<()> {
        let p = self.resolve(p)?;
        let mut guard = self.index.lock().unwrap();
        let mut i = clone_index(&guard)?;
        self.node(&mut i, &p)?;
        let n = i.nodes.get_mut(&p).unwrap();
        if n.directory {
            return Err(io::ErrorKind::IsADirectory.into());
        }
        n.base = None;
        n.base_len = 0;
        n.len = 0;
        n.blocks.clear();
        self.write_index(&mut i, &p, 0, bytes)?;
        self.save(&i)?;
        *guard = i;
        Ok(())
    }
    pub fn create(&self, p: &str, directory: bool) -> io::Result<()> {
        let p = self.resolve(p)?;
        if p.is_empty() {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        let mut guard = self.index.lock().unwrap();
        if self.meta(&guard, &p).is_ok() {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        let parent = p.rsplit_once('/').map_or("", |v| v.0);
        if !self.meta(&guard, parent)?.directory {
            return Err(io::ErrorKind::NotADirectory.into());
        }
        let mut i = clone_index(&guard)?;
        i.next += 1;
        i.nodes.insert(
            p,
            Node {
                attributes: Attributes::default(),
                id: (1 << 63) | i.next,
                base: None,
                len: 0,
                base_len: 0,
                directory,
                executable: false,
                blocks: BTreeMap::new(),
            },
        );
        self.save(&i)?;
        *guard = i;
        Ok(())
    }
    pub fn truncate(&self, p: &str, len: u64) -> io::Result<()> {
        if len > 2 * 1024 * 1024 * 1024 * 1024 {
            return Err(invalid("Virtual file exceeds 2 TiB"));
        }
        let p = self.resolve(p)?;
        let mut g = self.index.lock().unwrap();
        let mut i = clone_index(&g)?;
        self.node(&mut i, &p)?;
        let n = i.nodes.get_mut(&p).unwrap();
        if n.directory {
            return Err(io::ErrorKind::IsADirectory.into());
        }
        n.len = len;
        n.base_len = n.base_len.min(len);
        n.blocks.retain(|b, _| b * BLOCK < len);
        if len % BLOCK != 0 {
            let block = len / BLOCK;
            if let Some(hash) = n.blocks.get(&block).cloned() {
                let mut b = fs::read(self.root.join("blocks").join(hash))?;
                b[(len % BLOCK) as usize..].fill(0);
                let hash = format!("{:x}", Sha256::digest(&b));
                atomic_write(&self.root.join("blocks").join(&hash), &b)?;
                n.blocks.insert(block, hash);
            }
        }
        self.save(&i)?;
        *g = i;
        Ok(())
    }
    pub fn read_dir(&self, p: &str) -> io::Result<Vec<Entry>> {
        let p = self.resolve(p)?;
        let i = self.index.lock().unwrap();
        self.dir(&i, &p)
    }
    fn dir(&self, i: &Index, p: &str) -> io::Result<Vec<Entry>> {
        if !self.meta(i, p)?.directory {
            return Err(io::ErrorKind::NotADirectory.into());
        }
        let base = i.nodes.get(p).and_then(|n| n.base.as_deref()).unwrap_or(p);
        let mut names = BTreeSet::new();
        if let Ok(entries) = self.source.read_dir(base) {
            for e in entries {
                names.insert(e.name);
            }
        }
        let prefix = if p.is_empty() {
            String::new()
        } else {
            format!("{p}/")
        };
        for key in self
            .generated
            .keys()
            .chain(i.nodes.keys())
            .chain(self.aliases.keys())
        {
            if let Some(s) = key.strip_prefix(&prefix) {
                if let Some(n) = s.split('/').next() {
                    if !n.is_empty() {
                        names.insert(n.into());
                    }
                }
            }
        }
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        for name in names {
            if !seen.insert(if self.case_sensitive {
                name.clone()
            } else {
                name.to_ascii_lowercase()
            }) {
                continue;
            }
            let child = format!("{prefix}{name}");
            if let Ok(m) = self.meta(i, &self.resolve(&child)?) {
                out.push(Entry { name, metadata: m });
            }
        }
        Ok(out)
    }
    pub fn remove(&self, p: &str) -> io::Result<()> {
        let p = self.resolve(p)?;
        if p.is_empty() {
            return Err(invalid("Cannot remove virtual root"));
        }
        let mut g = self.index.lock().unwrap();
        if self.meta(&g, &p)?.directory && !self.dir(&g, &p)?.is_empty() {
            return Err(io::ErrorKind::DirectoryNotEmpty.into());
        }
        let mut i = clone_index(&g)?;
        i.nodes.remove(&p);
        i.deleted.insert(p.clone());
        let old = g.nodes.get(&p).cloned().unwrap_or_else(|| {
            let m = self.meta(&g, &p).unwrap();
            Node {
                attributes: Attributes::default(),
                id: source_id(&p),
                base: Some(p.clone()),
                base_len: m.len,
                len: m.len,
                directory: m.directory,
                executable: m.executable,
                blocks: BTreeMap::new(),
            }
        });
        self.save(&i)?;
        let mut h = self.handles.lock().unwrap();
        if h.records.values().any(|r| r.path == p) {
            h.orphans.insert(old.id, old);
        }
        *g = i;
        Ok(())
    }
    pub fn rename(&self, from: &str, to: &str, replace: bool) -> io::Result<()> {
        let from = self.resolve(from)?;
        let to = self.resolve(to)?;
        if from == to {
            return Ok(());
        }
        if from.is_empty() || to.is_empty() || to.starts_with(&format!("{from}/")) {
            return Err(invalid("Invalid rename"));
        }
        let mut g = self.index.lock().unwrap();
        let parent = to.rsplit_once('/').map_or("", |v| v.0);
        if !self.meta(&g, parent)?.directory {
            return Err(io::ErrorKind::NotADirectory.into());
        }
        if self.meta(&g, &to).is_ok() {
            if !replace {
                return Err(io::ErrorKind::AlreadyExists.into());
            }
            if self.meta(&g, &to)?.directory && !self.dir(&g, &to)?.is_empty() {
                return Err(io::ErrorKind::DirectoryNotEmpty.into());
            }
        }
        let from_meta = self.meta(&g, &from)?;
        if let Ok(to_meta) = self.meta(&g, &to) {
            if from_meta.directory != to_meta.directory {
                return Err(invalid(
                    "Cannot replace a file with a directory or a directory with a file",
                ));
            }
        }
        let mut i = clone_index(&g)?;
        i.nodes
            .retain(|key, _| key != &to && !key.starts_with(&format!("{to}/")));
        let mut todo = vec![(from.clone(), to.clone())];
        let mut moved = Vec::new();
        while let Some((a, b)) = todo.pop() {
            let m = self.meta(&i, &a)?;
            if m.directory {
                for e in self.dir(&i, &a)? {
                    todo.push((format!("{a}/{}", e.name), format!("{b}/{}", e.name)));
                }
            }
            self.node(&mut i, &a)?;
            moved.push((a, b));
        }
        for (a, b) in moved {
            let n = i.nodes.remove(&a).unwrap();
            i.nodes.insert(b, n);
        }
        i.deleted.insert(from.clone());
        i.deleted.insert(to.clone());
        self.save(&i)?;
        let mut h = self.handles.lock().unwrap();
        if let Ok(m) = self.meta(&g, &to) {
            let old = g.nodes.get(&to).cloned().unwrap_or_else(|| Node {
                attributes: Attributes::default(),
                id: source_id(&to),
                base: Some(to.clone()),
                base_len: m.len,
                len: m.len,
                directory: m.directory,
                executable: m.executable,
                blocks: BTreeMap::new(),
            });
            if h.records.values().any(|r| r.path == to) {
                h.orphans.insert(old.id, old);
            }
        }
        for r in h.records.values_mut() {
            if r.path == from || r.path.starts_with(&format!("{from}/")) {
                r.path = format!("{to}{}", &r.path[from.len()..]);
            }
        }
        *g = i;
        Ok(())
    }

    pub fn open_file(self: &Arc<Self>, p: &str) -> io::Result<FileHandle> {
        let p = self.resolve(p)?;
        let i = self.index.lock().unwrap();
        self.meta(&i, &p)?;
        let inode = i.nodes.get(&p).map_or_else(|| source_id(&p), |n| n.id);
        let mut h = self.handles.lock().unwrap();
        h.next += 1;
        let id = h.next;
        h.records.insert(id, OpenRecord { path: p, inode });
        Ok(FileHandle {
            view: self.clone(),
            id,
        })
    }
    fn handle_metadata(&self, id: u64) -> io::Result<Metadata> {
        let i = self.index.lock().unwrap();
        let h = self.handles.lock().unwrap();
        let r = h.records.get(&id).ok_or(io::ErrorKind::NotFound)?;
        if let Some(n) = h.orphans.get(&r.inode) {
            return Ok(Metadata {
                directory: n.directory,
                len: n.len,
                executable: n.executable,
            });
        }
        self.meta(&i, &r.path)
    }
    fn handle_attributes(&self, id: u64) -> io::Result<Attributes> {
        let i = self.index.lock().unwrap();
        let h = self.handles.lock().unwrap();
        let r = h.records.get(&id).ok_or(io::ErrorKind::NotFound)?;
        let node = h.orphans.get(&r.inode).or_else(|| i.nodes.get(&r.path));
        let mut a = node.map(|n| n.attributes.clone()).unwrap_or_default();
        if a.mode.is_none() {
            let m = if let Some(n) = node {
                Metadata {
                    directory: n.directory,
                    len: n.len,
                    executable: n.executable,
                }
            } else {
                self.meta(&i, &r.path)?
            };
            a.mode = Some(if m.directory || m.executable {
                0o755
            } else {
                0o644
            });
        }
        Ok(a)
    }
    fn read_handle(&self, id: u64, o: u64, b: &mut [u8]) -> io::Result<usize> {
        let i = self.index.lock().unwrap();
        let h = self.handles.lock().unwrap();
        let r = h.records.get(&id).ok_or(io::ErrorKind::NotFound)?;
        if let Some(n) = h.orphans.get(&r.inode) {
            let temp = Index {
                nodes: BTreeMap::from([(r.path.clone(), n.clone())]),
                ..Default::default()
            };
            return self.read_locked(&temp, &r.path, o, b);
        }
        self.read_locked(&i, &r.path, o, b)
    }
    fn write_handle(&self, id: u64, o: u64, b: &[u8]) -> io::Result<usize> {
        let mut g = self.index.lock().unwrap();
        let mut h = self.handles.lock().unwrap();
        let r = h.records.get(&id).ok_or(io::ErrorKind::NotFound)?;
        let p = r.path.clone();
        let inode = r.inode;
        if let Some(node) = h.orphans.get(&inode) {
            let mut temp = Index {
                nodes: BTreeMap::from([(p.clone(), node.clone())]),
                ..Default::default()
            };
            let n = self.write_index(&mut temp, &p, o, b)?;
            h.orphans.insert(inode, temp.nodes.remove(&p).unwrap());
            return Ok(n);
        }
        let mut i = clone_index(&g)?;
        let n = self.write_index(&mut i, &p, o, b)?;
        self.save(&i)?;
        *g = i;
        Ok(n)
    }
    pub fn canonical_path(&self, path: &str) -> io::Result<String> {
        self.resolve(path)
    }
    pub fn case_sensitive(&self) -> bool {
        self.case_sensitive
    }
    pub fn attributes(&self, p: &str) -> io::Result<Attributes> {
        let p = self.resolve(p)?;
        let i = self.index.lock().unwrap();
        let m = self.meta(&i, &p)?;
        let mut a = i
            .nodes
            .get(&p)
            .map(|n| n.attributes.clone())
            .unwrap_or_default();
        if a.mode.is_none() {
            a.mode = Some(if m.directory || m.executable {
                0o755
            } else {
                0o644
            })
        }
        Ok(a)
    }
    pub fn set_attributes(&self, p: &str, attributes: Attributes) -> io::Result<()> {
        let p = self.resolve(p)?;
        let mut g = self.index.lock().unwrap();
        let mut i = clone_index(&g)?;
        self.node(&mut i, &p)?;
        let n = i.nodes.get_mut(&p).unwrap();
        if let Some(mode) = attributes.mode {
            n.executable = mode & 0o111 != 0;
            n.attributes.mode = Some(mode);
        }
        if attributes.flags.is_some() {
            n.attributes.flags = attributes.flags
        }
        if attributes.created_ns != 0 {
            n.attributes.created_ns = attributes.created_ns
        }
        if attributes.modified_ns != 0 {
            n.attributes.modified_ns = attributes.modified_ns
        }
        if attributes.accessed_ns != 0 {
            n.attributes.accessed_ns = attributes.accessed_ns
        }
        self.save(&i)?;
        *g = i;
        Ok(())
    }
    pub fn identity(&self, p: &str) -> io::Result<u64> {
        let p = self.resolve(p)?;
        let i = self.index.lock().unwrap();
        self.meta(&i, &p)?;
        Ok(i.nodes.get(&p).map_or_else(|| source_id(&p), |n| n.id))
    }
    pub fn has_content_override(&self, p: &str) -> bool {
        self.resolve(p).is_ok_and(|p| {
            let i = self.index.lock().unwrap();
            if Self::hidden(&i, &p) {
                return true;
            }
            i.nodes.get(&p).is_some_and(|n| {
                !n.blocks.is_empty()
                    || n.base.as_deref() != Some(&p)
                    || self.base_meta(&p).is_ok_and(|m| m.len != n.len)
                    || n.base_len < n.len
            })
        })
    }
    pub fn has_overlay(&self, p: &str) -> bool {
        self.resolve(p).is_ok_and(|p| {
            let i = self.index.lock().unwrap();
            i.nodes.contains_key(&p) || Self::hidden(&i, &p)
        })
    }
    pub fn flush(&self) -> io::Result<()> {
        self.save(&self.index.lock().unwrap())
    }
}
fn clone_index(i: &Index) -> io::Result<Index> {
    serde_json::from_slice(&serde_json::to_vec(i).map_err(io::Error::other)?)
        .map_err(io::Error::other)
}
fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temp = path.with_extension("pending");
    let mut f = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    drop(f);
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        };
        use windows::core::PCWSTR;
        let a: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
        let b: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            MoveFileExW(
                PCWSTR(a.as_ptr()),
                PCWSTR(b.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
            .map_err(io::Error::other)?;
        }
    }
    #[cfg(not(windows))]
    {
        fs::rename(temp, path)?;
        File::open(path.parent().unwrap())?.sync_all()?;
    }
    Ok(())
}

fn source_id(p: &str) -> u64 {
    u64::from_le_bytes(Sha256::digest(p.as_bytes())[..8].try_into().unwrap()) & !(1 << 63)
}

impl Drop for View {
    fn drop(&mut self) {
        let Ok(i) = self.index.lock() else { return };
        let referenced = i
            .nodes
            .values()
            .flat_map(|n| n.blocks.values().cloned())
            .collect::<BTreeSet<_>>();
        if let Ok(files) = fs::read_dir(self.root.join("blocks")) {
            for f in files.flatten() {
                let name = f.file_name().to_string_lossy().into_owned();
                if name.len() == 64
                    && name.bytes().all(|b| b.is_ascii_hexdigit())
                    && !referenced.contains(&name)
                {
                    let _ = fs::remove_file(f.path());
                }
            }
        }
    }
}

fn default_case_sensitive() -> bool {
    true
}

/// Reset only private overlay data; original sources and captured state are untouched.
pub fn reset_overlay(root: &Path, entry_path: Option<&str>) -> io::Result<()> {
    if !root.exists() {
        return Ok(());
    }
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(root.join("session.lock"))?;
    lock.try_lock().map_err(|e| {
        io::Error::other(format!(
            "Close the active session before resetting its overlay: {e}"
        ))
    })?;
    let bytes = match fs::read(root.join("index.json")) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    let mut index: Index = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    if let Some(p) = entry_path {
        path(p)?;
        if p.is_empty() {
            return Err(invalid(
                "Specify a relative path or omit it to reset the complete overlay",
            ));
        }
        let p = if index.case_sensitive {
            p.into()
        } else {
            p.to_ascii_lowercase()
        };
        if index
            .deleted
            .iter()
            .any(|d| p.starts_with(&format!("{d}/")))
        {
            return Err(invalid("Reset the deleted parent directory first"));
        }
        index
            .nodes
            .retain(|n, _| n != &p && !n.starts_with(&format!("{p}/")));
        index
            .deleted
            .retain(|n| n != &p && !n.starts_with(&format!("{p}/")));
        atomic_write(
            &root.join("index.json"),
            &serde_json::to_vec(&index).map_err(io::Error::other)?,
        )?;
    } else {
        fs::remove_file(root.join("index.json"))?;
        index.nodes.clear();
        index.deleted.clear();
    }
    let referenced = index
        .nodes
        .values()
        .flat_map(|n| n.blocks.values().cloned())
        .collect::<BTreeSet<_>>();
    if let Ok(blocks) = fs::read_dir(root.join("blocks")) {
        for file in blocks {
            let file = file?;
            let name = file.file_name().to_string_lossy().into_owned();
            if name.len() == 64
                && name.bytes().all(|b| b.is_ascii_hexdigit())
                && !referenced.contains(&name)
            {
                fs::remove_file(file.path())?;
            }
        }
    }
    Ok(())
}
