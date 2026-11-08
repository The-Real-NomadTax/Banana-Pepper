//! Debian-format package repositories (Debian, Termux): index parsing,
//! dependency closure, verified `.deb` downloads and their data members.
//! Nothing is installed or run: packages are read as archives and merged into
//! a [`Tree`] that is written out as one tarball or sysroot.
use crate::util::{Context, Result, download, get};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq)]
pub enum MultiArch {
    No,
    Same,
    Foreign,
    Allowed,
}

#[derive(Clone, Debug)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub arch: String,
    pub multi_arch: MultiArch,
    /// Conjunction of alternatives: Pre-Depends and Depends.
    pub depends: Vec<Vec<Dependency>>,
    pub provides: Vec<String>,
    pub filename: String,
    pub sha256: String,
    pub essential: bool,
    /// 0 required, 1 important, 2 standard, 3 optional or unknown.
    pub priority: u8,
    /// Index of the repository this package came from.
    pub repo: usize,
}

#[derive(Clone, Debug)]
pub struct Dependency {
    pub name: String,
    /// `any`, `native` or a concrete architecture.
    pub arch: Option<String>,
}

/// Parse a `Packages` index.
pub fn parse(text: &str, repo: usize) -> Vec<Package> {
    let mut packages = Vec::new();
    for stanza in text.split("\n\n") {
        let mut fields: HashMap<&str, String> = HashMap::new();
        let mut last = None;
        for line in stanza.lines() {
            if line.starts_with([' ', '\t']) {
                if let Some(key) = last {
                    let value: &mut String = fields.entry(key).or_default();
                    value.push(' ');
                    value.push_str(line.trim());
                }
            } else if let Some((key, value)) = line.split_once(':') {
                fields.insert(key, value.trim().to_owned());
                last = Some(key);
            }
        }
        let (Some(name), Some(filename)) = (fields.get("Package"), fields.get("Filename")) else {
            continue;
        };
        let field = |k: &str| fields.get(k).cloned().unwrap_or_default();
        let mut depends = relations(&field("Pre-Depends"));
        depends.extend(relations(&field("Depends")));
        packages.push(Package {
            name: name.clone(),
            version: field("Version"),
            arch: field("Architecture"),
            multi_arch: match field("Multi-Arch").as_str() {
                "same" => MultiArch::Same,
                "foreign" => MultiArch::Foreign,
                "allowed" => MultiArch::Allowed,
                _ => MultiArch::No,
            },
            depends,
            provides: relations(&field("Provides"))
                .into_iter()
                .flatten()
                .map(|d| d.name)
                .collect(),
            filename: filename.clone(),
            sha256: field("SHA256"),
            essential: field("Essential") == "yes",
            priority: match field("Priority").as_str() {
                "required" => 0,
                "important" => 1,
                "standard" => 2,
                _ => 3,
            },
            repo,
        });
    }
    packages
}

/// `a (>= 1) | b:any, c [amd64] <!nocheck>` into alternatives; versions,
/// architecture restrictions and build profiles are dropped.
fn relations(field: &str) -> Vec<Vec<Dependency>> {
    field
        .split(',')
        .map(|group| {
            group
                .split('|')
                .filter_map(|alternative| {
                    let word = alternative.split(['(', '[', '<']).next()?.trim();
                    if word.is_empty() {
                        return None;
                    }
                    let (name, arch) = match word.split_once(':') {
                        Some((n, a)) => (n, Some(a.to_owned())),
                        None => (word, None),
                    };
                    Some(Dependency {
                        name: name.to_owned(),
                        arch,
                    })
                })
                .collect::<Vec<_>>()
        })
        .filter(|g| !g.is_empty())
        .collect()
}

/// A repository: its mirror and the index files it contributes.
pub struct Repository {
    pub mirror: String,
}

/// Every package known from the loaded indexes.
pub struct Universe {
    pub repositories: Vec<Repository>,
    pub packages: Vec<Package>,
    pub native: String,
    by_name: HashMap<String, Vec<usize>>,
    providers: HashMap<String, Vec<usize>>,
}

