use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use wallet_ipc::{Role, read_frame, request, write_frame};

use crate::{CANCELLED, IO_ERROR, PROTOCOL_ERROR, TIMEOUT};

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        // Reap on every exit path, including panic and cancellation.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub fn run(directory: &Path, role: Role, id: u64, cancel: &AtomicBool) -> Result<Vec<u8>, i32> {
    if cancel.load(Ordering::Acquire) {
        return Err(CANCELLED);
    }
    let mut command = Command::new(directory.join(role.executable()));
    command
        .current_dir(directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let mut child = OwnedChild(command.spawn().map_err(|_| IO_ERROR)?);
    let expected = request(role, id);
    {
        let input = child.0.stdin.take().ok_or(IO_ERROR)?;
        write_frame(input, &expected).map_err(|_| IO_ERROR)?;
    } // Close stdin: server rejects trailing bytes and then replies.
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if cancel.load(Ordering::Acquire) {
            return Err(CANCELLED);
        }
        if let Some(status) = child.0.try_wait().map_err(|_| IO_ERROR)? {
            if !status.success() {
                return Err(PROTOCOL_ERROR);
            }
            break;
        }
        if Instant::now() >= deadline {
            return Err(TIMEOUT);
        }
        thread::sleep(Duration::from_millis(5));
    }
    // Protocol reply is 14 bytes, comfortably below pipe capacity. An oversized
    // peer blocks and is killed by the deadline instead of allocating unboundedly.
    let mut output = child.0.stdout.take().ok_or(IO_ERROR)?;
    let reply = read_frame(&mut output).map_err(|_| PROTOCOL_ERROR)?;
    let mut extra = [0];
    if reply != expected || output.read(&mut extra).map_err(|_| IO_ERROR)? != 0 {
        return Err(PROTOCOL_ERROR);
    }
    Ok(role.reply().to_vec())
}
