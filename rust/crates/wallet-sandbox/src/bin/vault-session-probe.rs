//! PUBLIC test mnemonic only; this executable is not shipped to end users.
use std::{
    fs,
    io::{self, Read, Write},
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use wallet_sandbox::Container;
const WORDS: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art";
const PASSWORD: &[u8] = b"PUBLIC TEST PASSWORD ONLY";

fn main() {
    if let Err(error) = run() {
        eprintln!("public vault probe failed: {error}");
        std::process::exit(1);
    }
}
fn request(id: u64, opcode: u8, fields: &[&[u8]], target: &mut Vec<u8>) {
    let mut body = vec![2, opcode];
    body.extend(id.to_le_bytes());
    for field in fields {
        body.extend((field.len() as u16).to_le_bytes());
        body.extend_from_slice(field);
    }
    target.extend((body.len() as u32).to_le_bytes());
    target.extend(body);
}
fn run() -> io::Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 3 {
        return Err(io::Error::other(
            "usage: public-probe signer-exe artifact-directory",
        ));
    }
    let source = PathBuf::from(&args[1]);
    let root = PathBuf::from(&args[2]);
    if !source.is_absolute() || !root.is_absolute() {
        return Err(io::Error::other("absolute paths required"));
    }
    let result = std::process::Command::new(&source)
        .arg("--vault-session")
        .output()?;
    if result.status.code() != Some(2) || !result.stdout.is_empty() || !result.stderr.is_empty() {
        return Err(io::Error::other(
            "uncontained signer was not silently rejected",
        ));
    }
    fs::create_dir_all(&root)?;
    let container = Container::open("Web3Rust.VaultPublicTest.v2")?;
    let exe = root.join("wallet-signer.exe");
    fs::copy(source, &exe)?;
    container.grant_execute(&root)?;
    container.grant_execute(&exe)?;
    let identity = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let original = format!("public-{identity}-a");
    let restored = format!("public-{identity}-b");
    let mut bytes = Vec::new();
    request(
        1,
        7,
        &[original.as_bytes(), PASSWORD, WORDS.as_bytes()],
        &mut bytes,
    );
    request(2, 3, &[], &mut bytes);
    request(3, 4, &[b"incorrect backup"], &mut bytes);
    request(4, 4, &[WORDS.as_bytes()], &mut bytes);
    request(5, 3, &[], &mut bytes);
    request(6, 6, &[original.as_bytes(), PASSWORD], &mut bytes);
    request(
        7,
        7,
        &[restored.as_bytes(), PASSWORD, WORDS.as_bytes()],
        &mut bytes,
    );
    request(8, 4, &[WORDS.as_bytes()], &mut bytes);
    request(9, 6, &[restored.as_bytes(), PASSWORD], &mut bytes);
    request(
        10,
        2,
        &[original.as_bytes(), b"wrong public password"],
        &mut bytes,
    );
    request(11, 3, &[], &mut bytes);
    request(12, 5, &[], &mut bytes);
    assert!(bytes.len() < 4096);
    let mut process = container.spawn_offline(&exe, &["--vault-session"])?;
    process.input.take().unwrap().write_all(&bytes)?;
    let code = process.wait(Duration::from_secs(45))?;
    let mut output = Vec::new();
    process
        .output
        .take()
        .unwrap()
        .take(4096)
        .read_to_end(&mut output)?;
    if code != 0 {
        return Err(io::Error::other(format!("signer exit {code}")));
    }
    let mut replies = Vec::new();
    let mut data = output.as_slice();
    for id in 1..=12u64 {
        if data.len() < 4 {
            return Err(io::Error::other("missing response"));
        }
        let len = u32::from_le_bytes(data[..4].try_into().unwrap()) as usize;
        data = &data[4..];
        if len < 12 || data.len() < len {
            return Err(io::Error::other("response length"));
        }
        let frame = &data[..len];
        data = &data[len..];
        if frame[0] != 2 || u64::from_le_bytes(frame[2..10].try_into().unwrap()) != id {
            return Err(io::Error::other("response identity"));
        }
        replies.push((
            u16::from_le_bytes(frame[10..12].try_into().unwrap()),
            frame[12..].to_vec(),
        ));
    }
    let expected = [0, 0, 1, 0, 6, 0, 0, 0, 0, 2, 3, 0];
    for (value, expected) in replies.iter().zip(expected) {
        if value.0 != expected {
            return Err(io::Error::other("unexpected response status"));
        }
    }
    if !data.is_empty() || replies[1].1 != WORDS.as_bytes() || replies[5].1 != replies[8].1 {
        return Err(io::Error::other("backup or restored addresses mismatch"));
    }
    let fixture = include_str!("../../../../../tests/fixtures/hd-accounts.txt");
    let first = fixture
        .lines()
        .find(|line| line.starts_with(&"00".repeat(32)))
        .ok_or_else(|| io::Error::other("fixture missing"))?;
    let fields: Vec<_> = first.split('|').collect();
    if replies[5].1 != format!("{}\n{}", fields[1], fields[2]).as_bytes() {
        return Err(io::Error::other("independent address mismatch"));
    }
    println!(
        "PASS: uncontained service rejected; 12 ordered requests; backup confirmation; active export refused; wrong password rejected; independently verified BSC/TRON recovery"
    );
    // Remove only the two specifically created public-test profiles and known files.
    for name in [original, restored] {
        let path = container
            .directory
            .join("profiles")
            .join(format!("wallet-{name}"));
        for file in ["pending.vault", "active.vault", "profile.lock"] {
            fs::remove_file(path.join(file))?;
        }
        fs::remove_dir(path)?;
    }
    Ok(())
}
