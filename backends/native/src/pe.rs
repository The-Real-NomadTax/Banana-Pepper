//! Add a DLL import to a PE image (PE32 or PE32+). The new import descriptor
//! comes first, so Windows loads that DLL before the image's own imports and
//! before any of its code runs. Used on staged copies only.

fn u16_at(b: &[u8], at: usize) -> Result<u16, String> {
    b.get(at..at + 2)
        .map(|s| u16::from_le_bytes(s.try_into().unwrap()))
        .ok_or_else(|| "Truncated PE image".into())
}
fn u32_at(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
        .ok_or_else(|| "Truncated PE image".into())
}
fn put32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}
fn align(v: u32, a: u32) -> u32 {
    v.div_ceil(a) * a
}

/// The image with `dll!function` imported first.
pub fn add_import(image: &[u8], dll: &str, function: &str) -> Result<Vec<u8>, String> {
    let mut b = image.to_vec();
    if b.get(..2) != Some(b"MZ") {
        return Err("Not a PE image".into());
    }
    let pe = u32_at(&b, 0x3c)? as usize;
    if b.get(pe..pe + 4) != Some(b"PE\0\0") {
        return Err("Missing PE signature".into());
    }
    let coff = pe + 4;
    let sections = u16_at(&b, coff + 2)? as usize;
    let optional = coff + 20;
    let optional_size = u16_at(&b, coff + 16)? as usize;
    let plus = match u16_at(&b, optional)? {
        0x10b => false,
        0x20b => true,
        _ => return Err("Unknown PE optional header".into()),
    };
    let section_alignment = u32_at(&b, optional + 32)?;
    let file_alignment = u32_at(&b, optional + 36)?;
    let headers_size = u32_at(&b, optional + 60)? as usize;
    let directories = optional + if plus { 112 } else { 96 };
    let directory_count = u32_at(&b, optional + if plus { 108 } else { 92 })?;
    if directory_count < 12 || section_alignment == 0 || file_alignment == 0 {
        return Err("Unsupported PE directory layout".into());
    }
    let table = optional + optional_size;
    let section = |i: usize| table + i * 40;
    // Room for one more section header inside the existing headers.
    let first_raw = (0..sections)
        .map(|i| u32_at(&b, section(i) + 20))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|&r| r != 0)
        .min()
        .unwrap_or(headers_size as u32) as usize;
    let header = section(sections);
    if header + 40 > headers_size.min(first_raw) || b[header..header + 40].iter().any(|&x| x != 0) {
        return Err("No room for another section header".into());
    }
    let rva_to_offset = |b: &[u8], rva: u32| -> Result<usize, String> {
        for i in 0..sections {
            let s = section(i);
            let (va, vsize, raw_size, raw) = (
                u32_at(b, s + 12)?,
                u32_at(b, s + 8)?,
                u32_at(b, s + 16)?,
                u32_at(b, s + 20)?,
            );
            if rva >= va && rva < va + vsize.max(raw_size) {
                return Ok((rva - va + raw) as usize);
            }
        }
        Err(format!("RVA {rva:#x} is outside every section"))
    };
    // Existing descriptors (20 bytes each, ending with an all-zero one).
    let (import_rva, _) = (u32_at(&b, directories + 8)?, u32_at(&b, directories + 12)?);
    let mut descriptors = Vec::new();
    if import_rva != 0 {
        let mut at = rva_to_offset(&b, import_rva)?;
        loop {
            let d = b.get(at..at + 20).ok_or("Truncated import table")?;
            if d.iter().all(|&x| x == 0) {
                break;
            }
            descriptors.extend_from_slice(d);
            at += 20;
            if descriptors.len() > 20 * 4096 {
                return Err("Import table is not terminated".into());
            }
        }
    }
    // A signature would no longer match; drop it with its directory entry.
    let (security, security_size) = (u32_at(&b, directories + 32)?, u32_at(&b, directories + 36)?);
    if security != 0 && security_size != 0 && security as usize <= b.len() {
        b.truncate(security as usize);
        put32(&mut b, directories + 32, 0);
        put32(&mut b, directories + 36, 0);
    }
    // New section: descriptors, DLL name, hint/name, lookup table, address table.
    let last = section(sections - 1);
    let rva = align(
        u32_at(&b, last + 12)? + u32_at(&b, last + 8)?.max(u32_at(&b, last + 16)?),
        section_alignment,
    );
    let thunk = if plus { 8 } else { 4 };
    let mut data = vec![0u8; descriptors.len() + 40];
    let name_at = data.len();
    data.extend_from_slice(dll.as_bytes());
    data.push(0);
    data.resize(data.len().next_multiple_of(2), 0);
    let hint_at = data.len();
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(function.as_bytes());
    data.push(0);
    data.resize(data.len().next_multiple_of(8), 0);
    let lookup_at = data.len();
    data.extend(std::iter::repeat_n(0, thunk * 2));
    let address_at = data.len();
    data.extend(std::iter::repeat_n(0, thunk * 2));
    let rva_of = |at: usize| rva + at as u32;
    for at in [lookup_at, address_at] {
        put32(&mut data, at, rva_of(hint_at));
    }
    put32(&mut data, 0, rva_of(lookup_at));
    put32(&mut data, 12, rva_of(name_at));
    put32(&mut data, 16, rva_of(address_at));
    data[20..20 + descriptors.len()].copy_from_slice(&descriptors);
    let raw_size = align(data.len() as u32, file_alignment);
    let raw = align(b.len() as u32, file_alignment);
    b.resize(raw as usize, 0);
    data.resize(raw_size as usize, 0);
    b.extend_from_slice(&data);
    // Section header.
    b[header..header + 8].copy_from_slice(b".banana\0");
    put32(&mut b, header + 8, data.len() as u32);
    put32(&mut b, header + 12, rva);
    put32(&mut b, header + 16, raw_size);
    put32(&mut b, header + 20, raw);
    // Initialized data, readable and writable (the loader fills the IAT).
    put32(&mut b, header + 36, 0x4000_0040 | 0x8000_0000);
    b[coff + 2..coff + 4].copy_from_slice(&((sections + 1) as u16).to_le_bytes());
    put32(
        &mut b,
        optional + 56,
        align(rva + data.len() as u32, section_alignment),
    );
    put32(&mut b, optional + 64, 0); // checksum
    put32(&mut b, directories + 8, rva);
    put32(&mut b, directories + 12, (descriptors.len() + 40) as u32);
    // Bound imports describe the old table; the loader resolves instead.
    put32(&mut b, directories + 11 * 8, 0);
    put32(&mut b, directories + 11 * 8 + 4, 0);
    Ok(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_new_import_comes_first_and_the_old_ones_remain() {
        // Any PE on the build host works; this test binary is one on Windows.
        let Ok(original) = std::fs::read(std::env::current_exe().unwrap()) else {
            return;
        };
        if original.get(..2) != Some(b"MZ") {
            return;
        }
        let patched = add_import(&original, "banana_view.dll", "banana_view_attach").unwrap();
        let before = goblin::pe::PE::parse(&original).unwrap();
        let after = goblin::pe::PE::parse(&patched).unwrap();
        assert_eq!(after.imports[0].dll, "banana_view.dll");
        assert_eq!(after.imports[0].name, "banana_view_attach");
        let names = |pe: &goblin::pe::PE| {
            let mut v: Vec<_> = pe
                .imports
                .iter()
                .map(|i| (i.dll.to_owned(), i.name.to_string()))
                .collect();
            v.sort();
            v
        };
        let mut expected = names(&before);
        expected.push(("banana_view.dll".into(), "banana_view_attach".into()));
        expected.sort();
        assert_eq!(names(&after), expected);
        assert_eq!(after.sections.len(), before.sections.len() + 1);
        assert!(add_import(b"not a PE", "a.dll", "f").is_err());
    }
}
