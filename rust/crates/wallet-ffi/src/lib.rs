//! Public mock ABI only. No wallet creation, import, unlock, signing or broadcasting.
//!
//! # Safety
//! All pointer arguments must refer to live, correctly aligned memory of the
//! documented size. Output slots must be exclusively writable for the call.
//! Input and output regions must not overlap. Calls may run concurrently, but
//! buffer reads must finish before freeing that allocation. Invalid non-null
//! pointers are caller errors and cannot be made safe by catching Rust panics.
#![allow(clippy::missing_safety_doc)] // Shared pointer contract above and docs/abi.md.

mod network;
mod preview;
mod secure;
mod worker;

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use wallet_ipc::Role;

pub const OK: i32 = 0;
pub const PENDING: i32 = 1;
pub const INVALID_ARGUMENT: i32 = -1;
pub const INVALID_HANDLE: i32 = -2;
pub const NOT_FOUND: i32 = -3;
pub const CANCELLED: i32 = -4;
pub const IO_ERROR: i32 = -5;
pub const PROTOCOL_ERROR: i32 = -6;
pub const TIMEOUT: i32 = -7;
pub const LIMIT: i32 = -8;
pub const INTERNAL: i32 = -9;

struct Task {
    cancel: Arc<AtomicBool>,
    worker: JoinHandle<Result<Vec<u8>, i32>>,
}
struct Context {
    directory: PathBuf,
    tasks: HashMap<u64, Task>,
}
impl Drop for Context {
    fn drop(&mut self) {
        for task in self.tasks.values() {
            task.cancel.store(true, Ordering::Release);
        }
        for (_, task) in self.tasks.drain() {
            let _ = task.worker.join();
        }
    }
}

type Buffers = HashMap<usize, Box<[u8]>>;
static CONTEXTS: OnceLock<Mutex<HashMap<u64, Context>>> = OnceLock::new();
static BUFFERS: OnceLock<Mutex<Buffers>> = OnceLock::new();
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn contexts() -> &'static Mutex<HashMap<u64, Context>> {
    CONTEXTS.get_or_init(Mutex::default)
}
fn buffers() -> &'static Mutex<Buffers> {
    BUFFERS.get_or_init(Mutex::default)
}
fn next_id() -> Result<u64, i32> {
    NEXT_ID
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .map_err(|_| LIMIT)
}
fn boundary(f: impl FnOnce() -> Result<(), i32>) -> i32 {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(())) => OK,
        Ok(Err(code)) => code,
        Err(_) => INTERNAL,
    }
}
unsafe fn input<'a>(data: *const u8, len: u32, max: u32) -> Result<&'a str, i32> {
    if data.is_null() || len == 0 || len > max {
        return Err(INVALID_ARGUMENT);
    }
    let bytes = unsafe { std::slice::from_raw_parts(data, len as usize) };
    let value = std::str::from_utf8(bytes).map_err(|_| INVALID_ARGUMENT)?;
    if value.contains('\0') {
        return Err(INVALID_ARGUMENT);
    }
    Ok(value)
}

#[unsafe(no_mangle)]
pub extern "C" fn w3_version() -> u32 {
    1
}

