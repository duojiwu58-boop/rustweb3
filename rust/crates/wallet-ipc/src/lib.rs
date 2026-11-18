//! M0a public-data-only pipe protocol. Not an authenticated security boundary.
use std::io::{self, Read, Write};

pub const VERSION: u8 = 1;
pub const MAX_FRAME: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Signer = 1,
    Network = 2,
}

impl Role {
    pub fn executable(self) -> &'static str {
        match self {
            Self::Signer => "wallet-signer.exe",
            Self::Network => "wallet-network.exe",
        }
    }

    pub fn reply(self) -> &'static [u8] {
        match self {
            Self::Signer => "模拟签名进程：就绪（未启用密钥操作）".as_bytes(),
            Self::Network => "模拟联网进程：就绪（未启用联网）".as_bytes(),
        }
    }
}

pub fn write_frame(mut out: impl Write, bytes: &[u8]) -> io::Result<()> {
    if bytes.is_empty() || bytes.len() > MAX_FRAME {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    out.write_all(&(bytes.len() as u32).to_le_bytes())?;
    out.write_all(bytes)?;
    out.flush()
}

pub fn read_frame(mut input: impl Read) -> io::Result<Vec<u8>> {
    let mut length = [0; 4];
    input.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > MAX_FRAME {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut bytes = vec![0; length];
    input.read_exact(&mut bytes)?;
    Ok(bytes)
}

pub fn request(role: Role, id: u64) -> [u8; 10] {
    let mut bytes = [0; 10];
    bytes[0] = VERSION;
    bytes[1] = role as u8;
    bytes[2..].copy_from_slice(&id.to_le_bytes());
    bytes
}

/// One process handles exactly one public ping. Unknown commands never reach business code.
pub fn serve(role: Role, mut input: impl Read, output: impl Write) -> io::Result<()> {
    let frame = read_frame(&mut input)?;
    if frame.len() != 10 || frame[0] != VERSION || frame[1] != role as u8 {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut extra = [0];
    if input.read(&mut extra)? != 0 {
        return Err(io::ErrorKind::InvalidData.into());
    }
    write_frame(output, &frame)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unbounded_empty_and_truncated_frames() {
        for bytes in [vec![0, 0, 0, 0], vec![255; 4], vec![2, 0, 0, 0, 1]] {
            assert!(read_frame(bytes.as_slice()).is_err());
        }
        assert!(write_frame(Vec::new(), &[]).is_err());
        assert!(write_frame(Vec::new(), &[0; MAX_FRAME + 1]).is_err());
    }

    #[test]
    fn roles_versions_and_trailing_messages_are_rejected() {
        for bytes in [
            request(Role::Network, 1).to_vec(),
            vec![0; 10],
            vec![VERSION],
        ] {
            let mut wire = Vec::new();
            write_frame(&mut wire, &bytes).unwrap();
            assert!(serve(Role::Signer, wire.as_slice(), Vec::new()).is_err());
        }
        let mut wire = Vec::new();
        write_frame(&mut wire, &request(Role::Signer, 1)).unwrap();
        wire.push(0);
        assert!(serve(Role::Signer, wire.as_slice(), Vec::new()).is_err());
    }

    #[test]
    fn roundtrip_binds_role_and_request_id() {
        for role in [Role::Signer, Role::Network] {
            let expected = request(role, u64::MAX);
            let mut wire = Vec::new();
            write_frame(&mut wire, &expected).unwrap();
            let mut reply = Vec::new();
            serve(role, wire.as_slice(), &mut reply).unwrap();
            assert_eq!(read_frame(reply.as_slice()).unwrap(), expected);
        }
    }
}
