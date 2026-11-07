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
    let image = options.get("--image").ok_or("--image required")?;
    let root = options.get("--root").ok_or("--root required")?;
    #[cfg(all(windows, target_arch = "x86"))]
    {
        let state = options.get("--state").ok_or("--state required")?;
        let adapter = options.get("--adapter").ok_or("--adapter required")?;
        banana_water::win32::launch(image, root, state, adapter)
    }
    #[cfg(not(all(windows, target_arch = "x86")))]
    {
        let bytes = std::fs::read(image).map_err(|e| e.to_string())?;
        let mut argv = vec![
            image
                .file_name()
                .ok_or("Missing executable name")?
                .to_string_lossy()
                .into_owned(),
        ];
        argv.extend(guest);
        banana_water::run_worker(&bytes, root, &argv)
    }
}
fn main() {
    if let Err(e) = run() {
        eprintln!("Water: {e}");
        std::process::exit(125)
    }
}
