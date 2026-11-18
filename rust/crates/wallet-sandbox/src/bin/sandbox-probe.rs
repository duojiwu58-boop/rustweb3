//! Public-data-only isolation acceptance probe. Never opens a real wallet.
use std::{
    fs::{self, OpenOptions},
    io::{self, BufRead, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs},
    os::windows::process::CommandExt,
    path::PathBuf,
    time::Duration,
};
use wallet_sandbox::{Container, verify_offline_token};
use windows_sys::Win32::{
    Foundation::{CloseHandle, WAIT_OBJECT_0},
    System::Threading::{
        OpenProcess, PROCESS_DUP_HANDLE, PROCESS_SYNCHRONIZE, PROCESS_VM_READ, PROCESS_VM_WRITE,
        WaitForSingleObject,
    },
};

fn main() {
    if let Err(error) = run() {
        eprintln!("probe failed: {error}");
        std::process::exit(1);
    }
}
fn run() -> io::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--sleep-child") {
        verify_offline_token()?;
        std::thread::sleep(Duration::from_secs(60));
        return Ok(());
    }
    if args.get(1).map(String::as_str) == Some("--job-owner") {
        let container = Container::open("Web3Rust.OfflineProbe.v1")?;
        let child = container.spawn_offline(std::path::Path::new(&args[2]), &["--sleep-child"])?;
        println!("{}", child.id);
        io::stdout().flush()?;
        std::thread::sleep(Duration::from_secs(60));
        drop(child);
        return Ok(());
    }
    if args.get(1).map(String::as_str) == Some("--child") {
        return child(&args[2..]);
    }
    if args.get(1).map(String::as_str) == Some("--network-child") {
        return network_child(&args[2..]);
    }
    let root = PathBuf::from(
        args.get(1)
            .ok_or_else(|| io::Error::other("artifact directory required"))?,
    );
    if !root.is_absolute() {
        return Err(io::Error::other("absolute artifact directory required"));
    }
    fs::create_dir_all(&root)?;
    let container = Container::open("Web3Rust.OfflineProbe.v1")?;
    let exe = root.join("sandbox-probe.exe");
    if std::env::current_exe()? != exe {
        fs::copy(std::env::current_exe()?, &exe)?;
    }
    container.grant_execute(&root)?;
    container.grant_execute(&exe)?;
    let mut owner = std::process::Command::new(std::env::current_exe()?)
        .args(["--job-owner", exe.to_str().unwrap()])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .creation_flags(0x08000000)
        .spawn()?;
    let mut line = String::new();
    let result = io::BufReader::new(owner.stdout.take().unwrap()).read_line(&mut line);
    let child_id: io::Result<u32> = result.and_then(|_| {
        line.trim()
            .parse()
            .map_err(|_| io::Error::other("job owner child identity"))
    });
    let child_handle = match child_id {
        Ok(id) => unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, id) },
        Err(error) => {
            let _ = owner.kill();
            let _ = owner.wait();
            return Err(error);
        }
    };
    owner.kill()?;
    owner.wait()?;
    if child_handle.is_null() {
        return Err(io::Error::other("job child missing before parent kill"));
    }
    let exited = unsafe {
        let value = WaitForSingleObject(child_handle, 5000);
        CloseHandle(child_handle);
        value
    };
    if exited != WAIT_OBJECT_0 {
        return Err(io::Error::other("job child survived abrupt owner death"));
    }
    println!("kill-on-job-close: child exited after owner was forcibly killed");
    let outside = root.join("outside-public-fixture.txt");
    fs::write(&outside, b"PUBLIC FIXTURE - NOT A WALLET")?;
    let v4 = TcpListener::bind("127.0.0.1:0")?;
    let v6 = TcpListener::bind("[::1]:0")?;
    for listener in [&v4, &v6] {
        let connection =
            TcpStream::connect_timeout(&listener.local_addr()?, Duration::from_secs(1))?;
        let accepted = listener.accept()?;
        drop((connection, accepted));
        listener.set_nonblocking(true)?;
    }
    let outside_text = outside.to_str().ok_or_else(|| io::Error::other("path"))?;
    let parent = std::process::id().to_string();
    let ipv4 = v4.local_addr()?.to_string();
    let ipv6 = v6.local_addr()?.to_string();
    let mut process =
        container.spawn_offline(&exe, &["--child", outside_text, &parent, &ipv4, &ipv6])?;
    process
        .input
        .take()
        .unwrap()
        .write_all(b"PUBLIC-PIPE-CHALLENGE")?;
    let code = process.wait(Duration::from_secs(20))?;
    let mut output = String::new();
    process
        .output
        .take()
        .unwrap()
        .take(4096)
        .read_to_string(&mut output)?;
    print!("{output}");
    if code != 0 {
        return Err(io::Error::other(format!("child exit {code}")));
    }
    if fs::read(&outside)? != b"PUBLIC FIXTURE - NOT A WALLET" {
        return Err(io::Error::other("outside file changed"));
    }
    if !output.contains("OFFLINE_PROBE_EXPECTATIONS_PASSED") {
        return Err(io::Error::other("missing probe result"));
    }
    for listener in [&v4, &v6] {
        if listener.accept().is_ok() {
            return Err(io::Error::other("sandbox reached loopback listener"));
        }
    }
    println!("parent: outside bytes unchanged; child exited; job closed");
    let network = Container::open("Web3Rust.NetworkProbe.v1")?;
    network.grant_execute(&root)?;
    network.grant_execute(&exe)?;
    let secret_fixture = container.directory.join("secret-public-fixture.txt");
    fs::write(
        &secret_fixture,
        b"PUBLIC FIXTURE - TESTING CROSS-CONTAINER ACCESS",
    )?;
    let mut child = network.spawn_internet(
        &exe,
        &["--network-child", secret_fixture.to_str().unwrap(), &parent],
    )?;
    drop(child.input.take());
    let code = child.wait(Duration::from_secs(15))?;
    let mut output = String::new();
    child
        .output
        .take()
        .unwrap()
        .take(4096)
        .read_to_string(&mut output)?;
    print!("{output}");
    fs::remove_file(secret_fixture)?;
    if code != 0 {
        return Err(io::Error::other(format!("network probe exit {code}")));
    }
    Ok(())
}

