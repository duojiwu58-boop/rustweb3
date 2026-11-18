use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use web3rust::*;

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);
impl Fixture {
    fn new(mode: &str) -> Self {
        let base = std::env::temp_dir().join(format!(
            "web3rust-m0a-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let directory = base.join(mode);
        std::fs::create_dir_all(&directory).unwrap();
        let peer = std::env::var_os("W3_TEST_FAULT_PEER").expect("run scripts/verify-m0a.ps1");
        for name in ["wallet-signer.exe", "wallet-network.exe"] {
            std::fs::copy(&peer, directory.join(name)).unwrap();
        }
        Self(directory)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Only this test's unique directory; handles must be closed first.
        std::fs::remove_dir_all(self.0.parent().unwrap()).unwrap();
    }
}

struct Context(u64);
impl Context {
    fn new(path: &Path) -> Self {
        let bytes = path.to_str().unwrap().as_bytes();
        let mut id = 0;
        assert_eq!(
            unsafe { w3_context_create(bytes.as_ptr(), bytes.len() as u32, &mut id) },
            OK
        );
        Self(id)
    }
    fn submit(&self) -> u64 {
        let mut id = 0;
        assert_eq!(
            unsafe { w3_task_submit(self.0, b"signer.ping".as_ptr(), 11, &mut id) },
            OK
        );
        id
    }
    fn await_error(&self, task: u64) -> i32 {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let mut pointer = std::ptr::null();
            let mut len = 0;
            let code = unsafe { w3_task_poll(self.0, task, &mut pointer, &mut len) };
            assert!(pointer.is_null());
            assert_eq!(len, 0);
            if code != PENDING {
                return code;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Drop for Context {
    fn drop(&mut self) {
        assert_eq!(w3_context_destroy(self.0), OK);
    }
}

#[test]
#[ignore = "requires compiled fault peer; verify-m0a.ps1 runs this"]
fn failed_peers_are_bounded_and_reaped() {
    for (mode, expected) in [
        ("exit", PROTOCOL_ERROR),
        ("wrong-id", PROTOCOL_ERROR),
        ("timeout", TIMEOUT),
        ("oversized", TIMEOUT),
    ] {
        let fixture = Fixture::new(mode);
        let context = Context::new(&fixture.0);
        assert_eq!(context.await_error(context.submit()), expected, "{mode}");
    }
}

#[test]
#[ignore = "requires compiled fault peer; verify-m0a.ps1 runs this"]
fn cancellation_destruction_limits_and_stale_handles() {
    let fixture = Fixture::new("timeout");
    let context = Context::new(&fixture.0);
    let task = context.submit();
    assert_eq!(w3_task_cancel(context.0, task), OK);
    assert_eq!(context.await_error(task), CANCELLED);
    for _ in 0..32 {
        context.submit();
    }
    let mut task = 99;
    assert_eq!(
        unsafe { w3_task_submit(context.0, b"signer.ping".as_ptr(), 11, &mut task) },
        LIMIT
    );
    assert_eq!(task, 0);
    let id = context.0;
    drop(context);
    assert_eq!(w3_context_destroy(id), INVALID_HANDLE);
}

#[test]
#[ignore = "requires compiled fault peer; verify-m0a.ps1 runs this"]
fn missing_peer_after_creation_is_an_io_error() {
    let fixture = Fixture::new("exit");
    let context = Context::new(&fixture.0);
    std::fs::remove_file(fixture.0.join("wallet-signer.exe")).unwrap();
    assert_eq!(context.await_error(context.submit()), IO_ERROR);
}
