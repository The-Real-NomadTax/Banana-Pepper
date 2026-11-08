//! Committed test fixtures that need a foreign toolchain to rebuild.
use crate::util::{Result, root, run};
use std::process::Command;

/// Water's ELF32 dynamic-linker fixtures (`backends/water/tests/elf32`), built
/// on a Linux host with a 32-bit C toolchain. They are committed so the
/// Windows test needs no Linux toolchain.
pub fn water() -> Result<()> {
    if !cfg!(target_os = "linux") {
        return Err("Water's ELF32 fixtures build on a Linux host with gcc -m32".into());
    }
    let dir = root().join("backends/water/tests/elf32");
    std::fs::create_dir_all(dir.join("game/lib")).map_err(|e| e.to_string())?;
    let flags = ["-m32", "-O1", "-fno-stack-protector", "-fcf-protection=none"];
    let gcc = |args: &[&str]| run(Command::new("gcc").current_dir(&dir).args(flags).args(args));
    gcc(&[
        "-fPIC",
        "-shared",
        "-Wl,-soname,libbanana_fixture.so",
        "-o",
        "game/lib/libbanana_fixture.so",
        "src/library.c",
    ])?;
    gcc(&[
        "-fPIC",
        "-shared",
        "-Wl,-soname,libbanana_plugin.so",
        "-Wl,-rpath,$ORIGIN",
        "-o",
        "game/lib/libbanana_plugin.so",
        "src/plugin.c",
        "game/lib/libbanana_fixture.so",
    ])?;
    gcc(&[
        "-fno-pie",
        "-no-pie",
        "-Wl,-rpath,$ORIGIN/lib",
        "-o",
        "game/program",
        "src/program.c",
        "-Lgame/lib",
        "-lbanana_fixture",
        "-ldl",
    ])?;
    run(Command::new("strip").current_dir(&dir).args([
        "--strip-unneeded",
        "game/program",
        "game/lib/libbanana_fixture.so",
        "game/lib/libbanana_plugin.so",
    ]))
}
