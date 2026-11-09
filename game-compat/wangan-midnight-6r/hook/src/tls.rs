//! A minimal TLS 1.2 server for the emulated terminal.
//!
//! The cabinet's OpenSSL 1.0.2 client connects to its terminal with TLS 1.2
//! and offers, among others, TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256
//! (0xC02B). That is the one suite this server speaks: ECDHE on P-256, the
//! terminal's P-256 certificate (`pki.rs`), AES-128-GCM records and the
//! SHA-256 PRF (RFC 5246, RFC 5288, RFC 8422). No session resumption, no
//! renegotiation, no client certificates.
//!
//! No allocation and no `std`: the transport is a trait, so the hook runs it
//! on Winsock and the tests on memory.
use aes_gcm::{
    Aes128Gcm, Nonce, Tag,
    aead::{AeadInPlace, KeyInit},
};
use hmac::{Hmac, Mac};
use p256::{
    PublicKey, SecretKey,
    ecdsa::{Signature, signature::Signer},
    elliptic_curve::sec1::ToEncodedPoint,
};
use sha2::{Digest, Sha256};

/// A byte stream (a TCP connection).
pub trait Transport {
    /// Reads at least one byte; `None` when the stream ended or failed.
    fn read(&mut self, buffer: &mut [u8]) -> Option<usize>;
    /// Writes everything; `false` when the stream failed.
    fn write(&mut self, data: &[u8]) -> bool;
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Closed,
    Protocol(&'static str),
    /// The peer sent an alert (level, description).
    Alert(u8, u8),
}

const CHANGE_CIPHER_SPEC: u8 = 20;
const ALERT: u8 = 21;
const HANDSHAKE: u8 = 22;
const APPLICATION_DATA: u8 = 23;
const SUITE: u16 = 0xc02b;
const SECP256R1: u16 = 0x0017;
const ECDSA_SECP256R1_SHA256: u16 = 0x0403;

/// The largest record body accepted: a full plaintext plus GCM overhead.
pub const MAX_RECORD: usize = 16384 + 256;

fn read_exact<T: Transport>(io: &mut T, buffer: &mut [u8]) -> Result<(), Error> {
    let mut n = 0;
    while n < buffer.len() {
        n += io.read(&mut buffer[n..]).ok_or(Error::Closed)?;
    }
    Ok(())
}

/// One record: its type and its body in `buffer`.
fn read_record<T: Transport>(io: &mut T, buffer: &mut [u8]) -> Result<(u8, usize), Error> {
    let mut header = [0u8; 5];
    read_exact(io, &mut header)?;
    let len = usize::from(u16::from_be_bytes([header[3], header[4]]));
    if header[1] != 3 || len > MAX_RECORD || len > buffer.len() {
        return Err(Error::Protocol("bad record header"));
    }
    read_exact(io, &mut buffer[..len])?;
    Ok((header[0], len))
}

fn write_record<T: Transport>(io: &mut T, kind: u8, body: &[u8]) -> Result<(), Error> {
    let mut header = [kind, 3, 3, 0, 0];
    header[3..5].copy_from_slice(&(body.len() as u16).to_be_bytes());
    if io.write(&header) && io.write(body) { Ok(()) } else { Err(Error::Closed) }
}

/// P_SHA256 (RFC 5246 section 5): `out` filled from `secret`, `label`, `seed`.
pub fn prf(secret: &[u8], label: &[u8], seed: &[&[u8]], out: &mut [u8]) {
    let hmac = || <Hmac<Sha256> as Mac>::new_from_slice(secret).expect("any key length");
    // A(1) = HMAC(secret, label + seed)
    let mut a = hmac();
    a.update(label);
    for s in seed {
        a.update(s);
    }
    let mut a: [u8; 32] = a.finalize().into_bytes().into();
    let mut n = 0;
    while n < out.len() {
        let mut h = hmac();
        h.update(&a);
        h.update(label);
        for s in seed {
            h.update(s);
        }
        let block = h.finalize().into_bytes();
        let take = (out.len() - n).min(32);
        out[n..n + take].copy_from_slice(&block[..take]);
        n += take;
        let mut h = hmac();
        h.update(&a);
        a = h.finalize().into_bytes().into();
    }
}

/// One direction's record protection.
struct Direction {
    cipher: Aes128Gcm,
    iv: [u8; 4],
    sequence: u64,
}

impl Direction {
    fn new(key: &[u8], iv: &[u8]) -> Self {
        Self {
            cipher: Aes128Gcm::new_from_slice(key).expect("16-byte key"),
            iv: iv.try_into().expect("4-byte iv"),
            sequence: 0,
        }
    }

