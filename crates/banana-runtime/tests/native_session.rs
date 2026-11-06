//! Exercise the shipped supervisor through real isolation with synthetic guests.
#![cfg(target_os = "linux")]
use banana_runtime::{
    Sandbox,
    session::{self, Plan, Program},
};
use std::{fs, path::Path, time::Instant};

#[test]
fn relocated_compiled_session_preserves_arguments_and_reaps_timeout() {
    if banana_native::find_program("bwrap").is_none() {
        eprintln!("Bubblewrap unavailable on this host");
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let package = temp.path().join("Moved package [native]");
    fs::create_dir_all(package.join("helpers/linux")).unwrap();
    fs::write(package.join(banana_runtime::DIST_MARKER), "v1").unwrap();
    fs::copy(
        env!("CARGO_BIN_EXE_banana-session"),
        package.join("helpers/linux/banana-session"),
    )
    .unwrap();
    let output = package.join("runtime/session");
    fs::create_dir_all(&output).unwrap();
    let sandbox = |out: &Path| {
        let mut s = Sandbox::new();
        s.bind(out, "/work", false).chdir("/tmp");
        s
    };
    let literal = "literal ; $(touch /work/should-not-exist) [spaces]";
    let mut plan = Plan::new(Program::new("/usr/bin/printf", &["%s", literal]), "");
    plan.capture_every_ms = None;
    plan.timeout_ms = Some(2000);
    let code = session::launch(&mut sandbox(&output), &package, &output, &plan).unwrap();
    assert_eq!(
        code,
        0,
        "{}",
        fs::read_to_string(output.join("runner.log")).unwrap()
    );
    assert_eq!(
        fs::read_to_string(output.join("game.log")).unwrap(),
        literal
    );
    assert!(!output.join("should-not-exist").exists());
    plan.game = Program::new("/usr/bin/sleep", &["30"]);
    plan.timeout_ms = Some(50);
    let started = Instant::now();
    assert_eq!(
        session::launch(&mut sandbox(&output), &package, &output, &plan).unwrap(),
        124
    );
    assert!(started.elapsed().as_secs() < 3);
    assert_eq!(
        fs::read_to_string(output.join("game-exit-code.txt")).unwrap(),
        "124\n"
    );
}