impl Universe {
    pub fn new(native: &str) -> Self {
        Self {
            repositories: Vec::new(),
            packages: Vec::new(),
            native: native.to_owned(),
            by_name: HashMap::new(),
            providers: HashMap::new(),
        }
    }

    /// Load `<mirror>/<index>` (`.xz` and `.gz` are decompressed).
    pub fn load(&mut self, mirror: &str, index: &str) -> Result<()> {
        let url = format!("{mirror}/{index}");
        println!("Reading {url}");
        let bytes = decompress(index, get(&url)?)?;
        let repo = match self.repositories.iter().position(|r| r.mirror == mirror) {
            Some(i) => i,
            None => {
                self.repositories.push(Repository {
                    mirror: mirror.to_owned(),
                });
                self.repositories.len() - 1
            }
        };
        let text = String::from_utf8_lossy(&bytes);
        for package in parse(&text, repo) {
            // Architecture-independent packages appear in every index.
            if self.lookup(&package.name, &package.arch).is_some() {
                continue;
            }
            let index = self.packages.len();
            self.by_name.entry(package.name.clone()).or_default().push(index);
            for virtual_name in &package.provides {
                self.providers.entry(virtual_name.clone()).or_default().push(index);
            }
            self.packages.push(package);
        }
        Ok(())
    }

    pub fn lookup(&self, name: &str, arch: &str) -> Option<usize> {
        self.by_name
            .get(name)?
            .iter()
            .copied()
            .find(|&i| self.packages[i].arch == arch)
    }

    /// The effective architecture a package's own dependencies resolve in.
    fn arch_of(&self, package: &Package) -> String {
        if package.arch == "all" {
            self.native.clone()
        } else {
            package.arch.clone()
        }
    }

    /// Whether `candidate` satisfies `dependency` for a package of `arch`.
    fn satisfies(&self, candidate: &Package, dependency: &Dependency, arch: &str) -> bool {
        match dependency.arch.as_deref() {
            Some("any") => {
                candidate.multi_arch == MultiArch::Allowed
                    || candidate.multi_arch == MultiArch::Foreign
                    || candidate.arch == arch
                    || candidate.arch == "all"
            }
            Some("native") => candidate.arch == self.native || candidate.arch == "all",
            Some(specific) => candidate.arch == specific,
            None => {
                candidate.arch == arch
                    || candidate.arch == "all"
                    || candidate.multi_arch == MultiArch::Foreign
            }
        }
    }

    /// Candidates for one alternative: real packages first, then providers;
    /// within each, the requesting architecture before foreign ones.
    fn candidates(&self, dependency: &Dependency, arch: &str) -> Vec<usize> {
        let rank = |&i: &usize| {
            let p = &self.packages[i];
            u8::from(p.arch != arch && p.arch != "all")
        };
        let mut real: Vec<usize> = self
            .by_name
            .get(&dependency.name)
            .into_iter()
            .flatten()
            .copied()
            .filter(|&i| self.satisfies(&self.packages[i], dependency, arch))
            .collect();
        real.sort_by_key(rank);
        let mut provided: Vec<usize> = self
            .providers
            .get(&dependency.name)
            .into_iter()
            .flatten()
            .copied()
            .filter(|&i| self.satisfies(&self.packages[i], dependency, arch))
            .collect();
        // Like apt: the requesting architecture, then the higher priority.
        provided.sort_by_key(|i| {
            let p = &self.packages[*i];
            (rank(i), p.priority, p.name.clone())
        });
        real.extend(provided);
        real
    }

