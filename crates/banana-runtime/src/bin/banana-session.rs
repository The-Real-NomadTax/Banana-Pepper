fn main() {
    let result = (|| -> Result<i32, String> {
        if std::env::args_os()
            .nth(1)
            .is_some_and(|a| a == "--display-exec")
        {
            let mut args = std::env::args_os().skip(2);
            let executable = args.next().ok_or("--display-exec PROGRAM [ARGS]")?;
            let display = std::env::var_os("DISPLAY").ok_or("gamescope did not set DISPLAY")?;
            let mut command = std::process::Command::new(executable);
            command.args(args.map(|a| if a == "@DISPLAY@" { display.clone() } else { a }));
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                return Err(command.exec().to_string());
            }
            #[cfg(not(unix))]
            {
                return command
                    .status()
                    .map(|s| s.code().unwrap_or(1))
                    .map_err(|e| e.to_string());
            }
        }
        if std::env::args_os()
            .nth(1)
            .is_some_and(|a| a == "--setup-legacy32")
        {
            let root = std::env::args_os().nth(2).ok_or("--setup-legacy32 ROOT")?;
            banana_runtime::dependencies::setup_legacy(std::path::Path::new(&root))?;
            return Ok(0);
        }
        let path = std::env::args_os()
            .nth(1)
            .ok_or("Usage: banana-session SESSION.json")?;
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        banana_runtime::session::supervise(
            serde_json::from_slice(&bytes).map_err(|e| e.to_string())?,
        )
    })();
    match result {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("Session: {e}");
            std::process::exit(1);
        }
    }
}
