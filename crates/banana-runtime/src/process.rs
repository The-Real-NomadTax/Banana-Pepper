//! Owned child lifetimes and filesystem operations shared by Rust launchers.
use std::{
    fs::{self, File, OpenOptions},
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

pub fn lock(directory: &Path) -> Result<File, String> {
    fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let f = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(directory.join("session.lock"))
        .map_err(|e| e.to_string())?;
    f.try_lock()
        .map_err(|e| format!("Another session is using {}: {e}", directory.display()))?;
    Ok(f)
}
pub fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    fn walk(
        source: &Path,
        destination: &Path,
        parents: &mut Vec<std::path::PathBuf>,
    ) -> Result<(), String> {
        let canonical = source
            .canonicalize()
            .map_err(|e| format!("{}: {e}", source.display()))?;
        if parents.contains(&canonical) {
            return Err(format!("Link cycle in {}", source.display()));
        }
        if canonical.is_dir() {
            parents.push(canonical);
            fs::create_dir_all(destination).map_err(|e| e.to_string())?;
            for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                walk(&entry.path(), &destination.join(entry.file_name()), parents)?;
            }
            parents.pop();
        } else {
            fs::copy(canonical, destination).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    walk(source, destination, &mut Vec::new())
}
pub fn executable(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    let _ = path;
    Ok(())
}
pub struct ChildGuard(pub Child);
impl ChildGuard {
    pub fn spawn(command: &mut Command) -> Result<Self, String> {
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        command.stdin(Stdio::null());
        command.spawn().map(Self).map_err(|e| {
            format!(
                "Cannot start {}: {e}",
                command.get_program().to_string_lossy()
            )
        })
    }
    pub fn stop(&mut self) {
        #[cfg(target_os = "linux")]
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGTERM);
        }
        let until = Instant::now() + Duration::from_millis(500);
        while Instant::now() < until {
            if self.0.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        #[cfg(target_os = "linux")]
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGKILL);
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
    pub fn wait(&mut self, timeout: Option<Duration>) -> Result<Option<ExitStatus>, String> {
        let started = Instant::now();
        loop {
            if let Some(s) = self.0.try_wait().map_err(|e| e.to_string())? {
                return Ok(Some(s));
            }
            if timeout.is_some_and(|t| started.elapsed() >= t) {
                self.stop();
                return Ok(None);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            self.stop();
        }
    }
}
pub fn logged(
    command: &mut Command,
    log: &Path,
    timeout: Option<Duration>,
) -> Result<Option<ExitStatus>, String> {
    let file = File::create(log).map_err(|e| e.to_string())?;
    command
        .stdout(file.try_clone().map_err(|e| e.to_string())?)
        .stderr(file);
    ChildGuard::spawn(command)?.wait(timeout)
}
pub fn checked(command: &mut Command) -> Result<std::process::Output, String> {
    let output = command
        .output()
        .map_err(|e| format!("{}: {e}", command.get_program().to_string_lossy()))?;
    if !output.status.success() {
        return Err(format!(
            "{} failed ({}): {}",
            command.get_program().to_string_lossy(),
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(output)
}

/// Bound auxiliary tool execution and output without a shell or detached child.
pub fn bounded_output(
    command: &mut Command,
    timeout: Duration,
    limit: u64,
) -> Result<Option<std::process::Output>, String> {
    use std::io::Read;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = ChildGuard::spawn(command)?;
    let stdout = child.0.stdout.take().ok_or("Missing stdout pipe")?;
    let stderr = child.0.stderr.take().ok_or("Missing stderr pipe")?;
    let read = move |pipe: Box<dyn Read + Send>| {
        let mut bytes = Vec::new();
        let _ = pipe.take(limit).read_to_end(&mut bytes);
        bytes
    };
    let out = std::thread::spawn(move || read(Box::new(stdout)));
    let err = std::thread::spawn(move || read(Box::new(stderr)));
    let status = child.wait(Some(timeout))?;
    let stdout = out.join().map_err(|_| "Output reader failed")?;
    let stderr = err.join().map_err(|_| "Error reader failed")?;
    Ok(status.map(|status| std::process::Output {
        status,
        stdout,
        stderr,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore]
    fn child_waits() {
        if std::env::var_os("BANANA_TEST_CHILD").is_some() {
            std::thread::sleep(Duration::from_secs(30));
        }
    }
    #[test]
    fn timeout_reaps_a_real_child_without_an_interpreter() {
        let mut c = Command::new(std::env::current_exe().unwrap());
        c.args(["--ignored", "--exact", "process::tests::child_waits"])
            .env("BANANA_TEST_CHILD", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let started = Instant::now();
        let mut child = ChildGuard::spawn(&mut c).unwrap();
        assert!(
            child
                .wait(Some(Duration::from_millis(50)))
                .unwrap()
                .is_none()
        );
        assert!(child.0.try_wait().unwrap().is_some());
        assert!(started.elapsed() < Duration::from_secs(3));
    }
    #[test]
    fn save_copies_do_not_write_back_to_the_original_tree() {
        let t = tempfile::tempdir().unwrap();
        let a = t.path().join("original");
        let b = t.path().join("runtime");
        fs::create_dir(&a).unwrap();
        fs::write(a.join("save"), b"defaults").unwrap();
        copy_tree(&a, &b).unwrap();
        fs::write(b.join("save"), b"updated").unwrap();
        assert_eq!(fs::read(a.join("save")).unwrap(), b"defaults");
    }
}
