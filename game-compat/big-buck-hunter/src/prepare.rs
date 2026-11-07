use sha2::{Digest, Sha256};
pub const SOURCE_SHA256: &str = "c569677c4e4bcf3449939b6e9a44afd3346c153133391c24dc06c012a58c1244";
const CODE_OFFSET: usize = 0x2d80;
const CODE_SIZE: usize = 0x937c0;
const ENTRY: u32 = 0x0804ad80;
const MASK: [u8; 4] = [0x84, 0xeb, 0x67, 0xf1];

pub fn transform(bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    let hash = format!("{:x}", Sha256::digest(&bytes));
    if hash != SOURCE_SHA256 {
        return Err(format!(
            "Unrecognized Big Buck Hunter executable: SHA256 {hash}. No changes made."
        ));
    }
    let decoded = decode_known_build(bytes);
    // Confirm the reconstructed Linux CRT startup before writing anything.
    if &decoded[CODE_OFFSET..CODE_OFFSET + 11] != b"\x31\xed\x5e\x89\xe1\x83\xe4\xf0\x50\x54\x52" {
        return Err("Reconstructed entry validation failed".into());
    }
    Ok(decoded)
}
fn decode_known_build(mut bytes: Vec<u8>) -> Vec<u8> {
    for (i, byte) in bytes[CODE_OFFSET..CODE_OFFSET + CODE_SIZE]
        .iter_mut()
        .enumerate()
    {
        *byte ^= MASK[i % MASK.len()];
    }
    bytes[24..28].copy_from_slice(&ENTRY.to_le_bytes());
    bytes[9..16].fill(0); // Reserved ELF identification padding.
    bytes
}
