use arcade_water::elf::Image;
use std::{
    fs,
    path::PathBuf,
    process::{Command, ExitCode},
    time::{Duration, Instant},
};

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(125)
        }
    }
}
fn run() -> Result<u8, String> {
    let mut args = std::env::args_os().skip(1);
    let action=args.next().ok_or("Usage: water inspect ELF | run ELF [--root DIR] [--work DIR] [--timeout SECONDS] [-- GUEST_ARGS...]")?;
    let path = PathBuf::from(args.next().ok_or("Missing ELF path")?);
    let metadata = fs::metadata(&path).map_err(|e| e.to_string())?;
    if metadata.len() > arcade_water::elf::MAX_IMAGE_BYTES as u64 {
        return Err("ELF exceeds 256 MiB".into());
    }
    let bytes = fs::read(&path).map_err(|e| e.to_string())?;
    let plan = Image::parse(&bytes)?;
    if action == "inspect" {
        if args.next().is_some() {
            return Err("Usage: water inspect ELF".into());
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&plan).map_err(|e| e.to_string())?
        );
        return Ok(0);
    }
    if action != "run" && action != "--worker" {
        return Err("Expected inspect or run".into());
    }
    plan.require_native_support()?;
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    let mut root = path
        .parent()
        .ok_or("ELF has no parent directory")?
        .to_path_buf();
    let mut work = std::env::current_dir()
        .map_err(|e| e.to_string())?
        .join("runtime/linux-native/work");
    let mut timeout = 10u64;
    let mut guest = vec![format!(
        "/{}",
        path.file_name()
            .ok_or("Missing ELF filename")?
            .to_string_lossy()
    )];
    while let Some(option) = args.next() {
        if option == "--" {
            guest.extend(args.map(|s| s.to_string_lossy().into_owned()));
            break;
        }
        let value = args.next().ok_or("Option requires a value")?;
        if option == "--root" {
            root = PathBuf::from(value);
        } else if option == "--work" {
            work = PathBuf::from(value);
        } else if option == "--timeout" {
            timeout = value
                .to_str()
                .ok_or("Timeout must be a number")?
                .parse()
                .map_err(|_| "Timeout must be a number")?;
        } else {
            return Err(format!("Unknown option {}", option.to_string_lossy()));
        }
    }
    if timeout == 0 || timeout > 3600 {
        return Err("Timeout must be 1..3600 seconds".into());
    }
    if action == "--worker" {
        arcade_water::run_worker(&bytes, &root, &work, &guest)?;
        return Ok(125);
    }
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let work = work.canonicalize().map_err(|e| e.to_string())?;
    let mut child = Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
        .arg("--worker")
        .arg(path)
        .arg("--root")
        .arg(root)
        .arg("--work")
        .arg(work)
        .arg("--")
        .args(&guest[1..])
        .spawn()
        .map_err(|e| e.to_string())?;
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return match status.code() {
                Some(code) if (0..=255).contains(&code) => Ok(code as u8),
                _ => Err(format!("Water worker failed: {status}")),
            };
        }
        if start.elapsed() > Duration::from_secs(timeout) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "Linux guest exceeded {timeout} second limit; worker terminated"
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
