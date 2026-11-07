fn main() {
    let result = (|| -> Result<(), String> {
        if std::env::args().nth(1).as_deref() != Some("--setup-legacy32") {
            return Err("Usage: banana-session --setup-legacy32".into());
        }
        let paths = banana_runtime::storage::Paths::discover()?;
        banana_runtime::dependencies::setup_legacy(&paths.data)
    })();
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1)
    }
}
