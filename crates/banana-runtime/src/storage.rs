//! Per-user durable state and stable library entry identities.
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
};
#[derive(Clone, Debug)]
pub struct Paths {
    pub config: PathBuf,
    pub data: PathBuf,
    pub cache: PathBuf,
    pub state: PathBuf,
}
impl Paths {
    pub fn discover() -> Result<Self, String> {
        #[cfg(windows)]
        {
            let data = std::env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .ok_or("LOCALAPPDATA is not set")?
                .join("BananaPepper");
            let config = std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .ok_or("APPDATA is not set")?
                .join("banana-pepper");
            Ok(Self {
                config,
                data: data.join("data"),
                cache: data.join("cache"),
                state: data.join("state"),
            })
        }
        #[cfg(not(windows))]
        {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .ok_or("HOME is not set")?;
            let xdg = |key: &str, default: &str| {
                std::env::var_os(key)
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home.join(default))
                    .join("banana-pepper")
            };
            Ok(Self {
                config: xdg("XDG_CONFIG_HOME", ".config"),
                data: xdg("XDG_DATA_HOME", ".local/share"),
                cache: xdg("XDG_CACHE_HOME", ".cache"),
                state: xdg("XDG_STATE_HOME", ".local/state"),
            })
        }
    }
    /// The source's content fingerprint, hashed only when its stamp (names,
    /// sizes, modification times) changed since the last launch from this
    /// location. Archive integrity is verified together with each fresh hash.
    pub fn fingerprint(
        &self,
        location: &Path,
        source: &dyn banana_content::ContentSource,
    ) -> Result<String, String> {
        use sha2::{Digest, Sha256};
        let stamp = source.stamp("").map_err(|e| e.to_string())?;
        let cached = stamp.as_ref().map(|stamp| {
            let mut key = Sha256::new();
            key.update(location.to_string_lossy().as_bytes());
            key.update([0]);
            key.update(stamp.as_bytes());
            self.cache
                .join("fingerprints")
                .join(format!("{:x}", key.finalize()))
        });
        if let Some(text) = cached.as_ref().and_then(|p| fs::read_to_string(p).ok()) {
            let text = text.trim();
            if text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Ok(text.to_owned());
            }
        }
        source.verify().map_err(|e| e.to_string())?;
        let fingerprint = source.fingerprint().map_err(|e| e.to_string())?;
        // Only remember it if nothing changed while hashing.
        if let Some(path) = cached
            && source.stamp("").map_err(|e| e.to_string())? == stamp
        {
            fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
            let temp = path.with_extension("pending");
            fs::write(&temp, &fingerprint)
                .and_then(|()| fs::rename(&temp, &path))
                .map_err(|e| e.to_string())?;
        }
        Ok(fingerprint)
    }
    pub fn initialize(&self, resources: &Path) -> Result<(), String> {
        for p in [&self.config, &self.data, &self.cache, &self.state] {
            fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        for name in ["settings.cfg", "controls.cfg"] {
            let target = self.config.join(name);
            if !target.exists() {
                let old = resources.join(name);
                if old.is_file() {
                    let bytes = fs::read(old).map_err(|e| e.to_string())?;
                    let mut f = fs::OpenOptions::new()
                        .create_new(true)
                        .write(true)
                        .open(&target)
                        .map_err(|e| e.to_string())?;
                    use std::io::Write;
                    f.write_all(&bytes).map_err(|e| e.to_string())?;
                    f.sync_all().map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(())
    }
    pub fn entry(&self, game: &str, source: &Path, fingerprint: &str) -> Result<String, String> {
        let path = self.data.join("entries.json");
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.data.join("entries.lock"))
            .map_err(|e| e.to_string())?;
        lock.lock().map_err(|e| e.to_string())?;
        let mut entries: Vec<Entry> = match fs::read(&path) {
            Ok(b) => serde_json::from_slice(&b).map_err(|e| e.to_string())?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => vec![],
            Err(e) => return Err(e.to_string()),
        };
        let source = source.canonicalize().map_err(|e| e.to_string())?;
        let exact = entries
            .iter()
            .position(|e| e.game == game && e.source == source);
        let moved = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.game == game && e.fingerprint == fingerprint && !e.source.exists())
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        let index = if let Some(i) = exact {
            i
        } else if moved.len() == 1 {
            moved[0]
        } else {
            let id = format!(
                "{:x}-{:x}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                std::process::id()
            );
            entries.push(Entry {
                id,
                game: game.into(),
                source: source.clone(),
                fingerprint: fingerprint.into(),
            });
            entries.len() - 1
        };
        if entries[index].fingerprint != fingerprint
            && !self
                .data
                .join("overlays")
                .join(&entries[index].id)
                .join("index.json")
                .exists()
        {
            entries[index].fingerprint = fingerprint.into();
        }
        if entries[index].fingerprint != fingerprint {
            return Err("The library entry's build changed. Its persistent overlay has been retained; reset or migrate it before using this build.".into());
        }
        entries[index].source = source;
        let id = entries[index].id.clone();
        let temp = path.with_extension("pending");
        fs::write(
            &temp,
            serde_json::to_vec_pretty(&entries).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        fs::rename(temp, path).map_err(|e| e.to_string())?;
        Ok(id)
    }
}
#[derive(Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub game: String,
    pub source: PathBuf,
    pub fingerprint: String,
}

pub fn entries(paths: &Paths) -> Result<Vec<Entry>, String> {
    match fs::read(paths.data.join("entries.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(vec![]),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use banana_content::{ContentFile, ContentSource, Entry, Metadata};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    /// Counts file opens to prove a cached fingerprint reads no content.
    struct Counting(Arc<dyn ContentSource>, AtomicUsize);
    impl ContentSource for Counting {
        fn metadata(&self, p: &str) -> io::Result<Metadata> {
            self.0.metadata(p)
        }
        fn read_dir(&self, p: &str) -> io::Result<Vec<Entry>> {
            self.0.read_dir(p)
        }
        fn open(&self, p: &str) -> io::Result<Arc<dyn ContentFile>> {
            self.1.fetch_add(1, Ordering::Relaxed);
            self.0.open(p)
        }
        fn stamp(&self, root: &str) -> io::Result<Option<String>> {
            self.0.stamp(root)
        }
    }

    #[test]
    fn fingerprints_are_cached_until_the_source_changes() {
        let t = tempfile::tempdir().unwrap();
        let paths = Paths {
            config: t.path().join("config"),
            data: t.path().join("data"),
            state: t.path().join("state"),
            cache: t.path().join("cache"),
        };
        let game = t.path().join("game");
        fs::create_dir_all(game.join("data")).unwrap();
        fs::write(game.join("game.exe"), b"MZ...").unwrap();
        fs::write(game.join("data/level.bin"), b"level one").unwrap();
        let open = |game: &Path| {
            Counting(
                Arc::new(banana_content::Folder::new(game).unwrap()),
                AtomicUsize::new(0),
            )
        };
        let first = open(&game);
        let expected = first.fingerprint().unwrap();
        let reads = first.1.load(Ordering::Relaxed);
        assert!(reads > 0);
        assert_eq!(paths.fingerprint(&game, &first).unwrap(), expected);
        let second = open(&game);
        assert_eq!(paths.fingerprint(&game, &second).unwrap(), expected);
        assert_eq!(second.1.load(Ordering::Relaxed), 0, "cached launch read content");

        fs::write(game.join("data/level.bin"), b"level two, longer").unwrap();
        let changed = open(&game);
        let fresh = paths.fingerprint(&game, &changed).unwrap();
        assert_ne!(fresh, expected);
        assert_eq!(fresh, open(&game).fingerprint().unwrap());
        assert!(changed.1.load(Ordering::Relaxed) > 0);

        // Sources without a stamp are always hashed.
        let memory = banana_content::MemorySource::default();
        assert_eq!(
            paths.fingerprint(&game, &memory).unwrap(),
            memory.fingerprint().unwrap()
        );
    }

    #[test]
    fn entry_move_and_config_import_are_idempotent() {
        let t = tempfile::tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
            .unwrap();
        let paths = Paths {
            config: t.path().join("config"),
            data: t.path().join("data"),
            state: t.path().join("state"),
            cache: t.path().join("cache"),
        };
        let old = t.path().join("old");
        fs::create_dir(&old).unwrap();
        fs::write(old.join("settings.cfg"), "backend = wsl\n").unwrap();
        paths.initialize(&old).unwrap();
        fs::write(paths.config.join("settings.cfg"), "user settings").unwrap();
        paths.initialize(&old).unwrap();
        assert_eq!(
            fs::read_to_string(paths.config.join("settings.cfg")).unwrap(),
            "user settings"
        );
        let source = t.path().join("source");
        fs::create_dir(&source).unwrap();
        let id = paths.entry("test", &source, "fingerprint1").unwrap();
        let moved = t.path().join("moved");
        fs::rename(&source, &moved).unwrap();
        assert_eq!(paths.entry("test", &moved, "fingerprint1").unwrap(), id);
        fs::create_dir_all(paths.data.join("overlays").join(&id)).unwrap();
        fs::write(
            paths.data.join("overlays").join(&id).join("index.json"),
            "overlay exists",
        )
        .unwrap();
        assert!(paths.entry("test", &moved, "fingerprint2").is_err());
        let copy = t.path().join("copy");
        fs::create_dir(&copy).unwrap();
        assert_ne!(paths.entry("test", &copy, "fingerprint1").unwrap(), id);
        assert!(fs::read(old.join("settings.cfg")).is_ok());
    }
}

pub fn valid_identity(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}
