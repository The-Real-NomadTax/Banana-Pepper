//! Bounded CSV field writer with a C-compatible ABI. ABI/behavior reference:
//! https://github.com/rgamble/libcsv/blob/master/csv.pdf
use std::ffi::c_void;

/// Write a quoted field, doubling embedded quotes. Return required size even
/// when truncated; no NUL terminator is added. Null source returns zero.
///
/// # Safety
/// Source must hold `src_size` bytes; a non-null destination must hold
/// `dest_size` writable bytes. The source and destination must not overlap.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn csv_write2(
    dest: *mut c_void,
    dest_size: usize,
    src: *const c_void,
    src_size: usize,
    quote: u8,
) -> usize {
    if src.is_null() {
        return 0;
    }
    let mut required = 0usize;
    let mut emit = |byte| {
        if !dest.is_null() && required < dest_size {
            unsafe { dest.cast::<u8>().add(required).write(byte) };
        }
        required = required.saturating_add(1);
    };
    emit(quote);
    for &byte in unsafe { std::slice::from_raw_parts(src.cast::<u8>(), src_size) } {
        if byte == quote {
            emit(quote);
        }
        emit(byte);
    }
    emit(quote);
    required
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn csv_abi_handles_quotes_truncation_and_size_queries() {
        let src = b"a\"b,\nc";
        let expected = b"\"a\"\"b,\nc\"";
        for capacity in 0..=expected.len() + 1 {
            let mut buffer = [0xcc; 32];
            let needed = unsafe {
                csv_write2(
                    buffer.as_mut_ptr().cast(),
                    capacity,
                    src.as_ptr().cast(),
                    src.len(),
                    b'"',
                )
            };
            assert_eq!(needed, expected.len());
            let written = capacity.min(expected.len());
            assert_eq!(&buffer[..written], &expected[..written]);
            assert_eq!(buffer[written], 0xcc);
        }
        assert_eq!(
            unsafe {
                csv_write2(
                    std::ptr::null_mut(),
                    100,
                    src.as_ptr().cast(),
                    src.len(),
                    b'"',
                )
            },
            expected.len()
        );
        assert_eq!(
            unsafe { csv_write2(std::ptr::null_mut(), 0, std::ptr::null(), 0, b'"') },
            0
        );
    }
}