    /// The dependency closure of `roots` (`name` or `name:arch`), Depends
    /// and Pre-Depends only, as apt's `--no-install-recommends` installs it.
    pub fn resolve(&self, roots: &[String]) -> Result<Vec<usize>> {
        let mut selected = BTreeSet::new();
        let mut order = Vec::new();
        let mut pending = Vec::new();
        for root in roots {
            let (name, arch) = root.split_once(':').unwrap_or((root, &self.native));
            let index = self
                .lookup(name, arch)
                .or_else(|| self.lookup(name, "all"))
                .ok_or_else(|| format!("No package {root} in the loaded indexes"))?;
            if selected.insert(index) {
                order.push(index);
                pending.push(index);
            }
        }
        // A package counts as selected once chosen, so later alternatives
        // prefer it over a second provider.
        while let Some(index) = pending.pop() {
            let package = &self.packages[index];
            let arch = self.arch_of(package);
            for group in &package.depends {
                let options: Vec<Vec<usize>> =
                    group.iter().map(|d| self.candidates(d, &arch)).collect();
                if options.iter().flatten().any(|i| selected.contains(i)) {
                    continue;
                }
                let choice = options.iter().flatten().next().copied().ok_or_else(|| {
                    format!(
                        "{} {} depends on {}, which no loaded package provides",
                        package.name,
                        package.arch,
                        group
                            .iter()
                            .map(|d| d.name.as_str())
                            .collect::<Vec<_>>()
                            .join(" | ")
                    )
                })?;
                selected.insert(choice);
                order.push(choice);
                pending.push(choice);
            }
        }
        Ok(order)
    }

    /// Architecture-native packages marked Essential.
    pub fn essential(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .packages
            .iter()
            .filter(|p| p.essential && (p.arch == self.native || p.arch == "all"))
            .map(|p| p.name.clone())
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// Download (or reuse) a package, verified against its index SHA-256.
    pub fn fetch(&self, index: usize, cache: &Path) -> Result<PathBuf> {
        let package = &self.packages[index];
        let name = package
            .filename
            .rsplit('/')
            .next()
            .ok_or("Package without a file name")?;
        let path = cache.join(name);
        if package.sha256.len() != 64 {
            return Err(format!("{} has no SHA-256 in its index", package.name));
        }
        let mirror = &self.repositories[package.repo].mirror;
        download(
            &format!("{mirror}/{}", package.filename),
            &path,
            &package.sha256,
        )?;
        Ok(path)
    }
}

/// Decompress by the name's suffix (`.xz`, `.gz`, `.zst`, or plain).
pub fn decompress(name: &str, bytes: Vec<u8>) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    if name.ends_with(".xz") {
        lzma_rs::xz_decompress(&mut std::io::Cursor::new(bytes), &mut out).ctx(name)?;
    } else if name.ends_with(".gz") {
        flate2::read::GzDecoder::new(&bytes[..])
            .read_to_end(&mut out)
            .ctx(name)?;
    } else if name.ends_with(".zst") {
        ruzstd::decoding::StreamingDecoder::new(&bytes[..])
            .ctx(name)?
            .read_to_end(&mut out)
            .ctx(name)?;
    } else {
        return Ok(bytes);
    }
    Ok(out)
}

/// Members of a Unix `ar` archive (a `.deb`).
pub fn ar_members(bytes: &[u8]) -> Result<Vec<(String, &[u8])>> {
    let mut rest = bytes
        .strip_prefix(b"!<arch>\n")
        .ok_or("Not an ar archive")?;
    let mut members = Vec::new();
    while rest.len() >= 60 {
        let header = &rest[..60];
        let name = String::from_utf8_lossy(&header[..16])
            .trim_end()
            .trim_end_matches('/')
            .to_owned();
        let size: usize = String::from_utf8_lossy(&header[48..58])
            .trim()
            .parse()
            .ctx("ar member size")?;
        let body = rest.get(60..60 + size).ok_or("Truncated ar archive")?;
        members.push((name, body));
        rest = &rest[(60 + size + size % 2).min(rest.len())..];
    }
    Ok(members)
}

/// The decompressed `data.tar` of a `.deb`.
pub fn data_tar(deb: &Path) -> Result<Vec<u8>> {
    let bytes = std::fs::read(deb).ctx(deb.display())?;
    let (name, body) = ar_members(&bytes)?
        .into_iter()
        .find(|(n, _)| n.starts_with("data.tar"))
        .ok_or_else(|| format!("{} has no data member", deb.display()))?;
    decompress(&name, body.to_vec())
}

#[derive(Clone, Debug)]
pub enum Node {
    Directory,
    File(std::sync::Arc<Vec<u8>>),
    Symlink(String),
    /// A hard link to another path of the tree.
    Hardlink(String),
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub node: Node,
    pub mode: u32,
    pub mtime: u64,
}

