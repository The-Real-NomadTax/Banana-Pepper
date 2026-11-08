//! The guest filesystem: a root directory plus binds, with guest path
//! resolution (symlinks resolve inside the guest, never on the host).
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Bind {
    /// Normalized absolute guest path ("/" for the root).
    pub guest: String,
    pub host: PathBuf,
    /// Declaration order: among binds of one host path, the first names it.
    order: usize,
}

#[derive(Clone, Debug)]
pub struct Fs {
    /// Longest guest path first.
    binds: Vec<Bind>,
}

const MAX_LINKS: u32 = 40;

/// `a//b/./c` to `/a/b/c`; `..` stays as a component for the resolver.
fn components(path: &str) -> impl DoubleEndedIterator<Item = &str> {
    path.split('/').filter(|c| !c.is_empty() && *c != ".")
}

/// Normalize an absolute guest path lexically.
pub fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for c in components(path) {
        if c == ".." {
            parts.pop();
        } else {
            parts.push(c);
        }
    }
    format!("/{}", parts.join("/"))
}

/// Join a possibly relative guest path onto an absolute guest base.
pub fn join(base: &str, path: &str) -> String {
    if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("{}/{path}", base.trim_end_matches('/'))
    }
}

impl Fs {
    /// `root` becomes guest `/`; each bind maps a host path to a guest path.
    pub fn new(root: PathBuf, binds: Vec<(PathBuf, String)>) -> Self {
        let mut all = vec![Bind {
            guest: "/".into(),
            host: root,
            order: 0,
        }];
        for (order, (host, guest)) in binds.into_iter().enumerate() {
            let guest = normalize(&guest);
            all.retain(|b| b.guest != guest);
            all.push(Bind {
                guest,
                host,
                order: order + 1,
            });
        }
        all.sort_by_key(|b| std::cmp::Reverse(b.guest.len()));
        Self { binds: all }
    }

    /// Host location of a normalized guest path (no symlink resolution).
    pub fn host_of(&self, guest: &str) -> PathBuf {
        for bind in &self.binds {
            let rest = if bind.guest == "/" {
                guest.strip_prefix('/')
            } else if guest == bind.guest {
                Some("")
            } else {
                guest
                    .strip_prefix(bind.guest.as_str())
                    .and_then(|r| r.strip_prefix('/'))
            };
            if let Some(rest) = rest {
                return if rest.is_empty() {
                    bind.host.clone()
                } else {
                    bind.host.join(rest)
                };
            }
        }
        unreachable!("the root bind matches every path")
    }

    /// Guest path of a host path, when a bind (or the root) contains it.
    pub fn guest_of(&self, host: &Path) -> Option<String> {
        let mut best: Option<(&Bind, usize, String)> = None;
        for bind in &self.binds {
            let Ok(rest) = host.strip_prefix(&bind.host) else {
                continue;
            };
            let depth = bind.host.components().count();
            if best
                .as_ref()
                .is_none_or(|(b, d, _)| depth > *d || (depth == *d && bind.order < b.order))
            {
                let rest = rest.to_string_lossy().into_owned();
                best = Some((bind, depth, rest));
            }
        }
        let (bind, _, rest) = best?;
        Some(normalize(&format!("{}/{rest}", bind.guest)))
    }

