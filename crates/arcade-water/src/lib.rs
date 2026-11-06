//! Water Adapts The ELF Runtime: Linux ABI translation on Windows.
//!
//! Native execution currently supports a constrained static ELF64 ABI. Dynamic
//! linking, TLS, graphics and Linux threads are explicitly rejected. Guest code
//! runs in a dedicated process; this is a compatibility runtime, not a security
//! sandbox for untrusted executables.
pub mod elf;
pub mod stack;
pub mod vfs;

#[cfg(all(windows, target_arch = "x86_64"))]
mod windows;

pub fn run_worker(
    image: &[u8],
    root: &std::path::Path,
    work: &std::path::Path,
    argv: &[String],
) -> Result<(), String> {
    let plan = elf::Image::parse(image)?;
    plan.require_native_support()?;
    #[cfg(all(windows, target_arch = "x86_64"))]
    return windows::run(image, &plan, root, work, argv);
    #[cfg(not(all(windows, target_arch = "x86_64")))]
    {
        let _ = (root, work, argv);
        Err("Native Linux ABI execution requires Windows x86-64".into())
    }
}
