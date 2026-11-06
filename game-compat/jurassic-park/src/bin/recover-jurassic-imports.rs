//! Recover named C imports from a fingerprint-matched reference ELF; never execute it.
use goblin::elf::{Elf, sym::STT_FUNC};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

fn offset(elf: &Elf<'_>, address: u64) -> Result<usize, String> {
    elf.program_headers
        .iter()
        .find_map(|p| {
            (p.p_type == 1 && address >= p.p_vaddr && address < p.p_vaddr + p.p_filesz)
                .then(|| (p.p_offset + address - p.p_vaddr) as usize)
        })
        .ok_or_else(|| format!("Address {address:#x} is outside file-backed LOAD segments"))
}
fn slot(bytes: &[u8], elf: &Elf<'_>, plt: u32) -> Result<(u32, u32), String> {
    let at = offset(elf, plt.into())?;
    if bytes.get(at..at + 2) != Some(&[0xff, 0x25]) {
        return Err(format!("No absolute-jump PLT at {plt:#x}"));
    }
    let got = u32::from_le_bytes(bytes[at + 2..at + 6].try_into().unwrap());
    let at = offset(elf, got.into())?;
    Ok((
        got,
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()),
    ))
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if !(3..=4).contains(&args.len()) {
        return Err("Usage: recover-jurassic-imports <supplied-game_legacy_patched> <reference-library.so> <output.rs> [matching-debug-library.so]".into());
    }
    let snapshot = fs::read(&args[0])?;
    if format!("{:x}", Sha256::digest(&snapshot)) != banana_game_jurassic_park::prepare::Build::Legacy.sha256() {
        return Err("Unrecognized Jurassic snapshot".into());
    }
    let reference = fs::read(&args[1])?;
    let image = Elf::parse(&snapshot)?;
    let library = Elf::parse(&reference)?;
    if image.is_64 || library.is_64 || library.header.e_machine != 3 {
        return Err("Both images must be ELF32/i386".into());
    }
    let graphics_dispatch = library
        .dynsyms
        .iter()
        .any(|s| library.dynstrtab.get_at(s.st_name) == Some("glGetString"));
    let symbols: Vec<_> = library
        .dynsyms
        .iter()
        .filter(|s| s.st_type() == STT_FUNC && s.st_value != 0)
        .filter_map(|s| {
            library
                .dynstrtab
                .get_at(s.st_name)
                .filter(|name| {
                    !graphics_dispatch
                        || (s.st_size == 0
                            && name.starts_with("gl")
                            && !name.starts_with("glX")
                            && !["ARB", "EXT", "NV", "SGI", "ATI", "AMD", "APPLE", "OES"]
                                .iter()
                                .any(|suffix| name.ends_with(suffix)))
                })
                .map(|name| (name, s.st_value as u32))
        })
        .collect();
    let (base, anchor_count) = if symbols.iter().any(|s| s.0 == "__libc_start_main") {
        let mut bases = Vec::new();
        for (plt, name) in [
            (0x8139800, "__libc_start_main"),
            (0x813aa70, "getenv"),
            (0x813aec0, "__cxa_atexit"),
            (0x813a8c0, "isalpha"),
        ] {
            let relative = symbols
                .iter()
                .find(|s| s.0 == name)
                .ok_or("Missing fingerprint export")?
                .1;
            bases.push(
                slot(&snapshot, &image, plt)?
                    .1
                    .checked_sub(relative)
                    .ok_or("Invalid reference base")?,
            );
        }
        if bases.iter().any(|&b| b != bases[0]) || !bases[0].is_multiple_of(4096) {
            return Err(
                "Reference library does not match all four independent import fingerprints".into(),
            );
        }
        (bases[0], 4)
    } else if symbols.iter().any(|s| s.0 == "SDL_Init") {
        // Engine startup flags and SysGraphSetVideoMode's argument sequences.
        let mut bases = Vec::new();
        for (plt, name) in [
            (0x813bab0, "SDL_Init"),
            (0x813b5b0, "SDL_GL_SetAttribute"),
            (0x813a450, "SDL_SetVideoMode"),
            (0x813a560, "SDL_WM_SetCaption"),
        ] {
            let relative = symbols
                .iter()
                .find(|s| s.0 == name)
                .ok_or("Missing SDL fingerprint export")?
                .1;
            bases.push(
                slot(&snapshot, &image, plt)?
                    .1
                    .checked_sub(relative)
                    .ok_or("Invalid SDL reference base")?,
            );
        }
        if bases.iter().any(|&b| b != bases[0]) || !bases[0].is_multiple_of(4096) {
            return Err("Reference library does not match all four SDL fingerprints".into());
        }
        (bases[0], 4)
    } else if graphics_dispatch {
        // Only the fixed-stride OpenGL dispatch stubs, not vendor GLX functions.
        // These calls are independently identified by VidInit's GL enum arguments.
        let mut bases = Vec::new();
        for (plt, name) in [
            (0x81399c0, "glHint"),
            (0x813a290, "glGetString"),
            (0x813b3c0, "glLightModeli"),
            (0x8139f90, "glFrontFace"),
            (0x813b2e0, "glDisable"),
            (0x8139840, "glClearDepth"),
            (0x813ae40, "glClearStencil"),
            (0x813b000, "glGetIntegerv"),
            (0x8139d30, "glCreateShader"),
            (0x8139890, "glCompileShader"),
            (0x813b080, "glShaderSource"),
            (0x813a0d0, "glGetUniformLocation"),
            (0x813b9d0, "glGetShaderSource"),
        ] {
            let relative = symbols
                .iter()
                .find(|s| s.0 == name)
                .ok_or("Missing OpenGL dispatch fingerprint")?
                .1;
            bases.push(
                slot(&snapshot, &image, plt)?
                    .1
                    .checked_sub(relative)
                    .ok_or("Invalid OpenGL dispatch reference base")?,
            );
        }
        if bases.iter().any(|&b| b != bases[0]) || !bases[0].is_multiple_of(4096) {
            return Err(
                "Reference dispatch layout does not match all thirteen OpenGL fingerprints".into(),
            );
        }
        (bases[0], 13)
    } else if symbols.iter().any(|s| s.0 == "tanf") {
        // VecInit builds sine/cosine/tangent tables through these three PLTs.
        // The saved sine/cosine targets are optimized IFUNC implementations.
        let debug_path = args
            .get(3)
            .ok_or("libm recovery needs matching debug symbols")?;
        let debug_bytes = fs::read(debug_path)?;
        let debug = Elf::parse(&debug_bytes)?;
        let mut bases = Vec::new();
        for (plt, name) in [
            (0x8139880, "tanf"),
            (0x813b480, "__sinf_sse2"),
            (0x8139f10, "__cosf_sse2"),
        ] {
            let relative = debug
                .syms
                .iter()
                .find(|s| s.st_value != 0 && debug.strtab.get_at(s.st_name) == Some(name))
                .ok_or("Missing VecInit math fingerprint")?
                .st_value as u32;
            bases.push(
                slot(&snapshot, &image, plt)?
                    .1
                    .checked_sub(relative)
                    .ok_or("Invalid math reference base")?,
            );
        }
        if bases.iter().any(|&b| b != bases[0]) || !bases[0].is_multiple_of(4096) {
            return Err(
                "Reference library does not match all three VecInit math fingerprints".into(),
            );
        }
        (bases[0], 3)
    } else {
        let mut candidates: BTreeMap<u32, BTreeSet<&str>> = BTreeMap::new();
        for relocation in image.pltrelocs.iter() {
            let symbol = image
                .dynsyms
                .get(relocation.r_sym)
                .ok_or("Bad relocation symbol index")?;
            let name = image
                .dynstrtab
                .get_at(symbol.st_name)
                .ok_or("Bad relocation name")?;
            let at = offset(&image, relocation.r_offset)?;
            let saved = u32::from_le_bytes(snapshot[at..at + 4].try_into().unwrap());
            for (_, relative) in symbols.iter().filter(|s| s.0 == name) {
                if let Some(base) = saved
                    .checked_sub(*relative)
                    .filter(|b| *b > 0x10000000 && b.is_multiple_of(4096))
                {
                    candidates.entry(base).or_default().insert(name);
                }
            }
        }
        let mut candidates: Vec<_> = candidates.into_iter().collect();
        candidates.sort_by_key(|(_, names)| std::cmp::Reverse(names.len()));
        let best = candidates
            .first()
            .ok_or("No reference-library fingerprints matched")?;
        if best.1.len() < 4
            || candidates
                .get(1)
                .is_some_and(|other| other.1.len() >= best.1.len())
        {
            return Err("Reference library base is insufficiently verified or ambiguous".into());
        }
        (best.0, best.1.len())
    };
    let mut names: BTreeMap<u32, Vec<&str>> = BTreeMap::new();
    for (name, value) in symbols {
        if name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            names.entry(value).or_default().push(name);
        }
    }
    if let Some(debug_path) = args.get(3) {
        let debug_bytes = fs::read(debug_path)?;
        let debug = Elf::parse(&debug_bytes)?;
        let build_id = |elf: &Elf<'_>, bytes: &[u8]| {
            elf.iter_note_headers(bytes).and_then(|mut notes| {
                notes.find_map(|n| {
                    n.ok()
                        .filter(|n| n.n_type == 3 && n.name == "GNU")
                        .map(|n| n.desc.to_vec())
                })
            })
        };
        let reference_id =
            build_id(&library, &reference).ok_or("Reference library has no GNU build ID")?;
        if build_id(&debug, &debug_bytes) != Some(reference_id) {
            return Err("Debug symbols do not match the reference library build ID".into());
        }
        let apis: Vec<_> = library
            .dynsyms
            .iter()
            .filter(|s| s.st_type() == 10)
            .filter_map(|s| library.dynstrtab.get_at(s.st_name))
            .collect();
        for symbol in debug
            .syms
            .iter()
            .filter(|s| s.st_type() == STT_FUNC && s.st_value != 0)
        {
            if let Some(name) = debug.strtab.get_at(symbol.st_name) {
                let best = apis
                    .iter()
                    .filter(|api| {
                        let stem = if api.starts_with("__") {
                            format!("{api}_")
                        } else {
                            format!("__{api}_")
                        };
                        name.starts_with(&stem)
                    })
                    .max_by_key(|api| api.len());
                if let Some(api) = best {
                    names.entry(symbol.st_value as u32).or_default().push(api);
                }
            }
        }
        // `names` borrows the parsed debug symbols' canonical API names from `library`.
    }
    let mut bindings = Vec::new();
    for plt in (0x8139740..0x813bb60).step_by(16) {
        let (got, saved) = slot(&snapshot, &image, plt)?;
        if let Some(candidates) = saved.checked_sub(base).and_then(|r| names.get(&r)) {
            let mut candidates = candidates.clone();
            candidates.sort_by_key(|s| (s.starts_with('_'), s.len(), *s));
            bindings.push((plt, got, saved, candidates[0]));
        }
    }
    let hash = format!("{:x}", Sha256::digest(&reference));
    let mut output = format!(
        "// Generated by recover-jurassic-imports.\n// Reference SHA256: {hash}\n// Saved library base: {base:#x}; {anchor_count} independent anchors verified.\n&[\n"
    );
    for (plt, got, saved, name) in &bindings {
        output.push_str(&format!("    Binding {{ plt: {plt:#010x}, got: {got:#010x}, saved: {saved:#010x}, symbol: c\"{name}\" }},\n"));
    }
    output.push_str("]\n");
    fs::write(Path::new(&args[2]), output)?;
    println!(
        "Recovered {} exact-offset bindings; reference SHA256 {hash}",
        bindings.len()
    );
    Ok(())
}
