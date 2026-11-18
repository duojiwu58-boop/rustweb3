//! Trusted offline broker: relays only bounded public requests to one-shot network workers.
#![windows_subsystem = "windows"]
use std::{
    fs::File,
    io::{self, Read, Write},
    os::windows::io::AsRawHandle,
    time::{Duration, Instant},
};
use wallet_sandbox::{Container, SingleInstance};
use windows_sys::Win32::System::Pipes::PeekNamedPipe;
fn is_self_test() -> bool {
    std::env::args().any(|arg| matches!(arg.as_str(), "--self-test" | "--tron-self-test"))
}
#[path = "../explorer.rs"]
mod explorer;
fn main() {
    if let Err(error) = run() {
        eprintln!("Web3Rust startup/transport: {error}");
        if !is_self_test() {
            let message: Vec<u16> = format!("钱包未能启动或连接中断。\n\n{error}\0")
                .encode_utf16()
                .collect();
            let title: Vec<u16> = "Web3Rust\0".encode_utf16().collect();
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
                    std::ptr::null_mut(),
                    message.as_ptr(),
                    title.as_ptr(),
                    windows_sys::Win32::UI::WindowsAndMessaging::MB_OK
                        | windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONERROR,
                );
            }
        }
        std::process::exit(1);
    }
}
fn show_existing_wallet() {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    unsafe {
        let title: Vec<u16> = "Web3Rust Wallet\0".encode_utf16().collect();
        let window = FindWindowW(std::ptr::null(), title.as_ptr());
        if !window.is_null() {
            ShowWindow(window, SW_RESTORE);
            if SetForegroundWindow(window) != 0 { return; }
        }
        let message: Vec<u16> = "钱包已经打开，请切换到已有的钱包窗口。\0".encode_utf16().collect();
        let caption: Vec<u16> = "Web3Rust\0".encode_utf16().collect();
        MessageBoxW(std::ptr::null_mut(), message.as_ptr(), caption.as_ptr(), MB_OK | MB_ICONINFORMATION);
    }
}
fn available(file: &File) -> io::Result<u32> {
    let mut count = 0;
    let result = unsafe {
        PeekNamedPipe(
            file.as_raw_handle(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            &mut count,
            std::ptr::null_mut(),
        )
    };
    if result == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(count)
}
fn read(file: &mut File, mut bytes: &mut [u8]) -> io::Result<()> {
    let end = Instant::now() + Duration::from_secs(5);
    while !bytes.is_empty() {
        let count = (available(file)? as usize).min(bytes.len());
        if count > 0 {
            file.read_exact(&mut bytes[..count])?;
            bytes = &mut bytes[count..];
        } else {
            if Instant::now() >= end {
                return Err(io::Error::other("partial public frame timeout"));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    Ok(())
}
fn run() -> io::Result<()> {
    let context =
        |stage: &str, error: io::Error| io::Error::new(error.kind(), format!("{stage}: {error}"));
    let _instance = match SingleInstance::acquire() {
        Ok(instance) => instance,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists
            && !is_self_test() => {
            show_existing_wallet();
            return Ok(());
        }
        Err(error) => return Err(context("检查钱包单实例", error)),
    };
    let root = std::env::current_exe()?
        .parent()
        .ok_or_else(|| io::Error::other("binary directory"))?
        .to_owned();
    let self_test = is_self_test();
    let tron_test = std::env::args().nth(1).as_deref() == Some("--tron-self-test");
    let preview = std::env::args().nth(1).as_deref() == Some("--preview");
    let proxy = wallet_sandbox::network_config::load(&root)
        .map_err(|e| context("读取 network.ini 代理配置", e))?;
    let offline = Container::open(if self_test || preview {
        "Web3Rust.DesktopPublicTest.v2"
    } else {
        "Web3Rust.Wallet.v1"
    })
    .map_err(|e| context("打开离线钱包容器", e))?;
    let online = Container::open(if proxy.is_some() {
        "Web3Rust.ProxyNetwork.v1"
    } else {
        "Web3Rust.Network.v1"
    })
    .map_err(|e| context("打开联网容器", e))?;
    if proxy.is_some() {
        online
            .enable_proxy_loopback()
            .map_err(|e| context("配置本机代理访问", e))?;
    }
    offline
        .grant_execute(&root)
        .map_err(|e| context("检查钱包目录权限", e))?;
    online
        .grant_execute(&root)
        .map_err(|e| context("检查联网目录权限", e))?;
    for name in ["SecureWallet.exe", "web3rust.dll", "wallet-signer.exe"] {
        offline
            .grant_execute(&root.join(name))
            .map_err(|e| context(&format!("检查文件权限 {name}"), e))?;
    }
    online
        .grant_execute(&root.join("wallet-network.exe"))
        .map_err(|e| context("检查 wallet-network.exe 权限", e))?;
    let arguments: &[&str] = if preview {
        &["--public-preview"]
    } else if tron_test {
        &["--tron-network-self-test"]
    } else if self_test {
        &["--network-self-test"]
    } else {
        &[]
    };
    let mut gui = offline
        .spawn_offline_gui(&root.join("SecureWallet.exe"), arguments)
        .map_err(|e| context("启动离线钱包窗口", e))?;
    let mut input = gui.output.take().unwrap();
    let mut output = gui.input.take().unwrap();
    let mut expected = 1u64;
    loop {
        if let Some(code) = gui.try_wait()? {
            if code == 0 {
                return Ok(());
            }
            return Err(io::Error::other(format!("GUI exited {code}")));
        }
        match available(&input) {
            Ok(count) if count >= 4 => (),
            Ok(_) => {
                std::thread::sleep(Duration::from_millis(20));
                continue;
            }
            Err(_) => {
                gui.wait(Duration::from_secs(2))?;
                return Ok(());
            }
        }
        let mut prefix = [0; 4];
        read(&mut input, &mut prefix)?;
        let length = u32::from_le_bytes(prefix) as usize;
        if !(10..=2048).contains(&length) {
            return Err(io::Error::other("invalid public frame"));
        }
        let mut request = vec![0; length];
        read(&mut input, &mut request)?;
        if request[0] != 3
            || !matches!(request[1], 1..=9 | 12..=21)
            || request[2..10] != expected.to_le_bytes()
        {
            return Err(io::Error::other("public sequence mismatch"));
        }
        expected = expected
            .checked_add(1)
            .ok_or_else(|| io::Error::other("sequence exhausted"))?;
        let reply = (|| -> io::Result<Vec<u8>> {
            if request[1] == 12 {
                let url = explorer::address_url(&request[10..])?;
                if self_test || preview { return Err(io::Error::other("browser disabled in test mode")); }
                explorer::open(&url)?;
                let mut reply = request[..10].to_vec();
                reply.extend(0u16.to_le_bytes());
                reply.extend(br#"{"opened":true}"#);
                let mut framed = (reply.len() as u32).to_le_bytes().to_vec();
                framed.extend(reply);
                return Ok(framed);
            }
            let proxy_argument = proxy.as_ref().map(|value| format!("--proxy={value}"));
            let mut arguments = vec!["--public-request"];
            if let Some(argument) = proxy_argument.as_deref() {
                arguments.push(argument);
            }
            // A wallet may have cached history in both direct and proxy modes.
            let alternate = if request[1] == 13 {
                let container = Container::open(if proxy.is_some() {
                    "Web3Rust.Network.v1"
                } else { "Web3Rust.ProxyNetwork.v1" })?;
                container.grant_execute(&root)?;
                container.grant_execute(&root.join("wallet-network.exe"))?;
                Some(container)
            } else { None };
            let mut last_response = Vec::new();
            for target in std::iter::once(&online).chain(alternate.as_ref()) {
            let mut worker = target.spawn_internet(&root.join("wallet-network.exe"), &arguments)?;
            {
                let mut pipe = worker.input.take().unwrap();
                pipe.write_all(&prefix)?;
                pipe.write_all(&request)?;
            }
            let worker_code = worker.wait(Duration::from_secs(45))?;
            let mut response = Vec::new();
            worker
                .output
                .take()
                .unwrap()
                .take(4096)
                .read_to_end(&mut response)?;
            if worker_code != 0 {
                if self_test {
                    eprintln!(
                        "Public fixture worker exit {worker_code}: {}",
                        String::from_utf8_lossy(&response)
                    );
                }
                return Err(io::Error::other("worker failure"));
            }
            if response.len() < 16
                || response.len() > 2052
                || response[4..14] != request[..10]
                || u32::from_le_bytes(response[..4].try_into().unwrap()) as usize
                    != response.len() - 4
            {
                return Err(io::Error::other("invalid worker response"));
            }
            if response[14..16] != [0, 0] { return Ok(response); }
            last_response = response;
            }
            Ok(last_response)
        })();
        let reply = reply.unwrap_or_else(|_| {
            let mut reply = request[..10].to_vec(); reply.extend(1u16.to_le_bytes());
            reply.extend(br#"{"error":"Network worker unavailable; query original hash if a broadcast was attempted"}"#);
            let mut framed = (reply.len() as u32).to_le_bytes().to_vec(); framed.extend(reply); framed
        });
        output.write_all(&reply)?;
        output.flush()?;
    }
}
