//! Win32 private-profile (INI) semantics over file bytes, for profile calls
//! on view files: lookups are ASCII case-insensitive, values are trimmed and
//! unquoted, and writes keep the rest of the file as it was.

fn trim(b: &[u8]) -> &[u8] {
    let start = b.iter().position(|c| !c.is_ascii_whitespace()).unwrap_or(b.len());
    let end = b.iter().rposition(|c| !c.is_ascii_whitespace()).map_or(start, |i| i + 1);
    &b[start..end]
}
fn same(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}
fn lines(text: &[u8]) -> Vec<&[u8]> {
    text.split(|&c| c == b'\n')
        .map(|l| l.strip_suffix(b"\r").unwrap_or(l))
        .collect()
}
fn section_name(line: &[u8]) -> Option<&[u8]> {
    let line = trim(line);
    line.strip_prefix(b"[")
        .and_then(|l| l.iter().position(|&c| c == b']').map(|i| trim(&l[..i])))
}
fn entry(line: &[u8]) -> Option<(&[u8], &[u8])> {
    let line = trim(line);
    if line.starts_with(b";") || line.starts_with(b"[") {
        return None;
    }
    let at = line.iter().position(|&c| c == b'=')?;
    Some((trim(&line[..at]), trim(&line[at + 1..])))
}
fn unquote(v: &[u8]) -> &[u8] {
    if v.len() >= 2 && (v[0] == b'"' || v[0] == b'\'') && v[v.len() - 1] == v[0] {
        &v[1..v.len() - 1]
    } else {
        v
    }
}

/// Section names, or the keys of `section`, in file order.
pub fn names(text: &[u8], section: Option<&[u8]>) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in lines(text) {
        if let Some(name) = section_name(line) {
            inside = section.is_some_and(|s| same(s, name));
            if section.is_none() {
                out.push(name.to_vec());
            }
        } else if inside {
            if let Some((key, _)) = entry(line) {
                out.push(key.to_vec());
            }
        }
    }
    out
}

/// The value of `key` in `section`.
pub fn value(text: &[u8], section: &[u8], key: &[u8]) -> Option<Vec<u8>> {
    let mut inside = false;
    for line in lines(text) {
        if let Some(name) = section_name(line) {
            inside = same(section, name);
        } else if inside {
            if let Some((k, v)) = entry(line) {
                if same(k, key) {
                    return Some(unquote(v).to_vec());
                }
            }
        }
    }
    None
}

/// The file after setting (`Some`) or deleting (`None`) `key`; with no key,
/// the whole section is deleted.
pub fn set(text: &[u8], section: &[u8], key: Option<&[u8]>, new: Option<&[u8]>) -> Vec<u8> {
    let crlf = text.windows(2).any(|w| w == b"\r\n") || text.is_empty();
    let mut out: Vec<Vec<u8>> = lines(text).into_iter().map(<[u8]>::to_vec).collect();
    if out.last().is_some_and(Vec::is_empty) {
        out.pop();
    }
    let header = out
        .iter()
        .position(|l| section_name(l).is_some_and(|n| same(n, section)));
    let end = |from: usize, out: &Vec<Vec<u8>>| {
        (from + 1..out.len())
            .find(|&i| section_name(&out[i]).is_some())
            .unwrap_or(out.len())
    };
    match (header, key) {
        (None, None) => {}
        (Some(h), None) => {
            let e = end(h, &out);
            out.drain(h..e);
        }
        (None, Some(key)) => {
            if let Some(new) = new {
                out.push([b"[", section, b"]"].concat());
                out.push([key, b"=", new].concat());
            }
        }
        (Some(h), Some(key)) => {
            let e = end(h, &out);
            let at = (h + 1..e).find(|&i| entry(&out[i]).is_some_and(|(k, _)| same(k, key)));
            match (at, new) {
                (Some(i), Some(new)) => out[i] = [key, b"=", new].concat(),
                (Some(i), None) => {
                    out.remove(i);
                }
                (None, Some(new)) => {
                    // After the section's last entry, before trailing blanks.
                    let mut i = e;
                    while i > h + 1 && trim(&out[i - 1]).is_empty() {
                        i -= 1;
                    }
                    out.insert(i, [key, b"=", new].concat());
                }
                (None, None) => {}
            }
        }
    }
    let newline: &[u8] = if crlf { b"\r\n" } else { b"\n" };
    let mut bytes = Vec::new();
    for line in out {
        bytes.extend_from_slice(&line);
        bytes.extend_from_slice(newline);
    }
    bytes
}

