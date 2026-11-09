//! The emulated terminal's certificates.
//!
//! Drive cabinets reach the game server through their Wangan terminal over
//! TLS and accept the terminal's certificate only when the game's CA file
//! (`data/network/certs/v388-ca-cert.pem`) signed it. The emulation has its
//! own CA: the plan serves [`ca_pem`] at that path in the session view (the
//! source stays untouched), and the hook's terminal presents [`terminal_der`]
//! signed by it. Both are built here from fixed P-256 keys, deterministically
//! (RFC 6979 signatures), so the plan and the hook agree byte for byte. The
//! keys only make a local emulation trusted by a local game; they are not
//! secrets.
//!
//! No allocation and no `std`, so the hook builds the same bytes.
use p256::ecdsa::{Signature, SigningKey, signature::Signer};
use sha2::{Digest, Sha256};

/// The private key from a fixed label.
fn key(label: &[u8]) -> SigningKey {
    let mut seed: [u8; 32] = Sha256::digest(label).into();
    loop {
        if let Ok(key) = SigningKey::from_bytes(&seed.into()) {
            return key;
        }
        seed = Sha256::digest(seed).into();
    }
}

pub fn ca_key() -> SigningKey {
    key(b"Banana Pepper wmmt6r emulated terminal CA")
}

/// The terminal's private key (its certificate's).
pub fn terminal_key() -> SigningKey {
    key(b"Banana Pepper wmmt6r emulated terminal")
}

/// A DER writer over a fixed buffer. Values are written front to back;
/// [`Der::wrap`] turns everything written since a mark into one TLV.
pub struct Der<'a> {
    buffer: &'a mut [u8],
    len: usize,
}

impl<'a> Der<'a> {
    pub fn new(buffer: &'a mut [u8]) -> Self {
        Self { buffer, len: 0 }
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn bytes(&self) -> &[u8] {
        &self.buffer[..self.len]
    }
    pub fn raw(&mut self, bytes: &[u8]) -> &mut Self {
        self.buffer[self.len..self.len + bytes.len()].copy_from_slice(bytes);
        self.len += bytes.len();
        self
    }
    /// The content written since `mark` becomes the value of a `tag` TLV.
    pub fn wrap(&mut self, mark: usize, tag: u8) -> &mut Self {
        let content = self.len - mark;
        let header: &[u8] = match content {
            0..=0x7f => &[tag, content as u8],
            0x80..=0xff => &[tag, 0x81, content as u8],
            _ => &[tag, 0x82, (content >> 8) as u8, content as u8],
        };
        let h = header.len();
        self.buffer.copy_within(mark..self.len, mark + h);
        self.buffer[mark..mark + h].copy_from_slice(header);
        self.len += h;
        self
    }
    pub fn tlv(&mut self, tag: u8, value: &[u8]) -> &mut Self {
        let mark = self.len;
        self.raw(value).wrap(mark, tag)
    }
    /// A non-negative INTEGER from big-endian bytes.
    pub fn uint(&mut self, value: &[u8]) -> &mut Self {
        let mut v = value;
        while v.len() > 1 && v[0] == 0 {
            v = &v[1..];
        }
        let mark = self.len;
        if v[0] & 0x80 != 0 {
            self.raw(&[0]);
        }
        self.raw(v).wrap(mark, 0x02)
    }
}

const SEQUENCE: u8 = 0x30;
const SET: u8 = 0x31;
/// ecdsa-with-SHA256 (1.2.840.10045.4.3.2).
const ECDSA_SHA256: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02];
/// id-ecPublicKey (1.2.840.10045.2.1) and prime256v1 (1.2.840.10045.3.1.7).
const EC_PUBLIC_KEY: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01];
const PRIME256V1: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07];
/// id-at-commonName, id-at-organizationName.
const COMMON_NAME: &[u8] = &[0x55, 0x04, 0x03];
const ORGANIZATION: &[u8] = &[0x55, 0x04, 0x0a];
/// id-ce-basicConstraints, id-ce-keyUsage.
const BASIC_CONSTRAINTS: &[u8] = &[0x55, 0x1d, 0x13];
const KEY_USAGE: &[u8] = &[0x55, 0x1d, 0x0f];

fn name(d: &mut Der, common_name: &[u8]) {
    let name = d.len();
    for (oid, value) in [(ORGANIZATION, &b"Banana Pepper"[..]), (COMMON_NAME, common_name)] {
        let set = d.len();
        let attribute = d.len();
        d.tlv(0x06, oid).tlv(0x0c, value).wrap(attribute, SEQUENCE).wrap(set, SET);
    }
    d.wrap(name, SEQUENCE);
}

fn algorithm(d: &mut Der) {
    let m = d.len();
    d.tlv(0x06, ECDSA_SHA256).wrap(m, SEQUENCE);
}

/// A DER ECDSA-Sig-Value from a signature.
pub fn signature_der(signature: &Signature, out: &mut [u8]) -> usize {
    let (r, s) = signature.split_bytes();
    let mut d = Der::new(out);
    d.uint(&r).uint(&s).wrap(0, SEQUENCE);
    d.len()
}

