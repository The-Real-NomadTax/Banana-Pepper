//! Repository tooling, run as `cargo xtask <command>`. Nothing here ships:
//! it builds the desktop distribution, every Android runtime input and the
//! APK, and the committed test fixtures.
mod android;
mod apk;
mod box64;
mod deb;
mod dist;
mod fixtures;
mod rootfs;
mod util;

use std::process::Command;
use util::{Result, root, run};

const USAGE: &str = "cargo xtask <command>
  dist                 build every artifact for this host into target/dist/banana-pepper-<os>-<arch>/
  package              repackage target/dist from already-built artifacts
  run [ARGS…]          build, then start the packaged desktop app with ARGS
  android <command>    the Android runtime and APK (cargo xtask android help)
  water-fixtures       rebuild Water's committed ELF32 test fixtures (Linux)";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = dispatch(&args) {
        eprintln!("xtask: {e}");
        std::process::exit(1)
    }
}

fn dispatch(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("dist") if args.len() == 1 => dist::run(dist::Host::Current, true),
        Some("package") if args.len() == 1 => dist::run(dist::Host::Current, false),
        Some("run") => desktop(&args[1..]),
        Some("android") => android::main(&args[1..]),
        Some("water-fixtures") if args.len() == 1 => fixtures::water(),
        _ => Err(USAGE.into()),
    }
}

/// Build the distribution, then run its desktop app from the package.
fn desktop(args: &[String]) -> Result<()> {
    dist::run(dist::Host::Current, true)?;
    let package = root().join(format!(
        "target/dist/banana-pepper-{}-{}",
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    let exe = package.join(format!("arcade-desktop{}", std::env::consts::EXE_SUFFIX));
    run(Command::new(exe).args(args))
}