    fn aad(&self, kind: u8, len: usize) -> [u8; 13] {
        let mut aad = [0u8; 13];
        aad[..8].copy_from_slice(&self.sequence.to_be_bytes());
        aad[8] = kind;
        aad[9..11].copy_from_slice(&[3, 3]);
        aad[11..13].copy_from_slice(&(len as u16).to_be_bytes());
        aad
    }

    /// Decrypts a record body in place; returns the plaintext's range.
    fn open(&mut self, kind: u8, body: &mut [u8]) -> Result<core::ops::Range<usize>, Error> {
        if body.len() < 24 {
            return Err(Error::Protocol("short record"));
        }
        let len = body.len() - 24;
        let mut nonce = [0u8; 12];
        nonce[..4].copy_from_slice(&self.iv);
        nonce[4..].copy_from_slice(&body[..8]);
        let aad = self.aad(kind, len);
        let (text, tag) = body[8..].split_at_mut(len);
        self.cipher
            .decrypt_in_place_detached(Nonce::from_slice(&nonce), &aad, text, Tag::from_slice(tag))
            .map_err(|_| Error::Protocol("bad record MAC"))?;
        self.sequence += 1;
        Ok(8..8 + len)
    }

    /// Encrypts `plain` into a record body in `out`; returns its length.
    fn seal(&mut self, kind: u8, plain: &[u8], out: &mut [u8]) -> usize {
        let explicit = self.sequence.to_be_bytes();
        let mut nonce = [0u8; 12];
        nonce[..4].copy_from_slice(&self.iv);
        nonce[4..].copy_from_slice(&explicit);
        let aad = self.aad(kind, plain.len());
        out[..8].copy_from_slice(&explicit);
        out[8..8 + plain.len()].copy_from_slice(plain);
        let tag = self
            .cipher
            .encrypt_in_place_detached(Nonce::from_slice(&nonce), &aad, &mut out[8..8 + plain.len()])
            .expect("record fits");
        out[8 + plain.len()..24 + plain.len()].copy_from_slice(&tag);
        self.sequence += 1;
        24 + plain.len()
    }
}

/// An established connection.
pub struct Session {
    read: Direction,
    write: Direction,
}

/// Collects handshake messages from records, hashing each one.
struct Handshake {
    buffer: [u8; 4096],
    len: usize,
    transcript: Sha256,
}

impl Handshake {
    /// The next complete message (type, body) from `buffer`, reading plain
    /// handshake records as needed.
    fn next<T: Transport>(&mut self, io: &mut T, record: &mut [u8]) -> Result<(u8, usize, usize), Error> {
        loop {
            if self.len >= 4 {
                let body = usize::from(self.buffer[1]) << 16 | usize::from(self.buffer[2]) << 8 | usize::from(self.buffer[3]);
                if body + 4 > self.buffer.len() {
                    return Err(Error::Protocol("handshake message too long"));
                }
                if self.len >= body + 4 {
                    return Ok((self.buffer[0], 4, body));
                }
            }
            let (kind, n) = read_record(io, record)?;
            match kind {
                HANDSHAKE if self.len + n <= self.buffer.len() => {
                    self.buffer[self.len..self.len + n].copy_from_slice(&record[..n]);
                    self.len += n;
                }
                ALERT if n >= 2 => return Err(Error::Alert(record[0], record[1])),
                _ => return Err(Error::Protocol("unexpected record")),
            }
        }
    }

    /// Drops the message at the front (after hashing it).
    fn consume(&mut self, body: usize) {
        let total = 4 + body;
        self.transcript.update(&self.buffer[..total]);
        self.buffer.copy_within(total..self.len, 0);
        self.len -= total;
    }
}

/// Appends a handshake message (type, body) to `out` at `*n`.
fn message(out: &mut [u8], n: &mut usize, kind: u8, body: &[u8]) {
    out[*n] = kind;
    out[*n + 1..*n + 4].copy_from_slice(&(body.len() as u32).to_be_bytes()[1..]);
    out[*n + 4..*n + 4 + body.len()].copy_from_slice(body);
    *n += 4 + body.len();
}

struct Cursor<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        let s = self.data.get(self.at..self.at + n).ok_or(Error::Protocol("short message"))?;
        self.at += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<usize, Error> {
        Ok(usize::from(self.take(1)?[0]))
    }
    fn u16(&mut self) -> Result<usize, Error> {
        let b = self.take(2)?;
        Ok(usize::from(u16::from_be_bytes([b[0], b[1]])))
    }
}

