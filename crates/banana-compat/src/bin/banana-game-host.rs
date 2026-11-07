//! Small compiled host for the same registered GameCompat API used by the UI.
fn main() {
    let result = (|| -> Result<String, String> {
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        if args.len() != 3 {
            return Err("Usage: banana-game-host GAME_ID ACTION GAME_ROOT".into());
        }
        banana_compat::install();
        banana_runtime::run(
            args[0].to_str().ok_or("Invalid game id")?,
            std::path::Path::new(&args[2]),
            args[1].to_str().ok_or("Invalid action")?,
        )
    })();
    match result {
        Ok(message) => println!("{message}"),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
