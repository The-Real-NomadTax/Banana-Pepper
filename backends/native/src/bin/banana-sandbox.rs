//! Session namespace helper (see `banana_native::namespace`).
#[cfg(target_os = "linux")]
fn main() {
    banana_native::namespace::helper_main()
}
#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("banana-sandbox runs on Linux hosts");
    std::process::exit(125)
}