/// A filesystem image built in memory, keyed by normalized relative path.
#[derive(Default)]
pub struct Tree {
    pub entries: BTreeMap<String, Entry>,
}

/// `./usr//lib/` to `usr/lib`; rejects absolute and parent components.
pub fn normalize(path: &str) -> Option<String> {
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => return None,
            p => parts.push(p),
        }
    }
    Some(parts.join("/"))
}

impl Tree {
    /// Merge a `data.tar`. `map` rewrites (or, with `None`, drops) each path.
    pub fn merge_tar(&mut self, tar: &[u8], map: &dyn Fn(&str) -> Option<String>) -> Result<()> {
        let mut archive = tar::Archive::new(tar);
        for entry in archive.entries().ctx("package data")? {
            let mut entry = entry.ctx("package data")?;
            let raw = entry.path().ctx("package path")?.to_string_lossy().into_owned();
            let Some(path) = normalize(&raw).and_then(|p| map(&p)) else {
                continue;
            };
            if path.is_empty() {
                continue;
            }
            let header = entry.header();
            let mode = header.mode().unwrap_or(0o644) & 0o7777;
            let mtime = header.mtime().unwrap_or(0);
            let node = match header.entry_type() {
                tar::EntryType::Directory => Node::Directory,
                tar::EntryType::Symlink => Node::Symlink(
                    entry
                        .link_name()
                        .ctx("symlink")?
                        .ok_or("Symlink without a target")?
                        .to_string_lossy()
                        .into_owned(),
                ),
                tar::EntryType::Link => {
                    let target = entry
                        .link_name()
                        .ctx("hard link")?
                        .ok_or("Hard link without a target")?
                        .to_string_lossy()
                        .into_owned();
                    match normalize(&target).and_then(|t| map(&t)) {
                        Some(t) => Node::Hardlink(t),
                        None => continue,
                    }
                }
                tar::EntryType::Regular | tar::EntryType::Continuous => {
                    let mut bytes = Vec::with_capacity(entry.size() as usize);
                    entry.read_to_end(&mut bytes).ctx(&path)?;
                    Node::File(std::sync::Arc::new(bytes))
                }
                _ => continue,
            };
            // A directory never replaces what another package put there.
            if matches!(node, Node::Directory) && self.entries.contains_key(&path) {
                continue;
            }
            self.entries.insert(path, Entry { node, mode, mtime });
        }
        Ok(())
    }

    pub fn insert(&mut self, path: &str, node: Node, mode: u32) {
        self.entries.insert(
            normalize(path).expect("relative path"),
            Entry {
                node,
                mode,
                mtime: 0,
            },
        );
    }

    pub fn file(&self, path: &str) -> Option<&[u8]> {
        match &self.entries.get(path)?.node {
            Node::File(bytes) => Some(bytes),
            _ => None,
        }
    }

    /// Every directory that holds an entry gets an entry of its own.
    pub fn add_parents(&mut self) {
        let mut missing = BTreeSet::new();
        for path in self.entries.keys() {
            let mut current = path.as_str();
            while let Some((parent, _)) = current.rsplit_once('/') {
                if !self.entries.contains_key(parent) {
                    missing.insert(parent.to_owned());
                }
                current = parent;
            }
        }
        for path in missing {
            self.entries.insert(
                path,
                Entry {
                    node: Node::Directory,
                    mode: 0o755,
                    mtime: 0,
                },
            );
        }
    }