/// Stateless, bounded public-data preview. Never creates a signing intent.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn w3_preview_transfer(
    asset: u32,
    recipient: *const u8,
    recipient_len: u32,
    amount: *const u8,
    amount_len: u32,
    out: *mut *const u8,
    len: *mut u32,
) -> i32 {
    boundary(|| {
        if out.is_null() || len.is_null() {
            return Err(INVALID_ARGUMENT);
        }
        unsafe {
            *out = std::ptr::null();
            *len = 0;
        }
        let recipient = unsafe { input(recipient, recipient_len, 64)? };
        let amount = unsafe { input(amount, amount_len, 157)? };
        let bytes = preview::model(asset, recipient, amount)?
            .into_bytes()
            .into_boxed_slice();
        let mut buffers = buffers().lock().map_err(|_| INTERNAL)?;
        if buffers.len() >= 1024 {
            return Err(LIMIT);
        }
        let pointer = bytes.as_ptr();
        let length = bytes.len() as u32;
        buffers.insert(pointer as usize, bytes);
        unsafe {
            *out = pointer;
            *len = length;
        }
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn w3_context_create(path: *const u8, len: u32, out: *mut u64) -> i32 {
    boundary(|| {
        if out.is_null() {
            return Err(INVALID_ARGUMENT);
        }
        unsafe {
            *out = 0;
        }
        let path = PathBuf::from(unsafe { input(path, len, 32768)? });
        if !path.is_absolute() {
            return Err(INVALID_ARGUMENT);
        }
        let directory = path.canonicalize().map_err(|_| IO_ERROR)?;
        for role in [Role::Signer, Role::Network] {
            if !directory.join(role.executable()).is_file() {
                return Err(IO_ERROR);
            }
        }
        let mut contexts = contexts().lock().map_err(|_| INTERNAL)?;
        if contexts.len() >= 32 {
            return Err(LIMIT);
        }
        let id = next_id()?;
        contexts.insert(
            id,
            Context {
                directory,
                tasks: HashMap::new(),
            },
        );
        unsafe {
            *out = id;
        }
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn w3_context_destroy(context: u64) -> i32 {
    boundary(|| {
        let removed = contexts()
            .lock()
            .map_err(|_| INTERNAL)?
            .remove(&context)
            .ok_or(INVALID_HANDLE)?;
        drop(removed); // Registry lock released before cancelling/joining workers.
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn w3_task_submit(
    context: u64,
    data: *const u8,
    len: u32,
    out: *mut u64,
) -> i32 {
    boundary(|| {
        if out.is_null() {
            return Err(INVALID_ARGUMENT);
        }
        unsafe {
            *out = 0;
        }
        let role = match unsafe { input(data, len, 32)? } {
            "signer.ping" => Role::Signer,
            "network.ping" => Role::Network,
            _ => return Err(INVALID_ARGUMENT),
        };
        let mut contexts = contexts().lock().map_err(|_| INTERNAL)?;
        let context = contexts.get_mut(&context).ok_or(INVALID_HANDLE)?;
        if context.tasks.len() >= 32 {
            return Err(LIMIT);
        }
        let id = next_id()?;
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let directory = context.directory.clone();
        let worker = thread::Builder::new()
            .name("w3-mock".into())
            .spawn(move || worker::run(&directory, role, id, &worker_cancel))
            .map_err(|_| IO_ERROR)?;
        context.tasks.insert(id, Task { cancel, worker });
        unsafe {
            *out = id;
        }
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn w3_task_poll(
    context: u64,
    task: u64,
    out: *mut *const u8,
    len: *mut u32,
) -> i32 {
    boundary(|| {
        if out.is_null() || len.is_null() {
            return Err(INVALID_ARGUMENT);
        }
        unsafe {
            *out = std::ptr::null();
            *len = 0;
        }
        let mut contexts = contexts().lock().map_err(|_| INTERNAL)?;
        let context = contexts.get_mut(&context).ok_or(INVALID_HANDLE)?;
        let task_ref = context.tasks.get(&task).ok_or(NOT_FOUND)?;
        if !task_ref.worker.is_finished() {
            return Err(PENDING);
        }
        let mut buffers = buffers().lock().map_err(|_| INTERNAL)?;
        if buffers.len() >= 1024 {
            return Err(LIMIT);
        } // Keep task available for retry.
        let task = context.tasks.remove(&task).ok_or(NOT_FOUND)?;
        let result = task.worker.join().map_err(|_| INTERNAL)?;
        if task.cancel.load(Ordering::Acquire) {
            return Err(CANCELLED);
        }
        let bytes = result?.into_boxed_slice();
        let pointer = bytes.as_ptr();
        let length = bytes.len() as u32;
        buffers.insert(pointer as usize, bytes);
        unsafe {
            *out = pointer;
            *len = length;
        }
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn w3_task_cancel(context: u64, task: u64) -> i32 {
    boundary(|| {
        let contexts = contexts().lock().map_err(|_| INTERNAL)?;
        let context = contexts.get(&context).ok_or(INVALID_HANDLE)?;
        let task = context.tasks.get(&task).ok_or(NOT_FOUND)?;
        task.cancel.store(true, Ordering::Release);
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn w3_context_lock(context: u64) -> i32 {
    boundary(|| {
        let contexts = contexts().lock().map_err(|_| INTERNAL)?;
        let context = contexts.get(&context).ok_or(INVALID_HANDLE)?;
        for task in context.tasks.values() {
            task.cancel.store(true, Ordering::Release);
        }
        Ok(()) // Mock has no secrets; future public pings remain permitted.
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn w3_buffer_free(data: *const u8, len: u32) -> i32 {
    boundary(|| {
        if data.is_null() {
            return if len == 0 {
                Ok(())
            } else {
                Err(INVALID_ARGUMENT)
            };
        }
        let mut buffers = buffers().lock().map_err(|_| INTERNAL)?;
        let bytes = buffers.get(&(data as usize)).ok_or(INVALID_ARGUMENT)?;
        if bytes.len() != len as usize {
            return Err(INVALID_ARGUMENT);
        }
        buffers.remove(&(data as usize));
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn boundary_converts_panics_to_error() {
        assert_eq!(boundary(|| panic!("public test panic")), INTERNAL);
    }
    #[test]
    fn invalid_arguments_do_not_escape_abi() {
        unsafe {
            let mut id = 99;
            assert_eq!(
                w3_context_create(std::ptr::null(), 1, &mut id),
                INVALID_ARGUMENT
            );
            assert_eq!(id, 0);
            assert_eq!(
                w3_task_submit(0, [255].as_ptr(), 1, &mut id),
                INVALID_ARGUMENT
            );
            assert_eq!(
                w3_task_submit(0, b"signer.ping".as_ptr(), u32::MAX, &mut id),
                INVALID_ARGUMENT
            );
            assert_eq!(
                w3_task_submit(0, b"signer.ping".as_ptr(), 11, &mut id),
                INVALID_HANDLE
            );
        }
        assert_eq!(w3_context_destroy(0), INVALID_HANDLE);
        assert_eq!(w3_buffer_free(std::ptr::null(), 0), OK);
        assert_eq!(w3_buffer_free(std::ptr::null(), 1), INVALID_ARGUMENT);
    }
}