/// The ClientHello's random, after checking it offers the suite.
fn client_hello(body: &[u8]) -> Result<[u8; 32], Error> {
    let mut c = Cursor { data: body, at: 0 };
    if c.take(2)? < &[3, 3][..] {
        return Err(Error::Protocol("client does not offer TLS 1.2"));
    }
    let random: [u8; 32] = c.take(32)?.try_into().unwrap();
    let session = c.u8()?;
    c.take(session)?;
    let suites = c.u16()?;
    let suites = c.take(suites)?;
    if !suites.chunks(2).any(|s| s == SUITE.to_be_bytes()) {
        return Err(Error::Protocol("client does not offer ECDHE-ECDSA-AES128-GCM-SHA256"));
    }
    Ok(random)
}

impl Session {
    /// Runs a server handshake. `random` and `ephemeral` are fresh random
    /// bytes (the server random and the ECDHE private key); `certificates`
    /// is the DER chain, leaf first; `key` signs the key exchange.
    pub fn accept<T: Transport>(
        io: &mut T,
        random: &[u8; 32],
        ephemeral: &[u8; 32],
        certificates: &[&[u8]],
        key: &p256::ecdsa::SigningKey,
    ) -> Result<Self, Error> {
        let mut record = [0u8; MAX_RECORD];
        let mut hs = Handshake { buffer: [0; 4096], len: 0, transcript: Sha256::new() };

        let (kind, at, body) = hs.next(io, &mut record)?;
        if kind != 1 {
            return Err(Error::Protocol("expected ClientHello"));
        }
        let client_random = client_hello(&hs.buffer[at..at + body])?;
        hs.consume(body);

        let secret = SecretKey::from_bytes(ephemeral.into()).map_err(|_| Error::Protocol("bad ephemeral key"))?;
        let point = secret.public_key().to_encoded_point(false);
        let point = point.as_bytes();

        // ServerHello, Certificate, ServerKeyExchange, ServerHelloDone.
        let mut flight = [0u8; 4096];
        let mut n = 0;
        let mut hello = [0u8; 64];
        let mut h = 0;
        for part in [&[3u8, 3][..], random, &[0], &SUITE.to_be_bytes(), &[0]] {
            hello[h..h + part.len()].copy_from_slice(part);
            h += part.len();
        }
        // Extensions: renegotiation_info (empty), ec_point_formats
        // (uncompressed).
        let extensions = [0xff, 0x01, 0, 1, 0, 0x00, 0x0b, 0, 2, 1, 0];
        hello[h..h + 2].copy_from_slice(&(extensions.len() as u16).to_be_bytes());
        hello[h + 2..h + 2 + extensions.len()].copy_from_slice(&extensions);
        h += 2 + extensions.len();
        message(&mut flight, &mut n, 2, &hello[..h]);

        let mut chain = [0u8; 2048];
        let mut c = 3;
        for der in certificates {
            chain[c..c + 3].copy_from_slice(&(der.len() as u32).to_be_bytes()[1..]);
            chain[c + 3..c + 3 + der.len()].copy_from_slice(der);
            c += 3 + der.len();
        }
        chain[..3].copy_from_slice(&((c - 3) as u32).to_be_bytes()[1..]);
        message(&mut flight, &mut n, 11, &chain[..c]);

        let mut exchange = [0u8; 200];
        exchange[0] = 3; // named_curve
        exchange[1..3].copy_from_slice(&SECP256R1.to_be_bytes());
        exchange[3] = point.len() as u8;
        exchange[4..4 + point.len()].copy_from_slice(point);
        let params = 4 + point.len();
        let mut signed = [0u8; 200];
        signed[..32].copy_from_slice(&client_random);
        signed[32..64].copy_from_slice(random);
        signed[64..64 + params].copy_from_slice(&exchange[..params]);
        let signature: Signature = key.sign(&signed[..64 + params]);
        let mut der = [0u8; 80];
        let der_len = crate::pki::signature_der(&signature, &mut der);
        exchange[params..params + 2].copy_from_slice(&ECDSA_SECP256R1_SHA256.to_be_bytes());
        exchange[params + 2..params + 4].copy_from_slice(&(der_len as u16).to_be_bytes());
        exchange[params + 4..params + 4 + der_len].copy_from_slice(&der[..der_len]);
        message(&mut flight, &mut n, 12, &exchange[..params + 4 + der_len]);
        message(&mut flight, &mut n, 14, &[]);
        hs.transcript.update(&flight[..n]);
        write_record(io, HANDSHAKE, &flight[..n])?;

        // ClientKeyExchange: the client's ECDHE point.
        let (kind, at, body) = hs.next(io, &mut record)?;
        if kind != 16 || body < 1 || usize::from(hs.buffer[at]) != body - 1 {
            return Err(Error::Protocol("expected ClientKeyExchange"));
        }
        let client_point = PublicKey::from_sec1_bytes(&hs.buffer[at + 1..at + body])
            .map_err(|_| Error::Protocol("bad client key share"))?;
        hs.consume(body);
        let shared = p256::ecdh::diffie_hellman(secret.to_nonzero_scalar(), client_point.as_affine());
        let mut master = [0u8; 48];
        prf(
            shared.raw_secret_bytes(),
            b"master secret",
            &[&client_random, random],
            &mut master,
        );
        let mut keys = [0u8; 40];
        prf(&master, b"key expansion", &[random, &client_random], &mut keys);
        let mut session = Session {
            read: Direction::new(&keys[0..16], &keys[32..36]),
            write: Direction::new(&keys[16..32], &keys[36..40]),
        };

        // ChangeCipherSpec, then the client's Finished (encrypted).
        let (kind, len) = read_record(io, &mut record)?;
        if kind != CHANGE_CIPHER_SPEC || len != 1 {
            return Err(Error::Protocol("expected ChangeCipherSpec"));
        }
        let (kind, len) = read_record(io, &mut record)?;
        if kind != HANDSHAKE {
            return Err(Error::Protocol("expected Finished"));
        }
        let plain = session.read.open(HANDSHAKE, &mut record[..len])?;
        let finished = &record[plain];
        let mut expected = [0u8; 12];
        prf(&master, b"client finished", &[&hs.transcript.clone().finalize()], &mut expected);
        if finished.len() != 16 || finished[0] != 20 || finished[4..] != expected {
            return Err(Error::Protocol("bad client Finished"));
        }
        hs.transcript.update(finished);

        // ChangeCipherSpec, then our Finished.
        write_record(io, CHANGE_CIPHER_SPEC, &[1])?;
        let mut verify = [0u8; 12];
        prf(&master, b"server finished", &[&hs.transcript.finalize()], &mut verify);
        let mut done = [0u8; 16];
        let mut d = 0;
        message(&mut done, &mut d, 20, &verify);
        let mut out = [0u8; 64];
        let sealed = session.write.seal(HANDSHAKE, &done, &mut out);
        write_record(io, HANDSHAKE, &out[..sealed])?;
        Ok(session)
    }

