//! Shared helpers: repository paths, checked commands and pinned downloads.
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
};

pub type Result<T> = std::result::Result<T, String>;

/// Attach context to any displayable error.
pub trait Context<T> {
    fn ctx(self, what: impl std::fmt::Display) -> Result<T>;
}
impl<T, E: std::fmt::Display> Context<T> for std::result::Result<T, E> {
    fn ctx(self, what: impl std::fmt::Display) -> Result<T> {
        self.map_err(|e| format!("{what}: {e}"))
    }
}

/// The workspace root.
pub fn root() -> PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root");
    // External tools (git, CMake, Java) do not accept Windows verbatim paths.
    match path.to_str().and_then(|p| p.strip_prefix(r"\\?\")) {
        Some(plain) if !plain.starts_with("UNC") => PathBuf::from(plain),
        _ => path,
    }
}

/// Android build inputs and outputs: `apps/android/build` (never committed),
/// or `BANANA_ANDROID_BUILD`.
pub fn android_build() -> PathBuf {
    std::env::var_os("BANANA_ANDROID_BUILD")
        .map(PathBuf::from)
        .unwrap_or_else(|| root().join("apps/android/build"))
}

/// Downloads kept between runs.
pub fn downloads() -> Result<PathBuf> {
    let path = android_build().join("downloads");
    fs::create_dir_all(&path).ctx(path.display())?;
    Ok(path)
}

/// Run a command, failing on a non-zero exit.
pub fn run(command: &mut Command) -> Result<()> {
    let status = command
        .status()
        .ctx(format!("Cannot start {:?}", command.get_program()))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{:?} failed with {status}", command.get_program()))
    }
}

/// Run a command and return its standard output.
pub fn output(command: &mut Command) -> Result<String> {
    let out = command
        .output()
        .ctx(format!("Cannot start {:?}", command.get_program()))?;
    if !out.status.success() {
        return Err(format!(
            "{:?} failed with {}: {}",
            command.get_program(),
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path).ctx(path.display())?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buffer).ctx(path.display())?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

/// Fetch `url` into memory (indexes and other small files).
pub fn get(url: &str) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    ureq::get(url)
        .call()
        .ctx(url)?
        .into_body()
        .into_reader()
        .read_to_end(&mut bytes)
        .ctx(url)?;
    Ok(bytes)
}

/// Download `url` to `path` unless a file with the expected SHA-256 is
/// already there. A mismatch is an error, never a silent replacement.
pub fn download(url: &str, path: &Path, sha256: &str) -> Result<()> {
    if path.is_file() {
        if sha256_file(path)? == sha256 {
            return Ok(());
        }
        fs::remove_file(path).ctx(path.display())?;
    }
    println!("Downloading {url}");
    let partial = path.with_extension("downloading");
    {
        let mut reader = ureq::get(url).call().ctx(url)?.into_body().into_reader();
        let mut file = fs::File::create(&partial).ctx(partial.display())?;
        std::io::copy(&mut reader, &mut file).ctx(url)?;
        file.flush().ctx(partial.display())?;
    }
    let actual = sha256_file(&partial)?;
    if actual != sha256 {
        let _ = fs::remove_file(&partial);
        return Err(format!(
            "{url} has SHA-256 {actual}, expected {sha256}; refusing it"
        ));
    }
    fs::rename(&partial, path).ctx(path.display())
}

/// Replace `path` with a directory built by `fill` in a sibling staging
/// directory, so an interrupted build never leaves a partial output.
pub fn replace_dir(path: &Path, fill: impl FnOnce(&Path) -> Result<()>) -> Result<()> {
    let staging = path.with_extension("installing");
    if staging.exists() {
        fs::remove_dir_all(&staging).ctx(staging.display())?;
    }
    fs::create_dir_all(&staging).ctx(staging.display())?;
    fill(&staging)?;
    if path.exists() {
        fs::remove_dir_all(path).ctx(path.display())?;
    }
    fs::rename(&staging, path).ctx(path.display())
}

/// Find an executable on PATH (with `.exe`/`.cmd` on Windows).
pub fn which(name: &str) -> Option<PathBuf> {
    let suffixes: &[&str] = if cfg!(windows) {
        &[".exe", ".cmd", ".bat", ""]
    } else {
        &[""]
    };
    std::env::split_paths(&std::env::var_os("PATH")?).find_map(|dir| {
        suffixes
            .iter()
            .map(|s| dir.join(format!("{name}{s}")))
            .find(|p| p.is_file())
    })
}

/// A path as Java and Windows tools expect it (forward slashes are fine).
pub fn native(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