    /// Write the tree as a gzip tarball, numeric root ownership, hard links
    /// after every regular file so an extractor always finds their targets.
    pub fn write_tar_gz(&self, path: &Path) -> Result<()> {
        let partial = path.with_extension("building");
        let file = std::fs::File::create(&partial).ctx(partial.display())?;
        let encoder = flate2::write::GzEncoder::new(
            std::io::BufWriter::new(file),
            flate2::Compression::new(6),
        );
        let mut builder = tar::Builder::new(encoder);
        let header = |entry: &Entry, kind: tar::EntryType, size: u64| {
            let mut h = tar::Header::new_gnu();
            h.set_entry_type(kind);
            h.set_mode(entry.mode);
            h.set_mtime(entry.mtime);
            h.set_uid(0);
            h.set_gid(0);
            h.set_size(size);
            h
        };
        let mut links = Vec::new();
        for (name, entry) in &self.entries {
            let name = format!("./{name}");
            match &entry.node {
                Node::Directory => {
                    let mut h = header(entry, tar::EntryType::Directory, 0);
                    builder
                        .append_data(&mut h, format!("{name}/"), std::io::empty())
                        .ctx(&name)?;
                }
                Node::File(bytes) => {
                    let mut h = header(entry, tar::EntryType::Regular, bytes.len() as u64);
                    builder.append_data(&mut h, &name, &bytes[..]).ctx(&name)?;
                }
                Node::Symlink(target) => {
                    let mut h = header(entry, tar::EntryType::Symlink, 0);
                    builder.append_link(&mut h, &name, target).ctx(&name)?;
                }
                Node::Hardlink(target) => {
                    if !matches!(
                        self.entries.get(target).map(|e| &e.node),
                        Some(Node::File(_))
                    ) {
                        return Err(format!("{name} links to missing {target}"));
                    }
                    links.push((name, entry, format!("./{target}")));
                }
            }
        }
        for (name, entry, target) in links {
            let mut h = header(entry, tar::EntryType::Link, 0);
            builder.append_link(&mut h, &name, &target).ctx(&name)?;
        }
        builder
            .into_inner()
            .ctx("tar")?
            .finish()
            .ctx("gzip")?
            .into_inner()
            .map_err(|e| e.to_string())?;
        std::fs::rename(&partial, path).ctx(path.display())
    }

    /// Materialize the tree as a directory for build tools (a sysroot).
    /// Links become copies of what they point to, so the result needs no
    /// symlink support from the host filesystem.
    pub fn write_dir(&self, root: &Path) -> Result<()> {
        for (name, entry) in &self.entries {
            let target = root.join(name);
            match &entry.node {
                Node::Directory => std::fs::create_dir_all(&target).ctx(target.display())?,
                _ => {
                    let Some(bytes) = self.resolve(name) else {
                        continue; // dangling links are harmless in a sysroot
                    };
                    if let Some(parent) = target.parent() {
                        std::fs::create_dir_all(parent).ctx(parent.display())?;
                    }
                    std::fs::write(&target, bytes).ctx(target.display())?;
                }
            }
        }
        Ok(())
    }

    /// A file's bytes, following hard and symbolic links within the tree.
    pub fn resolve(&self, path: &str) -> Option<&[u8]> {
        self.resolve_at(path, 0)
    }

    fn resolve_at(&self, path: &str, depth: u32) -> Option<&[u8]> {
        if depth > 16 {
            return None;
        }
        match &self.entries.get(path)?.node {
            Node::File(bytes) => Some(bytes),
            Node::Hardlink(t) => self.resolve_at(t, depth + 1),
            Node::Symlink(t) => {
                let joined = if let Some(absolute) = t.strip_prefix('/') {
                    absolute.to_owned()
                } else {
                    let parent = path.rsplit_once('/').map_or("", |(p, _)| p);
                    let mut parts: Vec<&str> = parent.split('/').filter(|p| !p.is_empty()).collect();
                    for part in t.split('/') {
                        match part {
                            "" | "." => {}
                            ".." => {
                                parts.pop();
                            }
                            p => parts.push(p),
                        }
                    }
                    parts.join("/")
                };
                self.resolve_at(&joined, depth + 1)
            }
            Node::Directory => None,
        }
    }
}