/// Copy a string result into a caller buffer of `size` units the way
/// GetPrivateProfileString does; returns the count it reports.
pub fn copy_string(value: &[u8], out: &mut [u8]) -> usize {
    if out.is_empty() {
        return 0;
    }
    let n = value.len().min(out.len() - 1);
    out[..n].copy_from_slice(&value[..n]);
    out[n] = 0;
    n
}
/// Copy a name list (each NUL-terminated, then a final NUL).
pub fn copy_list(names: &[Vec<u8>], out: &mut [u8]) -> usize {
    if out.len() < 2 {
        if let Some(first) = out.first_mut() {
            *first = 0;
        }
        return 0;
    }
    let mut at = 0;
    for name in names {
        if at + name.len() + 2 > out.len() {
            // Truncated: what fits, double-terminated; reports size - 2.
            let n = (out.len() - 2 - at).min(name.len());
            out[at..at + n].copy_from_slice(&name[..n]);
            out[at + n] = 0;
            out[out.len() - 1] = 0;
            return out.len() - 2;
        }
        out[at..at + name.len()].copy_from_slice(name);
        at += name.len();
        out[at] = 0;
        at += 1;
    }
    out[at] = 0;
    at
}
/// GetPrivateProfileInt's parse: leading decimal digits, optional sign.
pub fn int(value: &[u8], default: i32) -> u32 {
    let v = trim(value);
    let (negative, digits) = match v.first() {
        Some(b'-') => (true, &v[1..]),
        Some(b'+') => (false, &v[1..]),
        _ => (false, v),
    };
    let digits: Vec<u8> = digits.iter().copied().take_while(u8::is_ascii_digit).collect();
    if digits.is_empty() {
        return if v.is_empty() { default as u32 } else { 0 };
    }
    let n = std::str::from_utf8(&digits)
        .ok()
        .and_then(|d| d.parse::<i64>().ok())
        .unwrap_or(0);
    (if negative { -n } else { n }) as i32 as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    const TEXT: &[u8] = b"[Video]\r\nResolution = 800x600\r\nFullscreen=on\r\n\r\n[Audio]\r\nMusicVolume=\"7\"\r\n";
    #[test]
    fn reads_values_names_and_ints() {
        assert_eq!(value(TEXT, b"video", b"RESOLUTION").unwrap(), b"800x600");
        assert_eq!(value(TEXT, b"Audio", b"MusicVolume").unwrap(), b"7");
        assert!(value(TEXT, b"Audio", b"Missing").is_none());
        assert_eq!(names(TEXT, None), [b"Video".to_vec(), b"Audio".to_vec()]);
        assert_eq!(names(TEXT, Some(b"VIDEO")), [b"Resolution".to_vec(), b"Fullscreen".to_vec()]);
        assert_eq!(int(b" 42abc", 5), 42);
        assert_eq!(int(b"", 5), 5);
        assert_eq!(int(b"-3", 5), (-3i32) as u32);
    }
    #[test]
    fn writes_keep_the_rest_of_the_file() {
        let t = set(TEXT, b"VIDEO", Some(b"Fullscreen"), Some(b"off"));
        assert_eq!(value(&t, b"Video", b"Fullscreen").unwrap(), b"off");
        assert_eq!(value(&t, b"Video", b"Resolution").unwrap(), b"800x600");
        let t = set(&t, b"Video", Some(b"VSync"), Some(b"1"));
        assert_eq!(names(&t, Some(b"Video")).len(), 3);
        assert!(t.windows(2).any(|w| w == b"\r\n"));
        let t = set(&t, b"Audio", None, None);
        assert_eq!(names(&t, None), [b"Video".to_vec()]);
        let t = set(&t, b"New", Some(b"k"), Some(b"v"));
        assert_eq!(value(&t, b"new", b"K").unwrap(), b"v");
        let t = set(&t, b"Video", Some(b"VSync"), None);
        assert!(value(&t, b"Video", b"VSync").is_none());
    }
    #[test]
    fn copies_like_win32() {
        let mut b = [9u8; 5];
        assert_eq!(copy_string(b"abcdef", &mut b), 4);
        assert_eq!(&b, b"abcd\0");
        let mut b = [9u8; 16];
        assert_eq!(copy_list(&[b"ab".to_vec(), b"cd".to_vec()], &mut b), 6);
        assert_eq!(&b[..7], b"ab\0cd\0\0");
        let mut b = [9u8; 5];
        assert_eq!(copy_list(&[b"abcdef".to_vec()], &mut b), 3);
        assert_eq!(&b, b"abc\0\0");
    }
}
