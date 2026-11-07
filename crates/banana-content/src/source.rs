use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom},
    path::Path,
    sync::{Arc, mpsc},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Metadata {
    pub directory: bool,
    pub len: u64,
    pub executable: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub metadata: Metadata,
}
pub trait ContentFile: Send + Sync {
    fn len(&self) -> u64;
    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> io::Result<usize>;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
pub trait ContentSource: Send + Sync {
    fn fingerprint(&self) -> io::Result<String> {
        fingerprint(self)
    }
    fn metadata(&self, path: &str) -> io::Result<Metadata>;
    fn read_dir(&self, path: &str) -> io::Result<Vec<Entry>>;
    fn open(&self, path: &str) -> io::Result<Arc<dyn ContentFile>>;
}
pub fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}
pub fn path(path: &str) -> io::Result<&str> {
    if path.is_empty() {
        return Ok(path);
    }
    if path.contains('\\') {
        return Err(invalid("Virtual paths must use /"));
    }
    super::validate_path(path).map_err(invalid)?;
    Ok(path)
}
pub fn read_all(source: &dyn ContentSource, name: &str, limit: u64) -> io::Result<Vec<u8>> {
    let file = source.open(name)?;
    if file.len() > limit {
        return Err(invalid("File exceeds read limit"));
    }
    let mut bytes = vec![0; file.len() as usize];
    let mut offset = 0;
    while offset < bytes.len() {
        let n = file.read_at(offset as u64, &mut bytes[offset..])?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        offset += n;
    }
    Ok(bytes)
}
pub trait ContentProvider: Send + Sync {
    fn kind(&self) -> &'static str;
    fn accepts(&self, source: &Path) -> bool;
    fn open(&self, source: &Path) -> io::Result<Arc<dyn ContentSource>>;
}
pub struct ProviderRegistry {
    providers: Vec<Arc<dyn ContentProvider>>,
}
impl Default for ProviderRegistry {
    fn default() -> Self {
        Self {
            providers: vec![Arc::new(FolderProvider), Arc::new(ZarProvider)],
        }
    }
}
impl ProviderRegistry {
    pub fn register(&mut self, provider: Arc<dyn ContentProvider>) -> io::Result<()> {
        if self.providers.iter().any(|p| p.kind() == provider.kind()) {
            return Err(invalid("Duplicate content provider"));
        }
        self.providers.insert(0, provider);
        Ok(())
    }
    pub fn supports(&self, source: &Path) -> bool {
        self.providers.iter().any(|p| p.accepts(source))
    }
    pub fn open(&self, source: &Path) -> io::Result<Arc<dyn ContentSource>> {
        self.providers
            .iter()
            .find(|p| p.accepts(source))
            .ok_or_else(|| invalid("No content provider supports this source"))?
            .open(source)
    }
}
static PROVIDERS: std::sync::OnceLock<ProviderRegistry> = std::sync::OnceLock::new();
pub fn install_providers(providers: ProviderRegistry) -> io::Result<()> {
    PROVIDERS
        .set(providers)
        .map_err(|_| invalid("Providers already installed"))
}
pub fn supports(source: &Path) -> bool {
    PROVIDERS
        .get_or_init(ProviderRegistry::default)
        .supports(source)
}
pub fn open(source: &Path) -> io::Result<Arc<dyn ContentSource>> {
    PROVIDERS
        .get_or_init(ProviderRegistry::default)
        .open(source)
}
struct FolderProvider;
impl ContentProvider for FolderProvider {
    fn kind(&self) -> &'static str {
        "folder"
    }
    fn accepts(&self, p: &Path) -> bool {
        p.is_dir()
    }
    fn open(&self, p: &Path) -> io::Result<Arc<dyn ContentSource>> {
        Ok(Arc::new(Folder::new(p)?))
    }
}
struct ZarProvider;
impl ContentProvider for ZarProvider {
    fn kind(&self) -> &'static str {
        "zar"
    }
    fn accepts(&self, p: &Path) -> bool {
        p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
            ["zar", "zarchive", "wua"]
                .iter()
                .any(|s| s.eq_ignore_ascii_case(e))
        })
    }
    fn open(&self, p: &Path) -> io::Result<Arc<dyn ContentSource>> {
        Ok(Arc::new(Zar::new(p).map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("Cannot load ZArchive {}: {e}", p.display()),
            )
        })?))
    }
}
pub struct Folder {
    root: cap_std::fs::Dir,
}
impl Folder {
    pub fn new(root: &Path) -> io::Result<Self> {
        Ok(Self {
            root: cap_std::fs::Dir::open_ambient_dir(root, cap_std::ambient_authority())?,
        })
    }
}
impl ContentSource for Folder {
    fn metadata(&self, p: &str) -> io::Result<Metadata> {
        path(p)?;
        let m = if p.is_empty() {
            self.root.dir_metadata()?
        } else {
            self.root.metadata(p)?
        };
        if !m.is_dir() && !m.is_file() {
            return Err(invalid("Special source files are unsupported"));
        }
        let mut magic = [0; 4];
        let executable = if m.is_file() {
            let mut f = self.root.open(p)?;
            f.read(&mut magic)? == 4 && (magic == *b"\x7fELF" || magic[..2] == *b"MZ")
        } else {
            false
        };
        Ok(Metadata {
            directory: m.is_dir(),
            len: if m.is_dir() { 0 } else { m.len() },
            executable,
        })
    }
    fn read_dir(&self, p: &str) -> io::Result<Vec<Entry>> {
        path(p)?;
        let mut entries = Vec::new();
        let dir = if p.is_empty() {
            self.root.try_clone()?
        } else {
            self.root.open_dir(p)?
        };
        for e in dir.entries()? {
            let e = e?;
            let name = e
                .file_name()
                .into_string()
                .map_err(|_| invalid("Content names must be UTF-8"))?;
            let child = if p.is_empty() {
                name.clone()
            } else {
                format!("{p}/{name}")
            };
            entries.push(Entry {
                name,
                metadata: self.metadata(&child)?,
            });
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }
    fn open(&self, p: &str) -> io::Result<Arc<dyn ContentFile>> {
        if self.metadata(p)?.directory {
            return Err(io::ErrorKind::IsADirectory.into());
        }
        let file = self.root.open(path(p)?)?.into_std();
        let len = file.metadata()?.len();
        Ok(Arc::new(HostFile { file, len }))
    }
}
struct HostFile {
    file: File,
    len: u64,
}
impl ContentFile for HostFile {
    fn len(&self) -> u64 {
        self.len
    }
    fn read_at(&self, offset: u64, b: &mut [u8]) -> io::Result<usize> {
        #[cfg(windows)]
        {
            use std::os::windows::fs::FileExt;
            self.file.seek_read(b, offset)
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileExt;
            self.file.read_at(b, offset)
        }
    }
}
struct Request {
    path: String,
    offset: u64,
    len: usize,
    reply: mpsc::SyncSender<io::Result<Vec<u8>>>,
}
struct ZarWorker {
    tx: Option<mpsc::Sender<Request>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Drop for ZarWorker {
    fn drop(&mut self) {
        self.tx.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
pub struct Zar {
    entries: BTreeMap<String, Metadata>,
    worker: Arc<ZarWorker>,
    pub fingerprint: String,
}
impl Zar {
    pub fn new(path: &Path) -> io::Result<Self> {
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(1);
        }
        let file = options.open(path)?;
        let (tx, rx) = mpsc::channel::<Request>();
        let (ready, result) = mpsc::sync_channel(1);
        // The native archive reader stays on its owning thread. No unsafe Send implementation.
        let thread = std::thread::Builder::new()
            .name("zarchive-reader".into())
            .spawn(move || {
                let setup = (|| -> io::Result<_> {
                    let mut file = file;
                    let mut h = Sha256::new();
                    io::copy(&mut file, &mut h)?;
                    file.rewind()?;
                    let mut archive =
                        zarchive_rs::Archive::new(file).map_err(|e| invalid(e.to_string()))?;
                    archive
                        .verify_integrity()
                        .map_err(|e| invalid(e.to_string()))?;
                    super::preflight(&mut archive).map_err(invalid)?;
                    let mut entries = BTreeMap::new();
                    entries.insert(
                        String::new(),
                        Metadata {
                            directory: true,
                            len: 0,
                            executable: false,
                        },
                    );
                    let mut pending = vec![String::new()];
                    while let Some(dir) = pending.pop() {
                        let children = archive
                            .read_dir(dir.as_bytes())
                            .map_err(|e| invalid(e.to_string()))?
                            .collect::<Result<Vec<_>, _>>()
                            .map_err(|e| invalid(e.to_string()))?;
                        for e in children {
                            let name = e
                                .path_str()
                                .ok_or_else(|| invalid("Non UTF-8 archive path"))?
                                .to_owned();
                            let m = e.metadata();
                            let mut executable = false;
                            if m.is_dir() {
                                pending.push(name.clone());
                            } else {
                                let mut magic = [0; 4];
                                let mut f = archive
                                    .open_file(name.as_bytes())
                                    .map_err(|e| invalid(e.to_string()))?
                                    .ok_or(io::ErrorKind::NotFound)?;
                                if f.read(&mut magic)? == 4 {
                                    executable = magic == *b"\x7fELF" || magic[..2] == *b"MZ";
                                }
                            }
                            entries.insert(
                                name,
                                Metadata {
                                    directory: m.is_dir(),
                                    len: m.len(),
                                    executable,
                                },
                            );
                        }
                    }
                    Ok((archive, entries, format!("{:x}", h.finalize())))
                })();
                let (mut archive, entries, fingerprint) = match setup {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = ready.send(Err(e));
                        return;
                    }
                };
                if ready.send(Ok((entries, fingerprint))).is_err() {
                    return;
                }
                while let Ok(r) = rx.recv() {
                    let answer = (|| {
                        let mut f = archive
                            .open_file(r.path.as_bytes())
                            .map_err(|e| invalid(e.to_string()))?
                            .ok_or(io::ErrorKind::NotFound)?;
                        f.seek(SeekFrom::Start(r.offset))?;
                        let mut bytes = vec![0; r.len];
                        let n = f.read(&mut bytes)?;
                        bytes.truncate(n);
                        Ok(bytes)
                    })();
                    let _ = r.reply.send(answer);
                }
            })?;
        let (entries, fingerprint) = result
            .recv()
            .map_err(|_| io::Error::other("Archive reader stopped"))??;
        Ok(Self {
            entries,
            worker: Arc::new(ZarWorker {
                tx: Some(tx),
                thread: Some(thread),
            }),
            fingerprint,
        })
    }
}
impl ContentSource for Zar {
    fn metadata(&self, name: &str) -> io::Result<Metadata> {
        self.entries
            .get(path(name)?)
            .cloned()
            .ok_or(io::ErrorKind::NotFound.into())
    }
    fn read_dir(&self, name: &str) -> io::Result<Vec<Entry>> {
        if !self.metadata(name)?.directory {
            return Err(io::ErrorKind::NotADirectory.into());
        }
        let prefix = if name.is_empty() {
            String::new()
        } else {
            format!("{name}/")
        };
        Ok(self
            .entries
            .iter()
            .filter_map(|(p, m)| {
                let n = p.strip_prefix(&prefix)?;
                if n.is_empty() || n.contains('/') {
                    None
                } else {
                    Some(Entry {
                        name: n.into(),
                        metadata: m.clone(),
                    })
                }
            })
            .collect())
    }
    fn open(&self, name: &str) -> io::Result<Arc<dyn ContentFile>> {
        let m = self.metadata(name)?;
        if m.directory {
            return Err(io::ErrorKind::IsADirectory.into());
        }
        Ok(Arc::new(ZarFile {
            worker: self.worker.clone(),
            path: name.into(),
            len: m.len,
        }))
    }
}
struct ZarFile {
    worker: Arc<ZarWorker>,
    path: String,
    len: u64,
}
impl ContentFile for ZarFile {
    fn len(&self) -> u64 {
        self.len
    }
    fn read_at(&self, offset: u64, b: &mut [u8]) -> io::Result<usize> {
        if offset >= self.len {
            return Ok(0);
        }
        let (tx, rx) = mpsc::sync_channel(1);
        self.worker
            .tx
            .as_ref()
            .unwrap()
            .send(Request {
                path: self.path.clone(),
                offset,
                len: b.len().min((self.len - offset) as usize),
                reply: tx,
            })
            .map_err(|_| io::Error::other("Archive reader stopped"))?;
        let data = rx
            .recv()
            .map_err(|_| io::Error::other("Archive reader stopped"))??;
        b[..data.len()].copy_from_slice(&data);
        Ok(data.len())
    }
}

#[derive(Default)]
pub struct MemorySource {
    pub files: BTreeMap<String, Arc<[u8]>>,
}
impl ContentSource for MemorySource {
    fn metadata(&self, p: &str) -> io::Result<Metadata> {
        path(p)?;
        if let Some(b) = self.files.get(p) {
            return Ok(Metadata {
                directory: false,
                len: b.len() as u64,
                executable: b.starts_with(b"\x7fELF") || b.starts_with(b"MZ"),
            });
        }
        let prefix = format!("{p}/");
        if p.is_empty() || self.files.keys().any(|k| k.starts_with(&prefix)) {
            return Ok(Metadata {
                directory: true,
                len: 0,
                executable: false,
            });
        }
        Err(io::ErrorKind::NotFound.into())
    }
    fn read_dir(&self, p: &str) -> io::Result<Vec<Entry>> {
        if !self.metadata(p)?.directory {
            return Err(io::ErrorKind::NotADirectory.into());
        }
        let prefix = if p.is_empty() {
            String::new()
        } else {
            format!("{p}/")
        };
        let mut names = std::collections::BTreeSet::new();
        for k in self.files.keys() {
            if let Some(s) = k.strip_prefix(&prefix) {
                names.insert(s.split('/').next().unwrap().to_owned());
            }
        }
        names
            .into_iter()
            .map(|name| {
                let n = format!("{prefix}{name}");
                Ok(Entry {
                    metadata: self.metadata(&n)?,
                    name,
                })
            })
            .collect()
    }
    fn open(&self, p: &str) -> io::Result<Arc<dyn ContentFile>> {
        Ok(Arc::new(MemoryFile(
            self.files
                .get(path(p)?)
                .cloned()
                .ok_or(io::ErrorKind::NotFound)?,
        )))
    }
}
pub struct MemoryFile(pub Arc<[u8]>);
impl ContentFile for MemoryFile {
    fn len(&self) -> u64 {
        self.0.len() as u64
    }
    fn read_at(&self, o: u64, b: &mut [u8]) -> io::Result<usize> {
        let o = o.min(self.len()) as usize;
        let n = b.len().min(self.0.len() - o);
        b[..n].copy_from_slice(&self.0[o..o + n]);
        Ok(n)
    }
}

pub struct Subtree {
    pub source: Arc<dyn ContentSource>,
    pub root: String,
}
impl Subtree {
    fn child(&self, p: &str) -> io::Result<String> {
        path(p)?;
        Ok(if self.root.is_empty() {
            p.into()
        } else if p.is_empty() {
            self.root.clone()
        } else {
            format!("{}/{p}", self.root)
        })
    }
}
impl ContentSource for Subtree {
    fn metadata(&self, p: &str) -> io::Result<Metadata> {
        self.source.metadata(&self.child(p)?)
    }
    fn read_dir(&self, p: &str) -> io::Result<Vec<Entry>> {
        self.source.read_dir(&self.child(p)?)
    }
    fn open(&self, p: &str) -> io::Result<Arc<dyn ContentFile>> {
        self.source.open(&self.child(p)?)
    }
}

pub struct MountedSources {
    pub base: Arc<dyn ContentSource>,
    pub mounts: BTreeMap<String, Arc<dyn ContentSource>>,
}
impl MountedSources {
    fn route(&self, p: &str) -> io::Result<Option<(&Arc<dyn ContentSource>, String)>> {
        path(p)?;
        Ok(self
            .mounts
            .iter()
            .rev()
            .find(|(m, _)| p == **m || p.starts_with(&format!("{m}/")))
            .map(|(m, s)| (s, p[m.len()..].trim_start_matches('/').into())))
    }
}
impl ContentSource for MountedSources {
    fn metadata(&self, p: &str) -> io::Result<Metadata> {
        if let Some((s, p)) = self.route(p)? {
            return s.metadata(&p);
        }
        let prefix = if p.is_empty() {
            String::new()
        } else {
            format!("{p}/")
        };
        if self.mounts.keys().any(|m| m.starts_with(&prefix)) {
            return Ok(Metadata {
                directory: true,
                len: 0,
                executable: false,
            });
        }
        self.base.metadata(p)
    }
    fn read_dir(&self, p: &str) -> io::Result<Vec<Entry>> {
        if let Some((s, p)) = self.route(p)? {
            return s.read_dir(&p);
        }
        if !self.metadata(p)?.directory {
            return Err(io::ErrorKind::NotADirectory.into());
        }
        let mut entries: BTreeMap<String, Metadata> = self
            .base
            .read_dir(p)
            .unwrap_or_default()
            .into_iter()
            .map(|e| (e.name, e.metadata))
            .collect();
        let prefix = if p.is_empty() {
            String::new()
        } else {
            format!("{p}/")
        };
        for key in self.mounts.keys() {
            if let Some(s) = key.strip_prefix(&prefix) {
                let name = s.split('/').next().unwrap();
                let full = format!("{prefix}{name}");
                entries.insert(name.into(), self.metadata(&full)?);
            }
        }
        Ok(entries
            .into_iter()
            .map(|(name, metadata)| Entry { name, metadata })
            .collect())
    }
    fn open(&self, p: &str) -> io::Result<Arc<dyn ContentFile>> {
        if let Some((s, p)) = self.route(p)? {
            s.open(&p)
        } else {
            self.base.open(p)
        }
    }
}

/// ASCII-insensitive guest namespace with preserved directory entry spelling.
pub struct CaseFoldSource(pub Arc<dyn ContentSource>);
impl CaseFoldSource {
    fn resolve(&self, p: &str) -> io::Result<String> {
        path(p)?;
        let mut current = String::new();
        for component in p.split('/').filter(|c| !c.is_empty()) {
            let entries = self.0.read_dir(&current)?;
            let candidates = entries
                .iter()
                .filter(|e| e.name.eq_ignore_ascii_case(component))
                .collect::<Vec<_>>();
            if candidates.len() > 1 {
                return Err(invalid(
                    "Content has ambiguous names in a case-insensitive guest namespace",
                ));
            }
            let name = &candidates.first().ok_or(io::ErrorKind::NotFound)?.name;
            current = if current.is_empty() {
                name.clone()
            } else {
                format!("{current}/{name}")
            };
        }
        Ok(current)
    }
}
impl ContentSource for CaseFoldSource {
    fn metadata(&self, p: &str) -> io::Result<Metadata> {
        self.0.metadata(&self.resolve(p)?)
    }
    fn read_dir(&self, p: &str) -> io::Result<Vec<Entry>> {
        let entries = self.0.read_dir(&self.resolve(p)?)?;
        let mut names = std::collections::BTreeSet::new();
        for e in &entries {
            if !names.insert(e.name.to_ascii_lowercase()) {
                return Err(invalid("Case-insensitive filename collision"));
            }
        }
        Ok(entries)
    }
    fn open(&self, p: &str) -> io::Result<Arc<dyn ContentFile>> {
        self.0.open(&self.resolve(p)?)
    }
}

fn fingerprint<S: ContentSource + ?Sized>(source: &S) -> io::Result<String> {
    let mut hash = Sha256::new();
    let mut stack = vec![(String::new(), 0usize)];
    let mut count = 0usize;
    let mut total = 0u64;
    let mut buffer = vec![0; 65536];
    while let Some((directory, depth)) = stack.pop() {
        if depth > 64 {
            return Err(invalid("Source nesting exceeds 64 directories"));
        }
        let mut entries = source.read_dir(&directory)?;
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        for e in entries {
            count += 1;
            if count > 1_000_000 {
                return Err(invalid("Source contains more than one million entries"));
            }
            let p = if directory.is_empty() {
                e.name
            } else {
                format!("{directory}/{}", e.name)
            };
            path(&p)?;
            hash.update((p.len() as u64).to_le_bytes());
            hash.update(p.as_bytes());
            hash.update([u8::from(e.metadata.directory)]);
            hash.update(e.metadata.len.to_le_bytes());
            if e.metadata.directory {
                stack.push((p, depth + 1))
            } else {
                total = total
                    .checked_add(e.metadata.len)
                    .ok_or_else(|| invalid("Source size overflow"))?;
                if total > super::MAX_UNPACKED_BYTES {
                    return Err(invalid("Source exceeds 2 TiB"));
                }
                let file = source.open(&p)?;
                let mut offset = 0;
                while offset < file.len() {
                    let n = file.read_at(offset, &mut buffer)?;
                    if n == 0 {
                        return Err(io::ErrorKind::UnexpectedEof.into());
                    }
                    hash.update(&buffer[..n]);
                    offset += n as u64;
                }
            }
        }
    }
    Ok(format!("{:x}", hash.finalize()))
}
