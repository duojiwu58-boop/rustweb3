//! Public mainnet read-only probe, launched with only internetClient capability.
use std::{
    fs,
    io::{self, Read},
    path::PathBuf,
    time::Duration,
};
use wallet_sandbox::Container;
fn main() {
    if let Err(error) = run() {
        eprintln!("RPC launcher: {error}");
        std::process::exit(1);
    }
}
fn run() -> io::Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 3 {
        return Err(io::Error::other(
            "network-exe and artifact-directory required",
        ));
    }
    let root = PathBuf::from(&args[2]);
    if !root.is_absolute() {
        return Err(io::Error::other("absolute directory required"));
    }
    fs::create_dir_all(&root)?;
    let container = Container::open("Web3Rust.NetworkProbe.v1")?;
    let exe = root.join("wallet-network.exe");
    fs::copy(&args[1], &exe)?;
    container.grant_execute(&root)?;
    container.grant_execute(&exe)?;
    let mut process = container.spawn_internet(&exe, &["--rpc-probe"])?;
    drop(process.input.take());
    let code = process.wait(Duration::from_secs(55))?;
    let mut output = String::new();
    process
        .output
        .take()
        .unwrap()
        .take(4096)
        .read_to_string(&mut output)?;
    print!("{output}");
    fs::write(root.join("rpc-probe.txt"), output)?;
    if code != 0 {
        return Err(io::Error::other(format!("public RPC process exit {code}")));
    }
    Ok(())
}
