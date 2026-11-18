//! Authenticated account count and selection; no mnemonic duplication.
use super::*;
pub(crate) const STATE: &str = "accounts.state";
pub(crate) const NEXT: &str = "accounts.next";
const DOMAIN: &[u8] = b"Web3Rust account state v1";

pub(crate) fn read(directory: &Path, material: &Material) -> Result<(u32, u32)> {
    let path = directory.join(STATE);
    if !path.try_exists().map_err(|_| Error::Storage)? { return Ok((1, 0)); }
    let mut bytes = Vec::new();
    File::open(path).map_err(|_| Error::Storage)?.take(49)
        .read_to_end(&mut bytes).map_err(|_| Error::Storage)?;
    if bytes.len() != 48 { return Err(Error::InvalidFormat); }
    let cipher = XChaCha20Poly1305::new_from_slice(&*material.key).map_err(|_| Error::Crypto)?;
    let value = Zeroizing::new(cipher.decrypt(&XNonce::try_from(&bytes[..24]).map_err(|_| Error::InvalidFormat)?,
        Payload { msg: &bytes[24..], aad: DOMAIN }).map_err(|_| Error::Authentication)?);
    let count = u32::from_le_bytes(value[..4].try_into().map_err(|_| Error::InvalidFormat)?);
    let selected = u32::from_le_bytes(value[4..].try_into().map_err(|_| Error::InvalidFormat)?);
    if count == 0 || count > MAX_ACCOUNTS || selected >= count { return Err(Error::InvalidFormat); }
    Ok((count, selected))
}

pub(crate) fn write(directory: &Path, material: &Material, count: u32, selected: u32) -> Result<()> {
    if count == 0 || count > MAX_ACCOUNTS || selected >= count { return Err(Error::InvalidFormat); }
    let cipher = XChaCha20Poly1305::new_from_slice(&*material.key).map_err(|_| Error::Crypto)?;
    let mut nonce = [0; 24]; random(&mut nonce)?;
    let mut value = count.to_le_bytes().to_vec(); value.extend(selected.to_le_bytes());
    let encrypted = cipher.encrypt(&XNonce::from(nonce), Payload { msg: &value, aad: DOMAIN }).map_err(|_| Error::Crypto)?;
    let mut bytes = nonce.to_vec(); bytes.extend(encrypted);
    // The profile lock is held; stale staging from an interrupted write is disposable.
    let mut file = OpenOptions::new().write(true).create(true).truncate(true)
        .open(directory.join(NEXT)).map_err(|_| Error::Storage)?;
    file.write_all(&bytes).and_then(|_| file.sync_all()).map_err(|_| Error::Storage)?;
    drop(file);
    storage::replace(directory, NEXT, STATE)
}
