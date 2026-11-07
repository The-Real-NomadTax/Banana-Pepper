fn main() {
    let result = (|| -> Result<String, String> {
        let mut args = std::env::args().skip(1);
        let code = args
            .next()
            .ok_or("Usage: banana-minecraft-card CODE [PLAYER]")?;
        let player = args
            .next()
            .map(|p| p.parse::<u8>())
            .transpose()
            .map_err(|_| "Invalid player")?
            .unwrap_or(1);
        if args.next().is_some() {
            return Err("Too many arguments".into());
        }
        banana_game_minecraft_dungeons::runtime::scan_card(player, &code)
    })();
    match result {
        Ok(message) => println!("{message}"),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
