//! Water's ELF32 dynamic linker, end to end: a real 32-bit Linux program
//! (`tests/elf32/src`, built by `tests/elf32/build.sh`) runs in `water-x86`
//! from a served view, with a library it ships and a plugin it dlopens.
#![cfg(all(windows, target_arch = "x86_64"))]
use std::{path::PathBuf, process::Command, sync::Arc};

/// The 32-bit worker, built for this test (tests run as x86-64).
fn worker() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let status = Command::new(env!("CARGO"))
        .args([
            "build",
            "--release",
            "--locked",
            "-p",
            "banana-water",
            "--bin",
            "water-x86",
            "--target",
            "i686-pc-windows-msvc",
        ])
        .current_dir(&manifest)
        .status()
        .expect("cargo");
    assert!(
        status.success(),
        "building water-x86 failed (rustup target add i686-pc-windows-msvc)"
    );
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest.join("../../target"));
    target.join("i686-pc-windows-msvc/release/water-x86.exe")
}

struct Served(Arc<banana_vfs::View>);
impl banana_api::SessionView for Served {
    fn serve(
        &self,
        r: Box<dyn std::io::Read + Send>,
        w: Box<dyn std::io::Write + Send>,
    ) -> std::io::Result<()> {
        banana_vfs::wire::serve(self.0.clone(), r, w)
    }
    fn read(&self, path: &str, limit: u64) -> std::io::Result<Vec<u8>> {
        let mut b = vec![0; limit as usize];
        let n = self.0.read(path, 0, &mut b)?;
        b.truncate(n);
        Ok(b)
    }
    fn read_dir(&self, path: &str) -> std::io::Result<Vec<banana_content::Entry>> {
        self.0.read_dir(path)
    }
}

#[test]
fn links_shipped_libraries_runs_constructors_and_dlopens_plugins() {
    let worker = worker();
    let game = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/elf32/game");
    let state = std::env::temp_dir().join(format!("water-elf32-{}", std::process::id()));
    std::fs::create_dir_all(&state).unwrap();
    let view = Arc::new(
        banana_vfs::View::new(
            Arc::new(banana_content::Folder::new(&game).unwrap()),
            &state.join("overlay"),
            "elf32",
            Default::default(),
            Default::default(),
        )
        .unwrap(),
    );
    let pipes = banana_api::process::ViewPipes::new().unwrap();
    let mut command = Command::new(worker);
    command
        .args(["--image", "program", "--state"])
        .arg(&state)
        .arg("--view")
        .arg(pipes.child_spec())
        .args(["--timeout", "30"])
        .env("BANANA_GUEST_ROOT", "/game")
        .env_remove("LD_LIBRARY_PATH")
        .current_dir(&state);
    let output = pipes
        .spawn(Arc::new(Served(view)), || command.output())
        .unwrap()
        .unwrap();
    let log = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(42), "{log}");
    assert!(
        log.contains("loaded /game/lib/libbanana_fixture.so"),
        "{log}"
    );
    assert!(
        log.contains("dlopen /game/lib/libbanana_plugin.so"),
        "{log}"
    );
    let _ = std::fs::remove_dir_all(&state);
}
