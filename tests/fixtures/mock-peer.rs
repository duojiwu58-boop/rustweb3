// Test-only fault injector, never copied into the application bin directory.
use std::io::{Read, Write};
fn main() {
    let directory = std::env::current_dir().unwrap();
    let mode = directory.file_name().unwrap().to_str().unwrap();
    match mode {
        "timeout" => std::thread::sleep(std::time::Duration::from_secs(30)),
        "exit" => std::process::exit(2),
        "wrong-id" => {
            let mut bytes = Vec::new();
            std::io::stdin().read_to_end(&mut bytes).unwrap();
            *bytes.last_mut().unwrap() ^= 1;
            std::io::stdout().write_all(&bytes).unwrap();
        }
        "oversized" => {
            let _ = std::io::stdout().write_all(&[255; 100_000]);
        }
        _ => std::process::exit(3),
    }
}