    /// The next application data into `out`; returns its length (0 when the
    /// peer closed the connection).
    pub fn read<T: Transport>(&mut self, io: &mut T, out: &mut [u8]) -> Result<usize, Error> {
        let mut record = [0u8; MAX_RECORD];
        loop {
            let (kind, len) = match read_record(io, &mut record) {
                Err(Error::Closed) => return Ok(0),
                r => r?,
            };
            let plain = self.read.open(kind, &mut record[..len])?;
            match kind {
                APPLICATION_DATA => {
                    let n = plain.len();
                    if n > out.len() {
                        return Err(Error::Protocol("record larger than the buffer"));
                    }
                    out[..n].copy_from_slice(&record[plain]);
                    if n > 0 {
                        return Ok(n);
                    }
                }
                ALERT => return Ok(0),
                _ => return Err(Error::Protocol("unexpected record")),
            }
        }
    }

    /// Sends application data.
    pub fn write<T: Transport>(&mut self, io: &mut T, data: &[u8]) -> Result<(), Error> {
        let mut record = [0u8; MAX_RECORD];
        for chunk in data.chunks(16384) {
            let n = self.write.seal(APPLICATION_DATA, chunk, &mut record);
            write_record(io, APPLICATION_DATA, &record[..n])?;
        }
        Ok(())
    }

    /// Sends a close_notify alert.
    pub fn close<T: Transport>(&mut self, io: &mut T) {
        let mut record = [0u8; 64];
        let n = self.write.seal(ALERT, &[1, 0], &mut record);
        let _ = write_record(io, ALERT, &record[..n]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 5246 PRF with SHA-256: the widely used test vector (from the
    /// IETF TLS working group mailing list, "TLS 1.2 PRF test vectors").
    #[test]
    fn prf_matches_the_published_vector() {
        let secret = hex("9bbe436ba940f017b17652849a71db35");
        let seed = hex("a0ba9f936cda311827a6f796ffd5198c");
        let mut out = [0u8; 100];
        prf(&secret, b"test label", &[&seed], &mut out);
        assert_eq!(
            out[..],
            hex("e3f229ba727be17b8d122620557cd453c2aab21d07c3d495329b52d4e61edb5a6b301791e90d35c9c9a46b4e14baf9af0fa022f7077def17abfd3797c0564bab4fbc91666e9def9b97fce34f796789baa48082d122ee42c5a72e5a5110fff70187347b66")[..]
        );
    }

    fn hex(text: &str) -> Vec<u8> {
        (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap()).collect()
    }
}
