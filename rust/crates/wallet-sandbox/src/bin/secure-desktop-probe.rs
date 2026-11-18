//! Launch the Delphi ABI smoke in a separate public-test offline container.
use std::{
    fs,
    io::{self, Read},
    path::PathBuf,
    time::Duration,
};
use wallet_sandbox::Container;
fn main() {
    if let Err(error) = run() {
        eprintln!("secure desktop probe: {error}");
        std::process::exit(1);
    }
}
fn run() -> io::Result<()> {
    let root = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or_else(|| io::Error::other("binary directory required"))?,
    );
    if !root.is_absolute() {
        return Err(io::Error::other("absolute directory required"));
    }
    let container = Container::open("Web3Rust.DesktopPublicTest.v2")?;
    container.grant_execute(&root)?;
    for name in [
        "SecureAbiSmoke.exe",
        "SecureWallet.exe",
        "web3rust.dll",
        "wallet-signer.exe",
    ] {
        container.grant_execute(&root.join(name))?;
    }
    let mut process = container.spawn_offline_gui(&root.join("SecureAbiSmoke.exe"), &[])?;
    drop(process.input.take());
    let code = process.wait(Duration::from_secs(45))?;
    let mut output = String::new();
    process
        .output
        .take()
        .unwrap()
        .take(4096)
        .read_to_string(&mut output)?;
    print!("{output}");
    if code != 0 || !output.starts_with("PASS:") {
        return Err(io::Error::other(format!("ABI smoke exit {code}")));
    }
    // Dedicated test identity holds only public fixtures, retained for restart inspection.
    fs::write(root.join("secure-abi-result.txt"), output)?;
    let mut process =
        container.spawn_offline_gui(&root.join("SecureWallet.exe"), &["--self-test"])?;
    drop(process.input.take());
    let code = process.wait(Duration::from_secs(30))?;
    let mut output = String::new();
    process
        .output
        .take()
        .unwrap()
        .take(4096)
        .read_to_string(&mut output)?;
    print!("{output}");
    if code != 0 || !output.starts_with("PASS:") {
        return Err(io::Error::other(format!("VCL test exit {code}")));
    }
    fs::write(root.join("secure-vcl-result.txt"), output)?;
    Ok(())
}