/// Map merged-/usr aliases (`bin/`, `sbin/`, `lib/`, `libNN/`) into `usr/`,
/// as dpkg does by following the aliasing symlinks. The aliases themselves
/// are dropped; the caller provides them.
pub fn merged_usr(path: &str) -> Option<String> {
    const ALIASES: &[&str] = &["bin", "sbin", "lib", "lib32", "lib64", "libx32"];
    let (first, rest) = path.split_once('/').unwrap_or((path, ""));
    if ALIASES.contains(&first) {
        if rest.is_empty() {
            return None;
        }
        return Some(format!("usr/{first}/{rest}"));
    }
    Some(path.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const INDEX: &str = "Package: app\nArchitecture: arm64\nVersion: 1\nDepends: libfoo (>= 1) | libbar, tool:any, virt\nFilename: pool/a/app.deb\nSHA256: aa\n\n\
Package: libfoo\nArchitecture: arm64\nMulti-Arch: same\nVersion: 1\nFilename: pool/l/libfoo.deb\nSHA256: bb\n\n\
Package: libfoo\nArchitecture: i386\nMulti-Arch: same\nVersion: 1\nFilename: pool/l/libfoo-i386.deb\nSHA256: cc\n\n\
Package: tool\nArchitecture: i386\nMulti-Arch: allowed\nVersion: 1\nFilename: pool/t/tool.deb\nSHA256: dd\n\n\
Package: impl\nArchitecture: all\nProvides: virt (= 2)\nVersion: 1\nFilename: pool/i/impl.deb\nSHA256: ee\nEssential: yes\n\n\
Package: game\nArchitecture: i386\nVersion: 1\nDepends: libfoo\nFilename: pool/g/game.deb\nSHA256: ff\n";

    fn universe() -> Universe {
        let mut u = Universe::new("arm64");
        u.repositories.push(Repository { mirror: "m".into() });
        for p in parse(INDEX, 0) {
            let i = u.packages.len();
            u.by_name.entry(p.name.clone()).or_default().push(i);
            for v in &p.provides {
                u.providers.entry(v.clone()).or_default().push(i);
            }
            u.packages.push(p);
        }
        u
    }

    fn names(u: &Universe, picked: &[usize]) -> BTreeSet<String> {
        picked
            .iter()
            .map(|&i| format!("{}:{}", u.packages[i].name, u.packages[i].arch))
            .collect()
    }

    #[test]
    fn closure_follows_alternatives_multiarch_and_provides() {
        let u = universe();
        let picked = u.resolve(&["app".into(), "game:i386".into()]).unwrap();
        assert_eq!(
            names(&u, &picked),
            ["app:arm64", "libfoo:arm64", "libfoo:i386", "tool:i386", "impl:all", "game:i386"]
                .map(String::from)
                .into()
        );
        assert_eq!(u.essential(), ["impl"]);
        assert!(u.resolve(&["missing".into()]).is_err());
    }

    #[test]
    fn merged_usr_aliases_map_into_usr() {
        assert_eq!(merged_usr("lib/ld-linux.so.2").as_deref(), Some("usr/lib/ld-linux.so.2"));
        assert_eq!(merged_usr("bin").as_deref(), None);
        assert_eq!(merged_usr("etc/hosts").as_deref(), Some("etc/hosts"));
        assert_eq!(normalize("./usr//lib/"), Some("usr/lib".into()));
        assert_eq!(normalize("../x"), None);
    }

    #[test]
    fn ar_members_are_read() {
        let mut ar = b"!<arch>\n".to_vec();
        for (name, body) in [("debian-binary", &b"2.0\n"[..]), ("data.tar", b"abc")] {
            ar.extend(format!("{name:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n", 0, 0, 0, 644, body.len()).bytes());
            ar.extend(body);
            if body.len() % 2 == 1 {
                ar.push(b'\n');
            }
        }
        let members = ar_members(&ar).unwrap();
        assert_eq!(members[1], ("data.tar".into(), &b"abc"[..]));
    }

    #[test]
    fn symlinks_resolve_for_sysroots() {
        let mut t = Tree::default();
        t.insert("usr/lib/libm.so.6", Node::File(std::sync::Arc::new(b"m".to_vec())), 0o644);
        t.insert("usr/lib/libm.so", Node::Symlink("libm.so.6".into()), 0o777);
        t.insert("usr/lib/x/libm.so", Node::Symlink("../libm.so".into()), 0o777);
        t.insert("lib/abs", Node::Symlink("/usr/lib/libm.so".into()), 0o777);
        for path in ["usr/lib/libm.so", "usr/lib/x/libm.so", "lib/abs"] {
            assert_eq!(t.resolve(path), Some(&b"m"[..]), "{path}");
        }
    }
}
