//! Offline EVM transfer codec. No private-key or network API.
use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
use rlp::{Rlp, RlpStream};
use sha3::{Digest, Keccak256};
use wallet_core::{Amount, Error, Result};

pub mod bsc;

pub fn keccak256(bytes: &[u8]) -> [u8; 32] {
    Keccak256::digest(bytes).into()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Address([u8; 20]);
impl Address {
    pub const fn from_bytes(bytes: [u8; 20]) -> Self {
        Self(bytes)
    }
    pub const fn bytes(self) -> [u8; 20] {
        self.0
    }
    pub fn from_public_key(sec1: &[u8]) -> Result<Self> {
        let key = VerifyingKey::from_sec1_bytes(sec1).map_err(|_| Error::InvalidAddress)?;
        let point = key.to_sec1_point(false);
        let hash = keccak256(&point.as_bytes()[1..]);
        Ok(Self(hash[12..].try_into().expect("fixed width")))
    }
    pub fn parse(text: &str) -> Result<Self> {
        if text.len() != 42 || !text.starts_with("0x") {
            return Err(Error::InvalidAddress);
        }
        let mut bytes = [0; 20];
        hex::decode_to_slice(&text[2..], &mut bytes).map_err(|_| Error::InvalidAddress)?;
        let address = Self(bytes);
        let has_lower = text[2..].bytes().any(|b| b.is_ascii_lowercase());
        let has_upper = text[2..].bytes().any(|b| b.is_ascii_uppercase());
        if has_lower && has_upper && address.checksum() != text {
            return Err(Error::InvalidAddress);
        }
        Ok(address)
    }
    pub fn checksum(self) -> String {
        let lower = hex::encode(self.0);
        let hash = keccak256(lower.as_bytes());
        let mut out = String::from("0x");
        for (i, byte) in lower.bytes().enumerate() {
            let nibble = if i % 2 == 0 {
                hash[i / 2] >> 4
            } else {
                hash[i / 2] & 15
            };
            out.push(if nibble >= 8 {
                byte.to_ascii_uppercase()
            } else {
                byte
            } as char);
        }
        out
    }
}

/// Verify a canonical low-s signature and recover its public address; never signs.
pub fn recover_address(digest: &[u8; 32], signature: &[u8; 65]) -> Result<Address> {
    if signature[64] > 1 {
        return Err(Error::InvalidSignature);
    }
    let sig = Signature::from_slice(&signature[..64]).map_err(|_| Error::InvalidSignature)?;
    if sig.normalize_s() != sig {
        return Err(Error::InvalidSignature);
    }
    let id = RecoveryId::from_byte(signature[64]).ok_or(Error::InvalidSignature)?;
    let key = VerifyingKey::recover_from_prehash(digest, &sig, id)
        .map_err(|_| Error::InvalidSignature)?;
    Address::from_public_key(key.to_sec1_point(false).as_bytes())
}

pub fn encode_transfer_call(recipient: Address, amount: Amount) -> Result<[u8; 68]> {
    if amount == Amount::ZERO {
        return Err(Error::InvalidAmount);
    }
    let mut data = [0; 68];
    data[..4].copy_from_slice(&[0xa9, 0x05, 0x9c, 0xbb]);
    data[16..36].copy_from_slice(&recipient.0);
    data[36..].copy_from_slice(&amount.to_be_bytes());
    Ok(data)
}
pub fn decode_transfer_call(data: &[u8]) -> Result<(Address, Amount)> {
    if data.len() != 68
        || data[..4] != [0xa9, 0x05, 0x9c, 0xbb]
        || data[4..16].iter().any(|b| *b != 0)
    {
        return Err(Error::UnsupportedTransfer);
    }
    let amount = Amount::from_be_bytes(data[36..].try_into().expect("checked length"));
    if amount == Amount::ZERO {
        return Err(Error::InvalidAmount);
    }
    Ok((
        Address(data[16..36].try_into().expect("checked length")),
        amount,
    ))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Transfer {
    Native {
        recipient: Address,
        amount: Amount,
    },
    Token {
        contract: Address,
        recipient: Address,
        amount: Amount,
    },
}
#[derive(Clone, Copy)]
pub struct Policy {
    /// Trusted caller configuration; production BSC whitelist is a later milestone.
    pub chain_id: u64,
    pub token_contract: Option<Address>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyTransfer {
    pub chain_id: u64,
    pub nonce: u64,
    pub gas_price: Amount,
    pub gas_limit: u64,
    pub transfer: Transfer,
}

/// Parse and authenticate only the transfer subset accepted by the fixed caller policy.
pub fn decode_signed_transfer(bytes: &[u8], policy: Policy) -> Result<(LegacyTransfer, Address)> {
    if bytes.len() > 512 {
        return Err(Error::InvalidEncoding);
    }
    let signed = Rlp::new(bytes);
    if signed.item_count().map_err(|_| Error::InvalidEncoding)? != 9 {
        return Err(Error::InvalidEncoding);
    }
    let v: u64 = signed.val_at(6).map_err(|_| Error::InvalidEncoding)?;
    if v < 35 || (v - 35) / 2 != policy.chain_id {
        return Err(Error::WrongNetwork);
    }
    let mut signature = [0; 65];
    signature[64] = ((v - 35) % 2) as u8;
    for (index, offset) in [(7, 0), (8, 32)] {
        let item = signed.at(index).map_err(|_| Error::InvalidEncoding)?;
        let value = item.data().map_err(|_| Error::InvalidEncoding)?;
        if value.is_empty() || value.len() > 32 || value[0] == 0 {
            return Err(Error::InvalidSignature);
        }
        signature[offset + 32 - value.len()..offset + 32].copy_from_slice(value);
    }
    let mut unsigned = RlpStream::new_list(9);
    for index in 0..6 {
        unsigned.append_raw(
            signed
                .at(index)
                .map_err(|_| Error::InvalidEncoding)?
                .as_raw(),
            1,
        );
    }
    unsigned.append(&policy.chain_id).append(&0u8).append(&0u8);
    let tx = LegacyTransfer::decode_signing_bytes(&unsigned.out(), policy)?;
    let sender = recover_address(&tx.signing_hash(policy)?, &signature)?;
    if tx.signed_bytes(policy, sender, &signature)? != bytes {
        return Err(Error::InvalidEncoding);
    }
    Ok((tx, sender))
}

fn minimal(bytes: &[u8]) -> &[u8] {
    &bytes[bytes.iter().position(|b| *b != 0).unwrap_or(bytes.len())..]
}
fn append_amount(stream: &mut RlpStream, amount: Amount) {
    stream.append(&minimal(&amount.to_be_bytes()));
}
impl LegacyTransfer {
    fn fields(&self, policy: Policy) -> Result<(Address, Amount, Vec<u8>)> {
        if self.chain_id != policy.chain_id || self.chain_id == 0 {
            return Err(Error::WrongNetwork);
        }
        if self.gas_limit < 21000 || self.gas_price == Amount::ZERO {
            return Err(Error::InvalidAmount);
        }
        match self.transfer {
            Transfer::Native { recipient, amount } if amount != Amount::ZERO => {
                Ok((recipient, amount, vec![]))
            }
            Transfer::Token {
                contract,
                recipient,
                amount,
            } if Some(contract) == policy.token_contract => Ok((
                contract,
                Amount::ZERO,
                encode_transfer_call(recipient, amount)?.to_vec(),
            )),
            _ => Err(Error::UnsupportedTransfer),
        }
    }
    fn prefix(&self, policy: Policy) -> Result<RlpStream> {
        let (to, value, data) = self.fields(policy)?;
        let mut stream = RlpStream::new_list(9);
        stream.append(&self.nonce);
        append_amount(&mut stream, self.gas_price);
        stream.append(&self.gas_limit).append(&to.0.as_slice());
        append_amount(&mut stream, value);
        stream.append(&data);
        Ok(stream)
    }
    pub fn signing_bytes(&self, policy: Policy) -> Result<Vec<u8>> {
        let mut stream = self.prefix(policy)?;
        stream.append(&self.chain_id).append(&0u8).append(&0u8);
        Ok(stream.out().to_vec())
    }
    pub fn signing_hash(&self, policy: Policy) -> Result<[u8; 32]> {
        Ok(keccak256(&self.signing_bytes(policy)?))
    }
    /// Accept only a signature that recovers the independently confirmed sender.
    pub fn signed_bytes(
        &self,
        policy: Policy,
        sender: Address,
        signature: &[u8; 65],
    ) -> Result<Vec<u8>> {
        if recover_address(&self.signing_hash(policy)?, signature)? != sender {
            return Err(Error::InvalidSignature);
        }
        let v = self
            .chain_id
            .checked_mul(2)
            .and_then(|v| v.checked_add(35 + signature[64] as u64))
            .ok_or(Error::Overflow)?;
        let mut stream = self.prefix(policy)?;
        stream
            .append(&v)
            .append(&minimal(&signature[..32]))
            .append(&minimal(&signature[32..64]));
        Ok(stream.out().to_vec())
    }
    pub fn decode_signing_bytes(bytes: &[u8], policy: Policy) -> Result<Self> {
        if bytes.len() > 512 {
            return Err(Error::InvalidEncoding);
        }
        let rlp = Rlp::new(bytes);
        let invalid = |_| Error::InvalidEncoding;
        if rlp.item_count().map_err(invalid)? != 9 {
            return Err(Error::InvalidEncoding);
        }
        let amount_at = |index| -> Result<Amount> {
            let item = rlp.at(index).map_err(invalid)?;
            let data = item.data().map_err(invalid)?;
            if data.len() > 32 || data.first() == Some(&0) {
                return Err(Error::InvalidEncoding);
            }
            let mut out = [0; 32];
            out[32 - data.len()..].copy_from_slice(data);
            Ok(Amount::from_be_bytes(out))
        };
        let to = Address(
            rlp.at(3)
                .map_err(invalid)?
                .data()
                .map_err(invalid)?
                .try_into()
                .map_err(|_| Error::InvalidAddress)?,
        );
        let value = amount_at(4)?;
        let data: Vec<u8> = rlp.val_at(5).map_err(invalid)?;
        let transfer = if data.is_empty() {
            Transfer::Native {
                recipient: to,
                amount: value,
            }
        } else {
            if value != Amount::ZERO || Some(to) != policy.token_contract {
                return Err(Error::UnsupportedTransfer);
            }
            let (recipient, amount) = decode_transfer_call(&data)?;
            Transfer::Token {
                contract: to,
                recipient,
                amount,
            }
        };
        let result = Self {
            chain_id: rlp.val_at(6).map_err(invalid)?,
            nonce: rlp.val_at(0).map_err(invalid)?,
            gas_price: amount_at(1)?,
            gas_limit: rlp.val_at(2).map_err(invalid)?,
            transfer,
        };
        // Exact re-encoding rejects trailing bytes, signature fields, nonminimal integers and RLP forms.
        if result.signing_bytes(policy)? != bytes {
            return Err(Error::InvalidEncoding);
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