fn network_child(args: &[String]) -> io::Result<()> {
    if args.len() != 2 {
        return Err(io::Error::other("network arguments"));
    }
    wallet_sandbox::verify_internet_token()?;
    denied(
        "network reading signer container",
        fs::read(&args[0]).map(|_| ()),
    )?;
    denied(
        "network writing signer container",
        OpenOptions::new().write(true).open(&args[0]).map(|_| ()),
    )?;
    unsafe {
        let handle = OpenProcess(
            PROCESS_VM_READ | PROCESS_VM_WRITE | PROCESS_DUP_HANDLE,
            0,
            args[1].parse().map_err(|_| io::Error::other("PID"))?,
        );
        if !handle.is_null() {
            CloseHandle(handle);
            return Err(io::Error::other("network accessed parent memory"));
        }
        denied(
            "network accessing parent memory",
            Err(io::Error::last_os_error()),
        )?;
    }
    let addresses: Vec<_> = ("example.com", 443).to_socket_addrs()?.collect();
    if addresses.is_empty() {
        return Err(io::Error::other("DNS empty"));
    }
    println!(
        "network capability: DNS positive control returned {} addresses",
        addresses.len()
    );
    let endpoint = addresses
        .iter()
        .find(|value| value.is_ipv4())
        .ok_or_else(|| io::Error::other("no IPv4"))?;
    match TcpStream::connect_timeout(endpoint, Duration::from_secs(3)) {
        Ok(_) => println!("network capability: public IPv4 TCP connected"),
        Err(error) => {
            return Err(io::Error::other(format!(
                "network positive control failed: {error}"
            )));
        }
    }
    Ok(())
}
fn child(args: &[String]) -> io::Result<()> {
    if args.len() != 4 {
        return Err(io::Error::other("arguments"));
    }
    verify_offline_token()?;
    println!("token: AppContainer, zero capabilities");
    let mut input = Vec::new();
    io::stdin().take(128).read_to_end(&mut input)?;
    if input != b"PUBLIC-PIPE-CHALLENGE" {
        return Err(io::Error::other("pipe mismatch"));
    }
    println!("dedicated inherited pipe: challenge matched");
    for (name, result) in [
        ("outside read", fs::read(&args[0]).map(|_| ())),
        (
            "outside write",
            OpenOptions::new().write(true).open(&args[0]).map(|_| ()),
        ),
    ] {
        denied(name, result)?;
    }
    let parent: u32 = args[1].parse().map_err(|_| io::Error::other("PID"))?;
    for access in [PROCESS_VM_READ, PROCESS_VM_WRITE, PROCESS_DUP_HANDLE] {
        unsafe {
            let handle = OpenProcess(access, 0, parent);
            if !handle.is_null() {
                CloseHandle(handle);
                return Err(io::Error::other("parent memory accessible"));
            }
            denied("parent memory", Err(io::Error::last_os_error()))?;
        }
    }
    for endpoint in [
        &args[2],
        &args[3],
        "127.0.0.1:1080",
        "1.1.1.1:443",
        "[2606:4700:4700::1111]:443",
    ] {
        let address: SocketAddr = endpoint.parse().map_err(|_| io::Error::other("endpoint"))?;
        let result = TcpStream::connect_timeout(&address, Duration::from_secs(2)).map(|_| ());
        match result {
            Err(error) => println!(
                "TCP {endpoint}: blocked or unavailable ({:?}, {:?})",
                error.kind(),
                error.raw_os_error()
            ),
            Ok(()) => {
                return Err(io::Error::other(format!(
                    "TCP {endpoint}: unexpectedly allowed"
                )));
            }
        }
    }
    match ("example.com", 443).to_socket_addrs() {
        Ok(_) => return Err(io::Error::other("offline DNS unexpectedly resolved")),
        Err(error) => println!("DNS resolver: {error} (failure alone does not prove isolation)"),
    }
    let own = std::env::current_dir()?.join("probe-public.tmp");
    fs::write(&own, b"PUBLIC-OWN-CONTAINER")?;
    if fs::read(&own)? != b"PUBLIC-OWN-CONTAINER" {
        return Err(io::Error::other("own cache mismatch"));
    }
    fs::remove_file(own)?;
    println!("own container: read/write OK");
    let nested = std::process::Command::new(std::env::current_exe()?)
        .arg("--forbidden-child")
        .creation_flags(0x08000000)
        .spawn();
    match nested {
        Err(error) => println!(
            "active process limit: child creation rejected ({:?})",
            error.raw_os_error()
        ),
        Ok(mut child) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::other("nested process unexpectedly allowed"));
        }
    }
    println!("OFFLINE_PROBE_EXPECTATIONS_PASSED");
    Ok(())
}
fn denied(name: &str, result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            println!("{name}: OS denied ({:?})", error.raw_os_error());
            Ok(())
        }
        Err(error) => Err(io::Error::other(format!(
            "{name}: not an access denial: {error}"
        ))),
        Ok(()) => Err(io::Error::other(format!("{name}: unexpectedly allowed"))),
    }
}
