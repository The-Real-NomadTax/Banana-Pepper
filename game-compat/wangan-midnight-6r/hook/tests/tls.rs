//! The emulated terminal's TLS server against a real OpenSSL client: the
//! client trusts only the emulation's CA, completes a TLS 1.2 handshake with
//! ECDHE-ECDSA-AES128-GCM-SHA256 and exchanges data both ways. Skipped when
//! no `openssl` command is installed.
use arcade_wmmt6r::{pki, tls};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    process::{Command, Stdio},
};

struct Stream(TcpStream);

impl tls::Transport for Stream {
    fn read(&mut self, buffer: &mut [u8]) -> Option<usize> {
        match self.0.read(buffer) {
            Ok(0) | Err(_) => None,
            Ok(n) => Some(n),
        }
    }
    fn write(&mut self, data: &[u8]) -> bool {
        self.0.write_all(data).is_ok()
    }
}

#[test]
fn openssl_client_trusts_the_terminal() {
    if Command::new("openssl").arg("version").output().is_err() {
        eprintln!("no openssl command; skipped");
        return;
    }
    let dir = std::env::temp_dir().join(format!("wmmt6r-tls-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut pem = [0u8; 1200];
    let n = pki::ca_pem(&mut pem);
    let ca = dir.join("ca.pem");
    std::fs::write(&ca, &pem[..n]).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut io = Stream(stream);
        let (mut leaf, mut root) = ([0u8; 800], [0u8; 800]);
        let l = pki::terminal_der(&mut leaf);
        let r = pki::ca_der(&mut root);
        let mut session =
            tls::Session::accept(&mut io, &[7; 32], &[9; 32], &[&leaf[..l], &root[..r]], &pki::terminal_key())
                .expect("handshake");
        let mut request = [0u8; 256];
        let n = session.read(&mut io, &mut request).unwrap();
        assert_eq!(&request[..n], b"ping\n");
        session.write(&mut io, b"pong\n").unwrap();
        session.close(&mut io);
    });

    let mut client = Command::new("openssl")
        .args(["s_client", "-quiet", "-tls1_2", "-verify_return_error"])
        .args(["-cipher", "ECDHE-ECDSA-AES128-GCM-SHA256"])
        .arg("-CAfile")
        .arg(&ca)
        .arg("-connect")
        .arg(format!("127.0.0.1:{port}"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    client.stdin.as_mut().unwrap().write_all(b"ping\n").unwrap();
    server.join().unwrap();
    drop(client.stdin.take());
    let output = client.wait_with_output().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("pong"),
        "client output: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