/// A certificate for `subject`'s key, issued by the CA (self-signed when
/// `subject` is the CA).
fn certificate(serial: u8, issuer: &[u8], subject_name: &[u8], subject: &SigningKey, ca: bool, out: &mut [u8]) -> usize {
    let mut tbs = [0u8; 600];
    let mut d = Der::new(&mut tbs);
    // Version 3.
    let v = d.len();
    d.tlv(0x02, &[2]).wrap(v, 0xa0);
    d.tlv(0x02, &[serial]);
    algorithm(&mut d);
    name(&mut d, issuer);
    let validity = d.len();
    d.tlv(0x17, b"200101000000Z").tlv(0x17, b"491231235959Z").wrap(validity, SEQUENCE);
    name(&mut d, subject_name);
    // SubjectPublicKeyInfo.
    let point = subject.verifying_key().to_encoded_point(false);
    let spki = d.len();
    let alg = d.len();
    d.tlv(0x06, EC_PUBLIC_KEY).tlv(0x06, PRIME256V1).wrap(alg, SEQUENCE);
    let bits = d.len();
    d.raw(&[0]).raw(point.as_bytes()).wrap(bits, 0x03).wrap(spki, SEQUENCE);
    // Extensions: basic constraints (and key usage for the CA).
    let extensions = d.len();
    let list = d.len();
    let e = d.len();
    d.tlv(0x06, BASIC_CONSTRAINTS);
    if ca {
        d.tlv(0x01, &[0xff]);
        let value = d.len();
        let inner = d.len();
        d.tlv(0x01, &[0xff]).wrap(inner, SEQUENCE).wrap(value, 0x04);
    } else {
        let value = d.len();
        let inner = d.len();
        d.wrap(inner, SEQUENCE).wrap(value, 0x04);
    }
    d.wrap(e, SEQUENCE);
    if ca {
        // keyCertSign | cRLSign.
        let e = d.len();
        d.tlv(0x06, KEY_USAGE).tlv(0x01, &[0xff]);
        let value = d.len();
        d.tlv(0x03, &[0x01, 0x06]).wrap(value, 0x04).wrap(e, SEQUENCE);
    }
    d.wrap(list, SEQUENCE).wrap(extensions, 0xa3);
    d.wrap(0, SEQUENCE);
    let tbs_len = d.len();

    let signature: Signature = ca_key().sign(&tbs[..tbs_len]);
    let mut sig = [0u8; 80];
    let sig_len = signature_der(&signature, &mut sig);

    let mut d = Der::new(out);
    d.raw(&tbs[..tbs_len]);
    algorithm(&mut d);
    let bits = d.len();
    d.raw(&[0]).raw(&sig[..sig_len]).wrap(bits, 0x03);
    d.wrap(0, SEQUENCE);
    d.len()
}

const CA_NAME: &[u8] = b"Banana Pepper Emulated Terminal CA";
/// The real terminal's certificate names it so; cabinets do not check.
const TERMINAL_NAME: &[u8] = b"terminal.wm4";

pub fn ca_der(out: &mut [u8]) -> usize {
    certificate(1, CA_NAME, CA_NAME, &ca_key(), true, out)
}

pub fn terminal_der(out: &mut [u8]) -> usize {
    certificate(2, CA_NAME, TERMINAL_NAME, &terminal_key(), false, out)
}

/// The CA certificate in PEM form, for the game's CA file.
pub fn ca_pem(out: &mut [u8]) -> usize {
    let mut der = [0u8; 800];
    let n = ca_der(&mut der);
    pem(&der[..n], b"CERTIFICATE", out)
}

fn pem(der: &[u8], label: &[u8], out: &mut [u8]) -> usize {
    const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut n = 0;
    let mut put = |bytes: &[u8]| {
        out[n..n + bytes.len()].copy_from_slice(bytes);
        n += bytes.len();
    };
    put(b"-----BEGIN ");
    put(label);
    put(b"-----\n");
    for line in der.chunks(48) {
        for group in line.chunks(3) {
            let b = [group[0], *group.get(1).unwrap_or(&0), *group.get(2).unwrap_or(&0)];
            let v = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
            let chars = [
                B64[(v >> 18) as usize & 63],
                B64[(v >> 12) as usize & 63],
                if group.len() > 1 { B64[(v >> 6) as usize & 63] } else { b'=' },
                if group.len() > 2 { B64[v as usize & 63] } else { b'=' },
            ];
            put(&chars);
        }
        put(b"\n");
    }
    put(b"-----END ");
    put(label);
    put(b"-----\n");
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn certificates_are_deterministic_and_well_formed() {
        let (mut a, mut b) = ([0u8; 800], [0u8; 800]);
        let n = ca_der(&mut a);
        assert_eq!(n, ca_der(&mut b));
        assert_eq!(a[..n], b[..n]);
        assert_eq!(a[0], 0x30);
        // The outer length covers the whole certificate.
        assert_eq!(usize::from(u16::from_be_bytes([a[2], a[3]])) + 4, n);
        let n = terminal_der(&mut a);
        assert_eq!(usize::from(u16::from_be_bytes([a[2], a[3]])) + 4, n);
        let mut text = [0u8; 1200];
        let n = ca_pem(&mut text);
        assert!(text[..n].starts_with(b"-----BEGIN CERTIFICATE-----\n"));
        assert!(text[..n].ends_with(b"-----END CERTIFICATE-----\n"));
    }
}
