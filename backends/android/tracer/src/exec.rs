//! What to execute for a guest `execve`. The kernel would resolve an ELF
//! interpreter or a `#!` interpreter on the host, so the tracer resolves them
//! in the guest instead: dynamic ELF programs run through their own guest
//! `ld.so` (`ld.so --argv0 NAME PROGRAM ARGS`), scripts through their guest
//! interpreter. No loader binary is involved.
use crate::path::Fs;
use std::{fs::File, io::Read};

pub struct Plan {
    /// Host path handed to the kernel.
    pub host: std::path::PathBuf,
    pub argv: Vec<Vec<u8>>,
    /// Guest path of the program that ends up running (for /proc/self/exe).
    pub program: String,
}

/// `PT_INTERP` of an ELF image, if it has one.
fn interpreter(header: &[u8], file: &mut File) -> Result<Option<String>, i32> {
    use std::os::unix::fs::FileExt;
    let wide = match header.get(4) {
        Some(2) => true,
        Some(1) => false,
        _ => return Err(libc::ENOEXEC),
    };
    let word = |o: usize, n: usize| -> u64 {
        let mut v = [0u8; 8];
        v[..n].copy_from_slice(&header[o..o + n]);
        u64::from_le_bytes(v)
    };
    let (phoff, phentsize, phnum) = if wide {
        (word(32, 8), word(54, 2), word(56, 2))
    } else {
        (word(28, 4), word(42, 2), word(44, 2))
    };
    for i in 0..phnum.min(256) {
        let mut ph = [0u8; 56];
        let size = phentsize.min(56) as usize;
        file.read_exact_at(&mut ph[..size], phoff + i * phentsize)
            .map_err(|_| libc::ENOEXEC)?;
        let field = |o: usize, n: usize| -> u64 {
            let mut v = [0u8; 8];
            v[..n].copy_from_slice(&ph[o..o + n]);
            u64::from_le_bytes(v)
        };
        if field(0, 4) != 3 {
            continue; // PT_INTERP
        }
        let (offset, length) = if wide {
            (field(8, 8), field(32, 8))
        } else {
            (field(4, 4), field(16, 4))
        };
        if length == 0 || length > 4096 {
            return Err(libc::ENOEXEC);
        }
        let mut name = vec![0u8; length as usize];
        file.read_exact_at(&mut name, offset)
            .map_err(|_| libc::ENOEXEC)?;
        let name = name.split(|&b| b == 0).next().unwrap_or_default();
        return Ok(Some(String::from_utf8_lossy(name).into_owned()));
    }
    Ok(None)
}

/// Plan the execution of guest `program` (absolute) with `argv`.
pub fn plan(fs: &Fs, program: &str, argv: Vec<Vec<u8>>, tgid: i32, tid: i32) -> Result<Plan, i32> {
    let mut program = program.to_owned();
    let mut argv = argv;
    for _ in 0..4 {
        let host = fs.translate(&program, true, tgid, tid)?;
        let mut file = File::open(&host).map_err(|e| e.raw_os_error().unwrap_or(libc::ENOENT))?;
        if file.metadata().is_ok_and(|m| m.is_dir()) {
            return Err(libc::EACCES);
        }
        let mut header = vec![0u8; 256];
        let n = file.read(&mut header).map_err(|_| libc::EIO)?;
        header.truncate(n);
        if header.starts_with(b"#!") {
            let line = header[2..]
                .split(|&b| b == b'\n')
                .next()
                .ok_or(libc::ENOEXEC)?;
            let text = String::from_utf8_lossy(line);
            let text = text.trim();
            let (interp, arg) = match text.split_once([' ', '\t']) {
                Some((i, a)) => (i.to_owned(), Some(a.trim().to_owned())),
                None => (text.to_owned(), None),
            };
            if !interp.starts_with('/') {
                return Err(libc::ENOEXEC);
            }
            let mut next = vec![interp.clone().into_bytes()];
            next.extend(arg.map(String::into_bytes));
            next.push(program.clone().into_bytes());
            next.extend(argv.into_iter().skip(1));
            argv = next;
            program = interp;
            continue;
        }
        if !header.starts_with(b"\x7fELF") || header.len() < 64 {
            return Err(libc::ENOEXEC);
        }
        return Ok(match interpreter(&header, &mut file)? {
            None => Plan {
                host,
                argv,
                program,
            },
            Some(loader) => {
                let name = argv
                    .first()
                    .cloned()
                    .unwrap_or_else(|| program.clone().into_bytes());
                let mut next = vec![loader.clone().into_bytes(), b"--argv0".to_vec(), name];
                next.push(program.clone().into_bytes());
                next.extend(argv.into_iter().skip(1));
                Plan {
                    host: fs.translate(&loader, true, tgid, tid)?,
                    argv: next,
                    program,
                }
            }
        });
    }
    Err(libc::ELOOP)
}
