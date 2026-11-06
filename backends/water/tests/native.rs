use banana_water::elf::Image;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static CASE: AtomicUsize = AtomicUsize::new(0);

/// Four ordinary PT_LOADs: ELF headers, executable text, rodata, and a data
/// byte followed by BSS. No Linux toolchain or proprietary fixture is required.
fn elf(code: &[u8], rodata: &[u8]) -> Vec<u8> {
    assert!(code.len() < 4096 && rodata.len() < 4096);
    let mut b = vec![0u8; 0x3001];
    b[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    b[16..18].copy_from_slice(&2u16.to_le_bytes());
    b[18..20].copy_from_slice(&62u16.to_le_bytes());
    b[20..24].copy_from_slice(&1u32.to_le_bytes());
    b[24..32].copy_from_slice(&0x401000u64.to_le_bytes());
    b[32..40].copy_from_slice(&64u64.to_le_bytes());
    b[52..54].copy_from_slice(&64u16.to_le_bytes());
    b[54..56].copy_from_slice(&56u16.to_le_bytes());
    b[56..58].copy_from_slice(&4u16.to_le_bytes());
    b[58..60].copy_from_slice(&64u16.to_le_bytes());
    for (index, (offset, length, memory, flags)) in [
        (0u64, 288u64, 288u64, 4u32),
        (0x1000, code.len() as u64, code.len() as u64, 5),
        (
            0x2000,
            rodata.len().max(1) as u64,
            rodata.len().max(1) as u64,
            4,
        ),
        (0x3000, 1, 4096, 6),
    ]
    .into_iter()
    .enumerate()
    {
        let at = 64 + index * 56;
        b[at..at + 4].copy_from_slice(&1u32.to_le_bytes());
        b[at + 4..at + 8].copy_from_slice(&flags.to_le_bytes());
        for (field, value) in [
            (8, offset),
            (16, 0x400000 + offset),
            (24, 0x400000 + offset),
            (32, length),
            (40, memory),
            (48, 4096),
        ] {
            b[at + field..at + field + 8].copy_from_slice(&value.to_le_bytes());
        }
    }
    b[0x1000..0x1000 + code.len()].copy_from_slice(code);
    b[0x2000..0x2000 + rodata.len()].copy_from_slice(rodata);
    b
}
struct Code {
    bytes: Vec<u8>,
    failures: Vec<usize>,
}
impl Code {
    fn new() -> Self {
        Self {
            bytes: Vec::new(),
            failures: Vec::new(),
        }
    }
    fn emit(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }
    fn imm(&mut self, opcode: &[u8], value: u32) {
        self.emit(opcode);
        self.emit(&value.to_le_bytes());
    }
    fn syscall(&mut self, n: u32) {
        self.imm(&[0xb8], n);
        self.emit(&[0x0f, 0x05]);
    }
    fn assert_rax(&mut self, value: i32) {
        self.imm(&[0x48, 0x3d], value as u32);
        self.emit(&[0x0f, 0x85]);
        self.failures.push(self.bytes.len());
        self.emit(&[0; 4]);
    }
    fn exit(&mut self, status: u32) {
        self.imm(&[0xbf], status);
        self.syscall(60);
    }
    fn finish(mut self) -> Vec<u8> {
        self.exit(0);
        let target = self.bytes.len();
        self.exit(99);
        for offset in self.failures {
            self.bytes[offset..offset + 4]
                .copy_from_slice(&((target as i64 - offset as i64 - 4) as i32).to_le_bytes());
        }
        self.bytes
    }
}
fn temp() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "arcade-native-{}-{}",
        std::process::id(),
        CASE.fetch_add(1, Ordering::SeqCst)
    ));
    fs::create_dir_all(&path).unwrap();
    path
}
#[cfg(all(windows, target_arch = "x86_64"))]
fn run(bytes: &[u8], timeout: u32) -> (std::process::Output, PathBuf) {
    let path = temp();
    fs::write(path.join("guest.elf"), bytes).unwrap();
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_water"))
        .arg("run")
        .arg(path.join("guest.elf"))
        .arg("--root")
        .arg(&path)
        .arg("--work")
        .arg(path.join("work"))
        .arg("--timeout")
        .arg(timeout.to_string())
        .output()
        .unwrap();
    (result, path)
}

#[test]
fn validates_elf_bounds_and_rejects_untranslated_entry_paths() {
    let mut code = Code::new();
    code.exit(0);
    let valid = elf(&code.bytes, b"");
    assert!(
        Image::parse(&valid)
            .unwrap()
            .require_native_support()
            .is_ok()
    );
    let mut bad = valid.clone();
    bad[64 + 2 * 56 + 32..64 + 2 * 56 + 40].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(Image::parse(&bad).is_err());
    let ambiguous = elf(&[0x48, 0xb8, 0x0f, 0x05, 0, 0, 0, 0, 0, 0, 0xc3], b"");
    assert!(
        Image::parse(&ambiguous)
            .unwrap()
            .require_native_support()
            .is_err()
    );
    let tls = elf(&[0x64, 0x48, 0x8b, 0x04, 0x25, 0, 0, 0, 0, 0xc3], b"");
    assert!(
        Image::parse(&tls)
            .unwrap()
            .native_blockers
            .iter()
            .any(|s| s.contains("TLS"))
    );
    let mut rwx = valid;
    rwx[64 + 56 + 4..64 + 56 + 8].copy_from_slice(&7u32.to_le_bytes());
    assert!(
        Image::parse(&rwx)
            .unwrap()
            .require_native_support()
            .is_err()
    );
}

