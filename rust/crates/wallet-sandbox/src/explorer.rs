//! Narrow broker action: only a BscScan address page, never caller-supplied URLs.
use std::io;
pub fn address_url(data: &[u8]) -> io::Result<String> {
    if data.len()!=44 || data[..2]!=42u16.to_le_bytes() || &data[2..4]!=b"0x"
        || !data[4..].iter().all(u8::is_ascii_hexdigit) {
        return Err(io::Error::other("invalid explorer address"));
    }
    Ok(format!("https://bscscan.com/address/{}", std::str::from_utf8(&data[2..]).unwrap()))
}
pub fn open(url: &str) -> io::Result<()> {
    let verb: Vec<u16> = "open\0".encode_utf16().collect();
    let url: Vec<u16> = url.encode_utf16().chain(Some(0)).collect();
    let result = unsafe { windows_sys::Win32::UI::Shell::ShellExecuteW(
        std::ptr::null_mut(), verb.as_ptr(), url.as_ptr(), std::ptr::null(), std::ptr::null(),
        windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL) };
    if result as isize <=32 { return Err(io::Error::other("could not open default browser")); }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_fixed_host_and_one_hex_address() {
        let mut frame=42u16.to_le_bytes().to_vec();
        frame.extend(b"0x1111111111111111111111111111111111111111");
        assert_eq!(address_url(&frame).unwrap(),"https://bscscan.com/address/0x1111111111111111111111111111111111111111");
        frame[10]=b'/'; assert!(address_url(&frame).is_err());
        frame[10]=b'1'; frame.push(0); assert!(address_url(&frame).is_err());
        assert!(address_url(b"https://example.com").is_err());
    }
}
