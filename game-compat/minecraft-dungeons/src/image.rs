//! Build-verified import reconstruction and runtime image preparation.
use crate::prepare::DUMP_SHA256;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
const BASE: usize = 0x400000;
fn read<const N: usize>(b: &[u8], at: usize) -> Result<[u8; N], String> {
    b.get(at..at + N)
        .ok_or_else(|| format!("Truncated image at {at:#x}"))?
        .try_into()
        .map_err(|_| "Invalid image field".into())
}
fn u32_at(b: &[u8], at: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(read(b, at)?))
}
fn u64_at(b: &[u8], at: usize) -> Result<u64, String> {
    Ok(u64::from_le_bytes(read(b, at)?))
}
fn cstr(b: &[u8], at: usize) -> Result<&[u8], String> {
    let b = b.get(at..).ok_or("Invalid string offset")?;
    let end = b
        .iter()
        .position(|x| *x == 0)
        .ok_or("Unterminated ELF string")?;
    Ok(&b[..end])
}
fn hash(name: &[u8]) -> u32 {
    let mut n = 0u32;
    for b in name {
        n = n.wrapping_shl(4).wrapping_add(u32::from(*b));
        let h = n & 0xf0000000;
        if h != 0 {
            n ^= h >> 24;
        }
        n &= !h;
    }
    n
}
pub fn prepare(source: &[u8]) -> Result<Vec<u8>, String> {
    if banana_runtime::dependencies::sha256(source) != DUMP_SHA256 {
        return Err("Unknown Minecraft build; refusing import reconstruction".into());
    }
    reconstruct(source)
}
fn reconstruct(source: &[u8]) -> Result<Vec<u8>, String> {
    let mut data = source.to_vec();
    let mut strings = source
        .get(0x44c0..0x44c0 + 3131)
        .ok_or("Missing string table")?
        .to_vec();
    let mut symbols = source
        .get(0x458..0x458 + 214 * 24)
        .ok_or("Missing symbols")?
        .to_vec();
    let mut versions = source
        .get(0x7850..0x7850 + 214 * 2)
        .ok_or("Missing versions")?
        .to_vec();
    let mut relocations = source
        .get(0x7fd0..0x7fd0 + 384)
        .ok_or("Missing relocations")?
        .to_vec();
    let source_strings = source
        .get(0x1b93984..0x1b980cc)
        .ok_or("Missing packed names")?;
    let mut names = BTreeMap::new();
    let mut cursor = 0;
    while cursor < strings.len() {
        let name = cstr(&strings, cursor)?.to_vec();
        let len = name.len() + 1;
        names.insert(name, cursor as u32);
        cursor += len;
    }
    let mut destinations = BTreeSet::new();
    for offset in (0x1b980cc..0x1b9dd78).step_by(36) {
        let kind = u32_at(source, offset)?;
        let name = u32_at(source, offset + 4)? as usize;
        let version = u32_at(source, offset + 8)? as usize;
        let destination = u64_at(source, offset + 12)?;
        let addend = u64_at(source, offset + 20)?;
        let weak = u32_at(source, offset + 28)?;
        let size = u32_at(source, offset + 32)?;
        if ![0, 6, 7].contains(&kind) || addend != 0 || size != 0 || weak > 1 {
            return Err("Unexpected import record".into());
        }
        if !(0x16c7a80..=0x16c8fe0).contains(&destination) || !destinations.insert(destination) {
            return Err("Unexpected or duplicate import destination".into());
        }
        if name == 0 || name >= source_strings.len() || version >= source_strings.len() {
            return Err("Invalid import name".into());
        }
        let symbol = cstr(source_strings, name)?;
        if !symbol.is_ascii() {
            return Err("Non-ASCII import name".into());
        }
        let name_offset = if let Some(n) = names.get(symbol) {
            *n
        } else {
            let n = strings.len() as u32;
            names.insert(symbol.to_vec(), n);
            strings.extend_from_slice(symbol);
            strings.push(0);
            n
        };
        let index = symbols.len() / 24;
        symbols.extend_from_slice(&name_offset.to_le_bytes());
        symbols.extend_from_slice(&[if weak == 1 { 0x20 } else { 0x10 }, 0, 0, 0]);
        symbols.extend_from_slice(&[0; 16]);
        versions.extend_from_slice(&1u16.to_le_bytes());
        relocations.extend_from_slice(&destination.to_le_bytes());
        relocations.extend_from_slice(&(((index as u64) << 32) | 6).to_le_bytes());
        relocations.extend_from_slice(&0i64.to_le_bytes());
    }
    if destinations.len() != 659 {
        return Err("Import count mismatch".into());
    }
    let count = symbols.len() / 24;
    let mut buckets = vec![0u32; 521];
    let mut chains = vec![0u32; count];
    for (index, chain) in chains.iter_mut().enumerate().skip(1) {
        let start = u32_at(&symbols, index * 24)? as usize;
        let bucket = hash(cstr(&strings, start)?) as usize % 521;
        *chain = buckets[bucket];
        buckets[bucket] = index as u32;
    }
    let mut hashes = Vec::new();
    for n in [521u32, count as u32]
        .into_iter()
        .chain(buckets)
        .chain(chains)
    {
        hashes.extend_from_slice(&n.to_le_bytes());
    }
    let new_offset = (data.len() + 4095) & !4095;
    data.resize(new_offset, 0);
    let mut addresses = BTreeMap::new();
    for (tag, blob) in [
        (5, strings.as_slice()),
        (6, symbols.as_slice()),
        (4, hashes.as_slice()),
        (0x6ffffff0, versions.as_slice()),
        (7, relocations.as_slice()),
    ] {
        data.resize((data.len() + 7) & !7, 0);
        addresses.insert(tag, 0x3000000u64 + (data.len() - new_offset) as u64);
        data.extend_from_slice(blob);
    }
    addresses.insert(10, strings.len() as u64);
    addresses.insert(8, relocations.len() as u64);
    for offset in (0x12c77a8..0x12c77a8 + 0x290).step_by(16) {
        if let Some(value) = addresses.get(&u64_at(&data, offset)?) {
            data[offset + 8..offset + 16].copy_from_slice(&value.to_le_bytes());
        }
    }
    let note = 64 + 4 * 56;
    if u32_at(&data, note)? != 4 {
        return Err("Unexpected program-header layout".into());
    }
    let size = (data.len() - new_offset) as u64;
    let mut header = Vec::new();
    header.extend_from_slice(&1u32.to_le_bytes());
    header.extend_from_slice(&6u32.to_le_bytes());
    for n in [new_offset as u64, 0x3000000, 0x3000000, size, size, 4096] {
        header.extend_from_slice(&n.to_le_bytes());
    }
    data[note..note + 56].copy_from_slice(&header);
    Ok(data)
}
fn hex(s: &str) -> Result<Vec<u8>, String> {
    if s.len() % 2 != 0 || !s.is_ascii() {
        return Err("Invalid hexadecimal record".into());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| "Invalid hexadecimal record".into())
        })
        .collect()
}
fn patch(image: &mut [u8], va: usize, original: &[u8], replacement: &[u8]) -> Result<(), String> {
    let at = va.checked_sub(BASE).ok_or("Invalid patch address")?;
    if image.get(at..at + original.len()) != Some(original) {
        return Err(format!("Unexpected bytes at {va:#x}"));
    }
    image
        .get_mut(at..at + replacement.len())
        .ok_or("Patch outside image")?
        .copy_from_slice(replacement);
    Ok(())
}
pub fn seeded(prepared: &[u8], seeds: &Value) -> Result<Vec<u8>, String> {
    if seeds["game_sha256"] != DUMP_SHA256 {
        return Err("Seed records belong to a different executable build".into());
    }
    let mut image = prepared.to_vec();
    patch(&mut image, 0x182a4ac, &[0; 4], &1u32.to_le_bytes())?;
    patch(&mut image, 0x179dee4, &[0; 4], &7000u32.to_le_bytes())?;
    *image
        .get_mut(0x17bad20 - BASE)
        .ok_or("Missing region cache")? = 48;
    *image
        .get_mut(0x179d100 - BASE)
        .ok_or("Missing factory flag")? = 1;
    if image.get(0x430487 - BASE) != Some(&0xe8) {
        return Err("Unexpected HASP initialization site".into());
    }
    for (name, va, size) in [("seed64", 0x16d3d40, 64), ("seed128", 0x16d3d80, 128)] {
        let b = hex(seeds[name].as_str().ok_or("Missing captured seed table")?)?;
        if b.len() != size {
            return Err(format!("{name} must be {size} bytes"));
        }
        image
            .get_mut(va - BASE..va - BASE + size)
            .ok_or("Seed outside image")?
            .copy_from_slice(&b);
    }
    for (va, value) in [
        (0x1eb7d20, 0x16d4220u64),
        (0x1eb7d40, 0x16d41e0),
        (0x1eb7d38, 0x16d3e00),
        (0x1eb7d28, 0x16d3d80),
        (0x1eb7d30, 0x16d3d40),
    ] {
        image
            .get_mut(va - BASE..va - BASE + 8)
            .ok_or("Seed pointer outside image")?
            .copy_from_slice(&value.to_le_bytes());
    }
    for (va, original, replacement) in [
        (0x4a458d, "7529", "9090"),
        (0x4a44e0, "53488d", "31c0c3"),
        (0x4a5440, "8b15fa5831", "e9dbffffff"),
        (0x438bc0, "488d35", "31c0c3"),
        (0x42466c, "741a", "9090"),
        (0x490f79, "782d", "9090"),
    ] {
        patch(&mut image, va, &hex(original)?, &hex(replacement)?)?;
    }
    for (va, original) in [
        (0x50e4f6, "48c7c600505b00488d15c72ec90031ffe9156b0a00"),
        (0x50e710, "48c7c600505b00488d15ad2cc90031ffe9fb680a00"),
    ] {
        let mut code = vec![0x48, 0x8d, 0x35];
        code.extend_from_slice(&((0x11a13cbi64 - (va + 7) as i64) as i32).to_le_bytes());
        code.extend_from_slice(&[0x31, 0xff, 0xe9]);
        code.extend_from_slice(&((0x4ac960i64 - (va + 14) as i64) as i32).to_le_bytes());
        code.resize(21, 0x90);
        patch(&mut image, va, &hex(original)?, &code)?;
    }
    patch(
        &mut image,
        0x430490,
        &hex("1c03000031c0488d6c2420e8c0450700")?,
        b"LOGO_TRANSITION\0",
    )?;
    let mut code = vec![0x48, 0x8d, 0x35];
    code.extend_from_slice(&((0x430490i64 - 0x433117) as i32).to_le_bytes());
    patch(&mut image, 0x433110, &hex("488d35f243d400")?, &code)?;
    let mut jump = vec![0xe9];
    jump.extend_from_slice(&((0x430542i64 - (0x430487 + 5)) as i32).to_le_bytes());
    patch(&mut image, 0x430487, &[0xe8], &jump)?;
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_builds_and_mismatched_records_are_rejected() {
        assert!(prepare(b"unknown").is_err());
        assert!(seeded(&[], &serde_json::json!({"game_sha256":"wrong"})).is_err());
    }
    #[test]
    fn reconstruction_rejects_duplicate_got_destinations() {
        let mut s = vec![0; 0x1b9dd78];
        s[0x1b93985..0x1b93989].copy_from_slice(b"sym\0");
        for offset in (0x1b980cc..0x1b9dd78).step_by(36) {
            s[offset..offset + 4].copy_from_slice(&6u32.to_le_bytes());
            s[offset + 4..offset + 8].copy_from_slice(&1u32.to_le_bytes());
            s[offset + 12..offset + 20].copy_from_slice(&0x16c7a80u64.to_le_bytes());
        }
        assert!(reconstruct(&s).unwrap_err().contains("duplicate"));
    }
    #[test]
    fn synthetic_image_matches_the_retired_runner_byte_for_byte() {
        let mut source = vec![0u8; 0x1b9dd78];
        source[0x1b93985..0x1b93989].copy_from_slice(b"sym\0");
        for (i, offset) in (0x1b980cc..0x1b9dd78).step_by(36).enumerate() {
            source[offset..offset + 4].copy_from_slice(&6u32.to_le_bytes());
            source[offset + 4..offset + 8].copy_from_slice(&1u32.to_le_bytes());
            source[offset + 12..offset + 20]
                .copy_from_slice(&(0x16c7a80u64 + i as u64 * 8).to_le_bytes());
            source[offset + 28..offset + 32].copy_from_slice(&(i as u32 % 2).to_le_bytes());
        }
        source[64 + 4 * 56..68 + 4 * 56].copy_from_slice(&4u32.to_le_bytes());
        for (i, tag) in [5u64, 6, 4, 0x6ffffff0, 7, 10, 8].into_iter().enumerate() {
            source[0x12c77a8 + i * 16..0x12c77a8 + i * 16 + 8].copy_from_slice(&tag.to_le_bytes());
        }
        for (va, original) in [
            (0x4a458d, "7529"),
            (0x4a44e0, "53488d"),
            (0x4a5440, "8b15fa5831"),
            (0x438bc0, "488d35"),
            (0x42466c, "741a"),
            (0x490f79, "782d"),
            (0x50e4f6, "48c7c600505b00488d15c72ec90031ffe9156b0a00"),
            (0x50e710, "48c7c600505b00488d15ad2cc90031ffe9fb680a00"),
            (0x430490, "1c03000031c0488d6c2420e8c0450700"),
            (0x433110, "488d35f243d400"),
        ] {
            let bytes = hex(original).unwrap();
            source[va - BASE..va - BASE + bytes.len()].copy_from_slice(&bytes);
        }
        source[0x430487 - BASE] = 0xe8;
        assert_eq!(
            banana_runtime::dependencies::sha256(&source),
            "b1b5108023f5b5e175789c9b7c200f1fb958ec42d47fccf56208ef56df76de39"
        );
        let prepared = reconstruct(&source).unwrap();
        assert_eq!(prepared.len(), 29006344);
        assert_eq!(
            banana_runtime::dependencies::sha256(&prepared),
            "218d556a8341a4e53f3d9fea68b091182cbd8b36dce8dac59dc0cbdb264a6dee"
        );
        let records = serde_json::json!({"game_sha256":DUMP_SHA256,"seed64":"11".repeat(64),"seed128":"22".repeat(128)});
        let result = seeded(&prepared, &records).unwrap();
        assert_eq!(
            banana_runtime::dependencies::sha256(&result),
            "87e832eb84bfdf400f2acb519866c08b1c4c630dad8fcf278b71f918b5d8e377"
        );
        assert_eq!(
            &source[0xbf30..0xd73300],
            &prepared[0xbf30..0xd73300],
            "Import reconstruction must preserve executable code"
        );
        let mut invalid = prepared.clone();
        invalid[0x4a458d - BASE] = 0;
        let saved = invalid.clone();
        assert!(seeded(&invalid, &records).is_err());
        assert_eq!(invalid, saved, "Rejected patches must be atomic");
    }
}
