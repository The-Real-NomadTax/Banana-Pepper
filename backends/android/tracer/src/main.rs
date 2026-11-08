//! `banana-tracer [PRoot options] COMMAND ARGS…`
#[cfg(any(target_os = "android", target_os = "linux"))]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = match banana_tracer::parse(&args) {
        Ok(options) => options,
        Err(e) => {
            eprintln!("banana-tracer: {e}");
            std::process::exit(2)
        }
    };
    match banana_tracer::Tracer::new(options).run() {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("banana-tracer: {e}");
            std::process::exit(1)
        }
    }
}

#[cfg(not(any(target_os = "android", target_os = "linux")))]
fn main() {
    eprintln!("banana-tracer runs on Android and Linux");
    std::process::exit(1)
}