    /// Resolve an absolute guest path to its host path, following guest
    /// symlinks (the final one only when `follow`). `/proc/self` names the
    /// tracee, and /proc's magic links are left to the kernel.
    pub fn translate(
        &self,
        guest: &str,
        follow: bool,
        tgid: i32,
        tid: i32,
    ) -> Result<PathBuf, i32> {
        if !guest.starts_with('/') {
            return Err(libc::EINVAL);
        }
        let mut pending: std::collections::VecDeque<String> =
            components(guest).map(str::to_owned).collect();
        let mut current: Vec<String> = Vec::new();
        let mut links = 0;
        while let Some(part) = pending.pop_front() {
            if part == ".." {
                current.pop();
                continue;
            }
            current.push(part);
            if current.len() == 2 && current[0] == "proc" {
                if current[1] == "self" {
                    current[1] = tgid.to_string();
                    continue;
                }
                if current[1] == "thread-self" {
                    current[1] = tgid.to_string();
                    current.extend(["task".to_owned(), tid.to_string()]);
                    continue;
                }
            }
            if pending.is_empty() && !follow {
                break;
            }
            // Below /proc/<pid> the links point at host paths: the kernel
            // resolves those itself.
            if current.len() > 2 && current[0] == "proc" {
                continue;
            }
            let now = format!("/{}", current.join("/"));
            let Ok(target) = std::fs::read_link(self.host_of(&now)) else {
                continue;
            };
            links += 1;
            if links > MAX_LINKS {
                return Err(libc::ELOOP);
            }
            current.pop();
            let target = target.to_string_lossy().into_owned();
            if target.starts_with('/') {
                current.clear();
            }
            for part in components(&target).rev() {
                pending.push_front(part.to_owned());
            }
        }
        let host = self.host_of(&format!("/{}", current.join("/")));
        if host.as_os_str().len() >= libc::PATH_MAX as usize {
            return Err(libc::ENAMETOOLONG);
        }
        Ok(host)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binds_and_guest_symlinks_resolve_inside_the_guest() {
        let temp = std::env::temp_dir().join(format!("banana-tracer-path-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp);
        let root = temp.join("root");
        let tmp = temp.join("tmp");
        std::fs::create_dir_all(root.join("usr/bin")).unwrap();
        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::fs::create_dir_all(&tmp).unwrap();
        std::os::unix::fs::symlink("usr/bin", root.join("bin")).unwrap();
        std::os::unix::fs::symlink("/usr/bin/real", root.join("usr/bin/link")).unwrap();
        std::os::unix::fs::symlink("/etc/a", root.join("etc/b")).unwrap();
        std::os::unix::fs::symlink("/etc/b", root.join("etc/a")).unwrap();
        let fs = Fs::new(
            root.clone(),
            vec![
                (tmp.clone(), "/tmp".into()),
                (tmp.clone(), "/dev/shm".into()),
            ],
        );

        assert_eq!(
            fs.translate("/bin/sh", true, 1, 1).unwrap(),
            root.join("usr/bin/sh")
        );
        assert_eq!(
            fs.translate("/usr/bin/link", true, 1, 1).unwrap(),
            root.join("usr/bin/real")
        );
        assert_eq!(
            fs.translate("/usr/bin/link", false, 1, 1).unwrap(),
            root.join("usr/bin/link")
        );
        assert_eq!(
            fs.translate("/tmp/x/../y", true, 1, 1).unwrap(),
            tmp.join("y")
        );
        assert_eq!(
            fs.translate("/dev/shm/z", true, 1, 1).unwrap(),
            tmp.join("z")
        );
        assert_eq!(
            fs.translate("/../../etc", true, 1, 1).unwrap(),
            root.join("etc")
        );
        assert_eq!(fs.translate("/etc/a", true, 1, 1), Err(libc::ELOOP));
        assert_eq!(
            fs.guest_of(&root.join("usr/lib")).as_deref(),
            Some("/usr/lib")
        );
        assert_eq!(fs.guest_of(&tmp.join("q")).as_deref(), Some("/tmp/q"));
        assert_eq!(fs.guest_of(Path::new("/elsewhere")), None);
        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn proc_self_names_the_tracee() {
        let fs = Fs::new(
            PathBuf::from("/r"),
            vec![(PathBuf::from("/proc"), "/proc".into())],
        );
        assert_eq!(
            fs.translate("/proc/self/maps", true, 42, 43).unwrap(),
            PathBuf::from("/proc/42/maps")
        );
        assert_eq!(
            fs.translate("/proc/thread-self/stat", true, 42, 43)
                .unwrap(),
            PathBuf::from("/proc/42/task/43/stat")
        );
        assert_eq!(
            fs.translate("/proc/self/fd/3", true, 42, 43).unwrap(),
            PathBuf::from("/proc/42/fd/3")
        );
    }
}
