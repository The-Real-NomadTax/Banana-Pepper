//! Verify World 1.20, enable guest hooks and use session-owned supervision.
use sha2::{Digest, Sha256};
pub const SOURCE_SHA256: &str = "e1f17750ad82573a91cf3f2bbc27a14ca1573115ae8d8c515aa7f89f5571abd9";
const HOOKS: [usize; 4] = [
    0x08208445 - 0x08048000,
    0x08208647 - 0x08048000,
    0x0820ad16 - 0x08048000,
    0x08209462 - 0x08048000,
];

pub fn transform(mut bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    let hash = format!("{:x}", Sha256::digest(&bytes));
    if hash != SOURCE_SHA256 {
        return Err(format!(
            "Unrecognized Big Buck World executable: SHA256 {hash}. No changes made."
        ));
    }
    for offset in HOOKS {
        if bytes.get(offset..offset + 3) != Some(&[0x55, 0x89, 0xe5]) {
            return Err(format!("Incompatible World JAMMA prologue at {offset:#x}"));
        }
    }
    // The cabinet guardian forks a ptrace supervisor, which conflicts with
    // namespace/Yama restrictions and Android's tracer. Run the game branch
    // directly; the session owner already supervises and reaps the process.
    let guardian = 0x082dea6a - 0x08048000;
    if bytes.get(guardian..guardian + 6) != Some(&[0x55, 0x89, 0xe5, 0x83, 0xec, 0x38]) {
        return Err("Incompatible World supervisor prologue".into());
    }
    let guardian_check = 0x082ded09 - 0x08048000;
    if bytes.get(guardian_check..guardian_check + 9)
        != Some(&[0x55, 0x89, 0xe5, 0x81, 0xec, 0xc8, 0x02, 0, 0])
    {
        return Err("Incompatible World supervisor check prologue".into());
    }
    // ELF32 program header #2: PT_LOAD, file offset 0, RX. The guest
    // installs its JAMMA hooks here before game initialization on every host.
    let flags = 52 + 2 * 32 + 24;
    if bytes.get(flags..flags + 4) != Some(&5u32.to_le_bytes()) {
        return Err("Incompatible World code segment flags".into());
    }
    bytes[flags..flags + 4].copy_from_slice(&7u32.to_le_bytes());
    bytes[guardian..guardian + 6].copy_from_slice(&[0xb8, 1, 0, 0, 0, 0xc3]);
    // Its per-frame liveness check must use the same session-owned policy.
    bytes[guardian_check..guardian_check + 3].copy_from_slice(&[0x31, 0xc0, 0xc3]);
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    #[test]
    fn unknown_images_are_rejected_before_patching() {
        assert!(
            super::transform(Vec::new())
                .unwrap_err()
                .contains("Unrecognized")
        );
        assert!(super::transform(vec![0; 256]).is_err());
    }
}
