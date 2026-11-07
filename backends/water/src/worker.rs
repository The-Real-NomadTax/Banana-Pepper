use std::{collections::BTreeMap, path::PathBuf};
fn run() -> Result<(), String> {
    if let Some(index) = std::env::args_os().position(|a| a == "--timeout") {
        let mut args = std::env::args_os().skip(1).collect::<Vec<_>>();
        let millis = args
            .get(index)
            .ok_or("--timeout needs seconds")?
            .to_string_lossy()
            .parse::<u64>()
            .map_err(|e| e.to_string())?
            .checked_mul(1000)
            .ok_or("Timeout overflow")?;
        args.drain(index - 1..=index);
        let mut command =
            std::process::Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
        command.args(args);
        let mut session = banana_api::process::ProcessSession::spawn(&mut command, Some(millis))?;
        let code = session.wait()?;
        if code == 124 {
            return Err("worker terminated after timeout".into());
        }
        drop(session);
        std::process::exit(code)
    }
    let mut args = std::env::args_os().skip(1);
    let mut options = BTreeMap::new();
    let mut guest = Vec::new();
    while let Some(k) = args.next() {
        if k == "--" {
            guest.extend(args.map(|a| a.to_string_lossy().into_owned()));
            break;
        }
        let v = args.next().ok_or("Worker option requires a value")?;
        options.insert(k.to_string_lossy().into_owned(), PathBuf::from(v));
    }
    // The executable is a view path; every guest file comes from the view.
    let image = options
        .get("--image")
        .ok_or("--image required")?
        .to_string_lossy()
        .into_owned();
    banana_water::vfs::connect(
        &options
            .get("--view")
            .ok_or("--view required")?
            .to_string_lossy(),
    )?;
    let bytes = banana_water::vfs::view()
        .and_then(|v| v.read_all(&image, 1 << 31))
        .map_err(|e| format!("Cannot read {image}: {e}"))?;
    #[cfg(all(windows, target_arch = "x86"))]
    {
        let _ = guest;
        let state = options.get("--state").ok_or("--state required")?;
        let adapter = options.get("--adapter").ok_or("--adapter required")?;
        banana_water::win32::launch(&bytes, state, adapter)
    }
    #[cfg(not(all(windows, target_arch = "x86")))]
    {
        let name = image.rsplit('/').next().unwrap_or(&image).to_owned();
        let mut argv = vec![name];
        argv.extend(guest);
        banana_water::run_worker(&bytes, &argv)
    }
}
fn main() {
    if let Err(e) = run() {
        eprintln!("Water: {e}");
        std::process::exit(125)
    }
}
