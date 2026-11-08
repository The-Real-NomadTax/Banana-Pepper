//! Tracee memory: reads through process_vm_readv (with a /proc/<tid>/mem
//! fallback) and writes through /proc/<tid>/mem, which also grows stacks.
use std::{fs::File, io, os::unix::fs::FileExt};

pub fn read(tid: i32, address: u64, buffer: &mut [u8]) -> io::Result<()> {
    let local = libc::iovec {
        iov_base: buffer.as_mut_ptr().cast(),
        iov_len: buffer.len(),
    };
    let remote = libc::iovec {
        iov_base: address as *mut libc::c_void,
        iov_len: buffer.len(),
    };
    // SAFETY: both vectors describe valid buffers of the stated length.
    let n = unsafe { libc::process_vm_readv(tid, &local, 1, &remote, 1, 0) };
    if n == buffer.len() as isize {
        return Ok(());
    }
    File::open(format!("/proc/{tid}/mem"))?.read_exact_at(buffer, address)
}

pub fn write(tid: i32, address: u64, bytes: &[u8]) -> io::Result<()> {
    File::options()
        .write(true)
        .open(format!("/proc/{tid}/mem"))?
        .write_all_at(bytes, address)
}

pub fn u64_at(tid: i32, address: u64) -> io::Result<u64> {
    let mut bytes = [0u8; 8];
    read(tid, address, &mut bytes)?;
    Ok(u64::from_ne_bytes(bytes))
}

/// A NUL-terminated string of at most `limit` bytes.
pub fn string(tid: i32, address: u64, limit: usize) -> io::Result<Vec<u8>> {
    if address == 0 {
        return Err(io::Error::from_raw_os_error(libc::EFAULT));
    }
    let mut out = Vec::new();
    let mut at = address;
    while out.len() < limit {
        // Never read across a page boundary in one go: the next page may
        // not exist.
        let chunk = (4096 - (at % 4096)) as usize;
        let mut bytes = vec![0u8; chunk.min(limit - out.len())];
        read(tid, at, &mut bytes)?;
        if let Some(end) = bytes.iter().position(|&b| b == 0) {
            out.extend_from_slice(&bytes[..end]);
            return Ok(out);
        }
        out.extend_from_slice(&bytes);
        at += bytes.len() as u64;
    }
    Err(io::Error::from_raw_os_error(libc::ENAMETOOLONG))
}

/// A NULL-terminated array of string pointers (argv).
pub fn strings(tid: i32, address: u64) -> io::Result<Vec<Vec<u8>>> {
    let mut out = Vec::new();
    if address == 0 {
        return Ok(out);
    }
    for index in 0..65536u64 {
        let pointer = u64_at(tid, address + index * 8)?;
        if pointer == 0 {
            return Ok(out);
        }
        out.push(string(tid, pointer, 128 * 1024)?);
    }
    Err(io::Error::from_raw_os_error(libc::E2BIG))
}

/// Builds data below the tracee's stack pointer, where the interrupted code
/// keeps nothing while it is stopped in a system call.
pub struct Scratch {
    tid: i32,
    next: u64,
    blocks: Vec<(u64, Vec<u8>)>,
}

impl Scratch {
    /// Room below `sp`, past the x86-64 red zone.
    pub fn new(tid: i32, sp: u64) -> Self {
        Self {
            tid,
            next: (sp - 256) & !15,
            blocks: Vec::new(),
        }
    }

    /// Place `data` (16-byte aligned) and return its tracee address.
    pub fn push(&mut self, data: &[u8]) -> u64 {
        self.next -= (data.len() as u64 + 15) & !15;
        self.blocks.push((self.next, data.to_vec()));
        self.next
    }

    pub fn push_str(&mut self, text: &[u8]) -> u64 {
        let mut data = text.to_vec();
        data.push(0);
        self.push(&data)
    }

    /// Write everything placed so far into the tracee.
    pub fn commit(&self) -> io::Result<()> {
        for (address, data) in &self.blocks {
            write(self.tid, *address, data)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn scratch_blocks_grow_down_from_below_the_stack() {
        let mut s = super::Scratch::new(0, 0x10000);
        let a = s.push_str(b"abc");
        let b = s.push(&[0; 17]);
        assert_eq!(a, 0x10000 - 256 - 16);
        assert_eq!(b, a - 32);
    }
}
