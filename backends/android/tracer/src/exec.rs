//! What to execute for a guest `execve`. The kernel would resolve an ELF
//! interpreter or a `#!` interpreter on the host, so the tracer resolves them
//! in the guest instead: dynamic ELF programs run through their own guest
//! `ld.so` (`ld.so --argv0 NAME PROGRAM ARGS`), scripts through their guest
//! interpreter. No loader binary is involved, and a dynamic program served by
//! the view is opened by `ld.so` like any other guest file.

/// Where programs come from: the guest root or the view.
pub trait Programs {
    /// The first bytes of a guest file (up to `limit`).
    fn head(&self, guest: &str, limit: usize) -> Result<Vec<u8>, i32>;
    /// A host path the kernel can execute for a guest file.
    fn host(&self, guest: &str) -> Result<std::path::PathBuf, i32>;
}

pub struct Plan {
    /// Host path handed to the kernel.
    pub host: std::path::PathBuf,
    pub argv: Vec<Vec<u8>>,
    /// Guest path of the program that ends up running (for /proc/self/exe).
    pub program: String,
}

/// Program headers are expected within the first 64 KiB.
const HEAD: usize = 65536;

/// `PT_INTERP` of an ELF image, if it has one.
fn interpreter(image: &[u8]) -> Result<Option<String>, i32> {
    let wide = match image.get(4) {
        Some(2) => true,
        Some(1) => false,
        _ => return Err(libc::ENOEXEC),
    };
    let word = |o: u64, n: usize| -> Result<u64, i32> {
        let o = o as usize;
        let bytes = image.get(o..o + n).ok_or(libc::ENOEXEC)?;
        let mut v = [0u8; 8];
        v[..n].copy_from_slice(bytes);
        Ok(u64::from_le_bytes(v))
    };
    let (phoff, phentsize, phnum) = if wide {
        (word(32, 8)?, word(54, 2)?, word(56, 2)?)
    } else {
        (word(28, 4)?, word(42, 2)?, word(44, 2)?)
    };
    for i in 0..phnum.min(256) {
        let ph = phoff + i * phentsize;
        if word(ph, 4)? != 3 {
            continue; // PT_INTERP
        }
        let (offset, length) = if wide {
            (word(ph + 8, 8)?, word(ph + 32, 8)?)
        } else {
            (word(ph + 4, 4)?, word(ph + 16, 4)?)
        };
        if length == 0 || length > 4096 {
            return Err(libc::ENOEXEC);
        }
        let name = image
            .get(offset as usize..(offset + length) as usize)
            .ok_or(libc::ENOEXEC)?;
        let name = name.split(|&b| b == 0).next().unwrap_or_default();
        return Ok(Some(String::from_utf8_lossy(name).into_owned()));
    }
    Ok(None)
}

/// Plan the execution of guest `program` (absolute) with `argv`.
pub fn plan(programs: &dyn Programs, program: &str, argv: Vec<Vec<u8>>) -> Result<Plan, i32> {
    let mut program = program.to_owned();
    let mut argv = argv;
    for _ in 0..4 {
        let head = programs.head(&program, HEAD)?;
        if head.starts_with(b"#!") {
            let line = head[2..]
                .split(|&b| b == b'\n')
                .next()
                .ok_or(libc::ENOEXEC)?;
            let text = String::from_utf8_lossy(&line[..line.len().min(256)]);
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
        if !head.starts_with(b"\x7fELF") || head.len() < 64 {
            return Err(libc::ENOEXEC);
        }
        return Ok(match interpreter(&head)? {
            None => Plan {
                host: programs.host(&program)?,
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
                    host: programs.host(&loader)?,
                    argv: next,
                    program,
                }
            }
        });
    }
    Err(libc::ELOOP)
}
