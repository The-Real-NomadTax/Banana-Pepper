use goblin::elf::{Elf, header, program_header};
use iced_x86::{Decoder, DecoderOptions, FlowControl, Mnemonic, Register, OpKind};
use serde::Serialize;
use std::collections::BTreeSet;

pub const MAX_IMAGE_BYTES: usize = 256 * 1024 * 1024;
const PAGE: u64 = 4096;

#[derive(Clone, Debug, Serialize)]
pub struct Segment {
    pub address: u64,
    pub memory_size: u64,
    pub file_offset: u64,
    pub file_size: u64,
    pub flags: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct Image {
    pub entry: u64,
    pub image_start: u64,
    pub image_end: u64,
    pub phdr: u64,
    pub phnum: u16,
    pub interpreter: Option<String>,
    pub libraries: Vec<String>,
    pub segments: Vec<Segment>,
    pub native_blockers: Vec<String>,
    pub syscalls: Vec<u64>,
    pub instructions: usize,
}

impl Image {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_IMAGE_BYTES { return Err("ELF exceeds the 256 MiB file limit".into()); }
        let elf = Elf::parse(bytes).map_err(|e| format!("Invalid ELF: {e}"))?;
        if !elf.is_64 || !elf.little_endian || elf.header.e_machine != header::EM_X86_64 {
            return Err("Expected little-endian x86-64 ELF".into());
        }
        if !matches!(elf.header.e_type, header::ET_EXEC | header::ET_DYN) {
            return Err("ELF must be an executable or PIE".into());
        }
        let mut blockers = Vec::new();
        if elf.header.e_type != header::ET_EXEC { blockers.push("PIE load bias and dynamic relocations are not implemented".into()); }
        if elf.interpreter.is_some() || elf.dynamic.is_some() {
            blockers.push("Linux dynamic linker, shared-library relocations and symbol versioning are not implemented".into());
        }
        if elf.program_headers.iter().any(|p| p.p_type == program_header::PT_TLS) {
            blockers.push("Linux TLS and FS-base translation are not implemented".into());
        }
        let mut segments = Vec::new();
        for p in &elf.program_headers {
            if p.p_type != program_header::PT_LOAD { continue; }
            if p.p_filesz > p.p_memsz || p.p_offset.checked_add(p.p_filesz).is_none_or(|end| end > bytes.len() as u64) {
                return Err("ELF LOAD extends beyond its file or memory bounds".into());
            }
            if p.p_align > 1 && (!p.p_align.is_power_of_two() || p.p_vaddr % p.p_align != p.p_offset % p.p_align) {
                return Err("Invalid ELF LOAD alignment".into());
            }
            if p.p_memsz == 0 { continue; }
            if p.p_vaddr < 0x10000 || p.p_vaddr.checked_add(p.p_memsz).is_none_or(|end| end >= 0x0000_7fff_0000_0000) {
                return Err("ELF LOAD address is outside the supported user address range".into());
            }
            if p.p_flags & 3 == 3 {
                blockers.push("Writable executable segments require dynamic code translation; direct execution is disabled".into());
            }
            segments.push(Segment { address: p.p_vaddr, memory_size: p.p_memsz, file_offset: p.p_offset, file_size: p.p_filesz, flags: p.p_flags });
        }
        segments.sort_by_key(|s| s.address);
        let first = segments.first().ok_or("ELF has no loadable segments")?;
        let image_start = first.address & !0xffff;
        let image_end = segments.iter().map(|s| (s.address + s.memory_size + PAGE - 1) & !(PAGE - 1)).max().unwrap();
        if image_end - image_start > MAX_IMAGE_BYTES as u64 { return Err("ELF address span exceeds 256 MiB".into()); }
        for pair in segments.windows(2) {
            if (pair[0].address + pair[0].memory_size + PAGE - 1) / PAGE > pair[1].address / PAGE {
                return Err("Overlapping ELF LOAD pages are unsupported".into());
            }
        }
        if !segments.iter().any(|s| s.flags & 1 != 0 && elf.entry >= s.address && elf.entry < s.address + s.file_size) {
            return Err("ELF entry is outside file-backed executable memory".into());
        }
        let phdr = segments.iter().find_map(|s| {
            let end = elf.header.e_phoff.checked_add(elf.header.e_phnum as u64 * elf.header.e_phentsize as u64)?;
            (elf.header.e_phoff >= s.file_offset && end <= s.file_offset + s.file_size)
                .then_some(s.address + elf.header.e_phoff - s.file_offset)
        }).unwrap_or(0);
        let mut syscalls = Vec::new();
        let mut instructions = 0;
        // A dynamic image is reportable, but cannot be instruction-certified by
        // this static backend: its executable segments can contain data/code.
        if blockers.is_empty() {
            let mut starts = BTreeSet::new();
            let mut branches = Vec::new();
            for segment in segments.iter().filter(|s| s.flags & 1 != 0) {
                if segment.memory_size != segment.file_size {
                    blockers.push("Executable BSS is unsupported".into());
                }
                let code = &bytes[segment.file_offset as usize..(segment.file_offset + segment.file_size) as usize];
                let mut decoder = Decoder::with_ip(64, code, segment.address, DecoderOptions::NONE);
                while decoder.can_decode() {
                    let instruction = decoder.decode();
                    if instruction.is_invalid() { blockers.push(format!("Undecodable executable bytes at {:#x}", instruction.ip())); break; }
                    starts.insert(instruction.ip());
                    instructions += 1;
                    if matches!(instruction.segment_prefix(), Register::FS | Register::GS) {
                        blockers.push(format!("Guest segment/TLS access at {:#x} requires translation", instruction.ip()));
                    }
                    match instruction.mnemonic() {
                        Mnemonic::Syscall => syscalls.push(instruction.ip()),
                        Mnemonic::Sysenter | Mnemonic::Sysexit | Mnemonic::Sysret | Mnemonic::Int | Mnemonic::Into | Mnemonic::Iret | Mnemonic::Iretd | Mnemonic::Iretq => {
                            blockers.push(format!("Unsupported kernel-entry instruction at {:#x}", instruction.ip()));
                        }
                        _ => {}
                    }
                    if instruction.op_count()>0
                        && matches!(instruction.op0_kind(),OpKind::NearBranch16|OpKind::NearBranch32|OpKind::NearBranch64)
                        && matches!(instruction.flow_control(), FlowControl::UnconditionalBranch | FlowControl::ConditionalBranch | FlowControl::Call) {
                        branches.push(instruction.near_branch_target());
                    }
                }
                for (offset, pair) in code.windows(2).enumerate() {
                    if pair == [0x0f, 0x05] && syscalls.binary_search(&(segment.address + offset as u64)).is_err() {
                        blockers.push(format!("Ambiguous syscall bytes at {:#x}; refusing an unsafe rewrite", segment.address + offset as u64));
                    }
                }
            }
            if !starts.contains(&elf.entry) { blockers.push("Entry is not an instruction boundary".into()); }
            if branches.iter().any(|target| !starts.contains(target)) {
                blockers.push("A direct branch targets an uncertified instruction boundary".into());
            }
        }
        blockers.sort(); blockers.dedup();
        Ok(Self { entry: elf.entry, image_start, image_end, phdr, phnum: elf.header.e_phnum,
            interpreter: elf.interpreter.map(str::to_owned), libraries: elf.libraries.iter().map(|s| s.to_string()).collect(),
            segments, native_blockers: blockers, syscalls, instructions })
    }

    pub fn require_native_support(&self) -> Result<(), String> {
        if self.native_blockers.is_empty() { Ok(()) }
        else { Err(format!("Native Linux execution is unsupported for this image:\n- {}", self.native_blockers.join("\n- "))) }
    }
}
