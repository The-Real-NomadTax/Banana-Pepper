//! Dedicated 32-bit worker. No WSL, Linux libraries, or third-party loaders.
#[cfg(all(windows, target_arch = "x86"))]
#[path = "win32/mod.rs"]
mod win32;

fn main() {
    if let Err(e) = run() {
        eprintln!("[water] {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    #[cfg(all(windows, target_arch = "x86"))]
    {
        win32::launch()
    }
    #[cfg(not(all(windows, target_arch = "x86")))]
    {
        Err("Build water-buck with --target i686-pc-windows-msvc; this is a 32-bit Windows ELF worker".into())
    }
}
