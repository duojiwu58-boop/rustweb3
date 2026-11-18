//! Separate caller-owned sensitive ABI buffers. No public task JSON or buffer registry.
use crate::{INTERNAL, INVALID_ARGUMENT, IO_ERROR, PROTOCOL_ERROR, TIMEOUT, boundary};
use std::{
    io::{Read, Write},
    os::windows::{io::AsRawHandle, process::CommandExt},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use windows_sys::Win32::System::Pipes::PeekNamedPipe;
use zeroize::Zeroizing;

struct Client {
    process: Child,
    input: ChildStdin,
    output: ChildStdout,
    next: u64,
    epoch: u64,
}
impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}
static CLIENT: OnceLock<Mutex<Option<Client>>> = OnceLock::new();
static CANCEL: AtomicBool = AtomicBool::new(false);
static EPOCH: AtomicU64 = AtomicU64::new(1);
fn client() -> &'static Mutex<Option<Client>> {
    CLIENT.get_or_init(Mutex::default)
}

#[unsafe(no_mangle)]
pub extern "C" fn w3_secure_open() -> i32 {
    boundary(|| {
        wallet_sandbox::verify_offline_token().map_err(|_| -20)?;
        let mut guard = client().lock().map_err(|_| INTERNAL)?;
        *guard = None;
        let directory = std::env::current_exe()
            .map_err(|_| IO_ERROR)?
            .parent()
            .ok_or(IO_ERROR)?
            .to_owned();
        let mut process = Command::new(directory.join("wallet-signer.exe"))
            .arg("--vault-session")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(0x08000000)
            .spawn()
            .map_err(|_| IO_ERROR)?;
        let input = process.stdin.take().ok_or(IO_ERROR)?;
        let output = process.stdout.take().ok_or(IO_ERROR)?;
        *guard = Some(Client {
            process,
            input,
            output,
            next: 1,
            epoch: EPOCH.fetch_add(1, Ordering::AcqRel) + 1,
        });
        CANCEL.store(false, Ordering::Release);
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn w3_secure_close() -> i32 {
    boundary(|| {
        EPOCH.fetch_add(1, Ordering::AcqRel);
        CANCEL.store(true, Ordering::Release);
        *client().lock().unwrap_or_else(|poison| poison.into_inner()) = None;
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn w3_secure_epoch() -> u64 {
    EPOCH.load(Ordering::Acquire)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn w3_secure_exchange(
    epoch: u64,
    opcode: u32,
    data: *const u8,
    length: u32,
    output: *mut u8,
    capacity: u32,
    written: *mut u32,
) -> i32 {
    boundary(|| {
        if written.is_null() {
            return Err(INVALID_ARGUMENT);
        }
        unsafe {
            *written = 0;
        }
        if !(1..=35).contains(&opcode)
            || length > 2038
            || (length > 0 && data.is_null())
            || output.is_null()
            || capacity != 2048
        {
            return Err(INVALID_ARGUMENT);
        }
        unsafe {
            std::ptr::write_bytes(output, 0, capacity as usize);
        }
        let mut guard = client().lock().map_err(|_| INTERNAL)?;
        let connection = guard.as_mut().ok_or(-21)?;
        if connection.epoch != epoch || CANCEL.load(Ordering::Acquire) {
            return Err(crate::CANCELLED);
        }
        let body = if length == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(data, length as usize) }
        };
        let result = connection.exchange(opcode as u8, body);
        match result {
            Ok((status, reply)) => {
                if status != 0 {
                    return Err(-30 - i32::from(status));
                }
                unsafe {
                    std::ptr::copy_nonoverlapping(reply.as_ptr(), output, reply.len());
                    *written = reply.len() as u32;
                }
                Ok(())
            }
            Err(code) => {
                *guard = None;
                Err(code)
            }
        }
    })
}
impl Client {
    fn read(&mut self, mut target: &mut [u8], deadline: Instant) -> Result<(), i32> {
        while !target.is_empty() {
            if CANCEL.load(Ordering::Acquire) {
                return Err(crate::CANCELLED);
            }
            if Instant::now() >= deadline {
                return Err(TIMEOUT);
            }
            let mut available = 0;
            let ok = unsafe {
                PeekNamedPipe(
                    self.output.as_raw_handle(),
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                    &mut available,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                return Err(IO_ERROR);
            }
            if available == 0 {
                if self.process.try_wait().map_err(|_| IO_ERROR)?.is_some() {
                    return Err(IO_ERROR);
                }
                std::thread::sleep(Duration::from_millis(5));
                continue;
            }
            let count = target.len().min(available as usize);
            self.output
                .read_exact(&mut target[..count])
                .map_err(|_| IO_ERROR)?;
            target = &mut target[count..];
        }
        Ok(())
    }
    fn exchange(&mut self, opcode: u8, body: &[u8]) -> Result<(u16, Zeroizing<Vec<u8>>), i32> {
        let id = self.next;
        self.next = self.next.checked_add(1).ok_or(PROTOCOL_ERROR)?;
        let mut request = Zeroizing::new(Vec::with_capacity(14 + body.len()));
        request.extend(((10 + body.len()) as u32).to_le_bytes());
        request.extend([2, opcode]);
        request.extend(id.to_le_bytes());
        request.extend_from_slice(body);
        self.input.write_all(&request).map_err(|_| IO_ERROR)?;
        self.input.flush().map_err(|_| IO_ERROR)?;
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut prefix = [0; 4];
        self.read(&mut prefix, deadline)?;
        let length = u32::from_le_bytes(prefix) as usize;
        if !(12..=2048).contains(&length) {
            return Err(PROTOCOL_ERROR);
        }
        let mut reply = Zeroizing::new(vec![0; length]);
        self.read(&mut reply, deadline)?;
        if reply[0] != 2 || reply[1] != opcode || reply[2..10] != id.to_le_bytes() {
            return Err(PROTOCOL_ERROR);
        }
        let status = u16::from_le_bytes(reply[10..12].try_into().unwrap());
        Ok((status, Zeroizing::new(reply[12..].to_vec())))
    }
}
