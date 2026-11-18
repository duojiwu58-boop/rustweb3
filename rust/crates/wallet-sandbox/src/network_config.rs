//! User-editable transport settings, read by the broker before launching workers.
use std::{io, path::Path};

pub fn validate_proxy(value: &str) -> io::Result<String> {
    let authority = value.strip_prefix("http://").unwrap_or(value);
    let (host, port) = authority
        .rsplit_once(':')
        .ok_or_else(|| io::Error::other("proxy must be http://host:port"))?;
    if host.is_empty()
        || host.len() > 253
        || !host
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-'))
        || port.parse::<u16>().ok().filter(|p| *p > 0).is_none()
        || !port.bytes().all(|c| c.is_ascii_digit())
    {
        return Err(io::Error::other("invalid HTTP proxy host or port"));
    }
    Ok(authority.to_owned())
}

pub fn load(directory: &Path) -> io::Result<Option<String>> {
    let path = directory.join("network.ini");
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    parse(&text)
}

fn parse(text: &str) -> io::Result<Option<String>> {
    if text.len() > 4096 {
        return Err(io::Error::other("network.ini too large"));
    }
    let mut proxy = None;
    let mut found = false;
    for line in text.trim_start_matches('\u{feff}').lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') || line == "[network]"
        {
            continue;
        }
        let value = line
            .strip_prefix("proxy=")
            .or_else(|| line.strip_prefix("proxy ="))
            .ok_or_else(|| io::Error::other("network.ini: expected proxy=http://host:port"))?
            .trim();
        if found {
            return Err(io::Error::other("network.ini: duplicate proxy"));
        }
        found = true;
        if !value.is_empty() {
            proxy = Some(validate_proxy(value)?);
        }
    }
    if !found {
        return Err(io::Error::other("network.ini: missing proxy setting"));
    }
    Ok(proxy)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn editable_proxy_and_direct_mode() {
        assert_eq!(
            parse("\u{feff}; note\n[network]\nproxy=http://127.0.0.1:8080\n")
                .unwrap()
                .as_deref(),
            Some("127.0.0.1:8080")
        );
        assert_eq!(parse("proxy = \n").unwrap(), None);
        for bad in [
            "proxy=socks5://localhost:8080",
            "proxy=http://user:pass@host:80",
            "proxy=localhost:0",
            "proxy=host:65536",
            "proxy=host:80/path",
            "proxy=host:80\nproxy=",
            "unknown=yes",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }
}