#[cfg(all(windows, target_arch = "x86_64"))]
#[test]
fn executes_linux_write_and_preserves_guest_exit_status() {
    let message = b"native Linux on Windows\n";
    let mut code = Code::new();
    code.imm(&[0xbf], 1);
    code.imm(&[0xbe], 0x402000);
    code.imm(&[0xba], message.len() as u32);
    code.syscall(1);
    code.exit(7);
    let (result, path) = run(&elf(&code.bytes, message), 3);
    assert_eq!(
        result.status.code(),
        Some(7),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(result.stdout, message);
    fs::remove_dir_all(path).unwrap();
}

#[cfg(all(windows, target_arch = "x86_64"))]
#[test]
fn translates_errno_validates_guest_buffers_and_zeroes_bss() {
    let mut code = Code::new();
    code.syscall(999);
    code.assert_rax(-38);
    code.imm(&[0xbf], 1);
    code.imm(&[0xbe], 0xdead0000);
    code.imm(&[0xba], 8);
    code.syscall(1);
    code.assert_rax(-14);
    code.emit(&[0x48, 0x8b, 0x04, 0x25, 0x08, 0x30, 0x40, 0x00]);
    code.assert_rax(0);
    code.emit(&[0x48, 0x8b, 0x04, 0x24]);
    code.assert_rax(1); // initial argc
    let (result, path) = run(&elf(&code.finish(), b""), 3);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    fs::remove_dir_all(path).unwrap();
}

#[cfg(all(windows, target_arch = "x86_64"))]
#[test]
fn supports_private_mmap_protection_and_unmap() {
    let mut code = Code::new();
    code.emit(&[0x31, 0xff]);
    code.imm(&[0xbe], 4096);
    code.imm(&[0xba], 3);
    code.imm(&[0x41, 0xba], 0x22);
    code.imm(&[0x49, 0xc7, 0xc0], u32::MAX);
    code.emit(&[0x45, 0x31, 0xc9]);
    code.syscall(9);
    code.emit(&[0x49, 0x89, 0xc4]); // r12 = allocation
    code.emit(&[0x48, 0xbb]);
    code.emit(b"mmap-ok\n");
    code.emit(&[0x49, 0x89, 0x1c, 0x24]);
    code.imm(&[0xbf], 1);
    code.emit(&[0x4c, 0x89, 0xe6]);
    code.imm(&[0xba], 8);
    code.syscall(1);
    code.assert_rax(8);
    code.emit(&[0x4c, 0x89, 0xe7]);
    code.imm(&[0xbe], 4096);
    code.imm(&[0xba], 1);
    code.syscall(10);
    code.assert_rax(0);
    code.emit(&[0x4c, 0x89, 0xe7]);
    code.imm(&[0xbe], 4096);
    code.syscall(11);
    code.assert_rax(0);
    let (result, path) = run(&elf(&code.finish(), b""), 3);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(result.stdout, b"mmap-ok\n");
    fs::remove_dir_all(path).unwrap();
}

#[cfg(all(windows, target_arch = "x86_64"))]
#[test]
fn guest_file_round_trip_is_confined_to_work_mount() {
    let name = b"/work/test.txt\0";
    let message = b"file-io works\n";
    let mut data = name.to_vec();
    data.extend_from_slice(message);
    let mut code = Code::new();
    code.imm(&[0xbf], 0x402000);
    code.imm(&[0xbe], 1 | 64 | 512);
    code.imm(&[0xba], 0o600);
    code.syscall(2);
    code.emit(&[0x41, 0x89, 0xc4]);
    code.emit(&[0x44, 0x89, 0xe7]);
    code.imm(&[0xbe], 0x402000 + name.len() as u32);
    code.imm(&[0xba], message.len() as u32);
    code.syscall(1);
    code.assert_rax(message.len() as i32);
    code.emit(&[0x44, 0x89, 0xe7]);
    code.syscall(3);
    code.assert_rax(0);
    code.imm(&[0xbf], 0x402000);
    code.emit(&[0x31, 0xf6]);
    code.syscall(2);
    code.emit(&[0x41, 0x89, 0xc4]);
    code.emit(&[0x44, 0x89, 0xe7]);
    code.imm(&[0xbe], 0x403008);
    code.imm(&[0xba], message.len() as u32);
    code.syscall(0);
    code.assert_rax(message.len() as i32);
    code.imm(&[0xbf], 1);
    code.imm(&[0xbe], 0x403008);
    code.imm(&[0xba], message.len() as u32);
    code.syscall(1);
    code.assert_rax(message.len() as i32);
    let (result, path) = run(&elf(&code.finish(), &data), 3);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(result.stdout, message);
    assert_eq!(fs::read(path.join("work/test.txt")).unwrap(), message);
    fs::remove_dir_all(path).unwrap();
}

#[cfg(all(windows, target_arch = "x86_64"))]
#[test]
fn terminates_a_non_returning_guest() {
    let (result, path) = run(&elf(&[0xeb, 0xfe], b""), 1);
    assert_eq!(result.status.code(), Some(125));
    assert!(String::from_utf8_lossy(&result.stderr).contains("worker terminated"));
    fs::remove_dir_all(path).unwrap();
}
