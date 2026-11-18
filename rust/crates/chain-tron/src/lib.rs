//! Offline TRON transfer codec. No private-key or network API.
mod proto;
pub mod mainnet;
use chain_evm::{
    Address as EvmAddress, decode_transfer_call, encode_transfer_call, recover_address,
};
use prost::Message;
use sha2::{Digest, Sha256};
use wallet_core::{Amount, Error, Result};

const NATIVE_URL: &str = "type.googleapis.com/protocol.TransferContract";
const TRIGGER_URL: &str = "type.googleapis.com/protocol.TriggerSmartContract";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Address([u8; 21]);
impl Address {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != 21 || bytes[0] != 0x41 {
            return Err(Error::InvalidAddress);
        }
        Ok(Self(bytes.try_into().expect("checked length")))
    }
    pub fn from_public_key(sec1: &[u8]) -> Result<Self> {
        Ok(Self::from_evm(EvmAddress::from_public_key(sec1)?))
    }
    fn from_evm(address: EvmAddress) -> Self {
        let mut bytes = [0x41; 21];
        bytes[1..].copy_from_slice(&address.bytes());
        Self(bytes)
    }
    fn evm(self) -> EvmAddress {
        EvmAddress::from_bytes(self.0[1..].try_into().expect("fixed width"))
    }
    pub fn bytes(self) -> [u8; 21] {
        self.0
    }
    pub fn parse(text: &str) -> Result<Self> {
        if text.len() != 34 || !text.starts_with('T') {
            return Err(Error::InvalidAddress);
        }
        let bytes = bs58::decode(text)
            .with_check(Some(0x41))
            .into_vec()
            .map_err(|_| Error::InvalidAddress)?;
        Self::from_bytes(&bytes)
    }
    pub fn base58(self) -> String {
        bs58::encode(self.0).with_check().into_string()
    }
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
    /// Trusted whitelist; no production contract is hardcoded in this prototype.
    pub token_contract: Option<Address>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transaction {
    pub owner: Address,
    pub ref_block_bytes: [u8; 2],
    pub ref_block_hash: [u8; 8],
    pub timestamp_ms: i64,
    pub expiration_ms: i64,
    /// Energy limit only, not a total fee cap. Native transfers require zero.
    pub fee_limit: i64,
    pub transfer: Transfer,
}
fn strict_decode<M: Message + Default>(bytes: &[u8]) -> Result<M> {
    if bytes.is_empty() || bytes.len() > 1024 {
        return Err(Error::InvalidEncoding);
    }
    let value = M::decode(bytes).map_err(|_| Error::InvalidEncoding)?;
    if value.encode_to_vec() != bytes {
        return Err(Error::InvalidEncoding);
    }
    Ok(value)
}
impl Transaction {
    /// Decode the exact supported signed envelope and verify owner/signature.
    pub fn decode_signed(bytes: &[u8], policy: Policy) -> Result<Self> {
        let signed: proto::Signed = strict_decode(bytes)?;
        if signed.signatures.len() != 1 { return Err(Error::InvalidSignature); }
        let raw = signed.raw.ok_or(Error::InvalidEncoding)?.encode_to_vec();
        let tx = Self::decode_raw(&raw, policy)?;
        let signature: [u8; 65] = signed.signatures[0].as_slice().try_into().map_err(|_| Error::InvalidSignature)?;
        if tx.signed_bytes(policy, &signature)? != bytes { return Err(Error::InvalidEncoding); }
        Ok(tx)
    }
    fn raw(&self, policy: Policy) -> Result<proto::Raw> {
        let ttl = self
            .expiration_ms
            .checked_sub(self.timestamp_ms)
            .ok_or(Error::InvalidEncoding)?;
        if self.timestamp_ms <= 0 || !(1..=86_400_000).contains(&ttl) || self.fee_limit < 0 {
            return Err(Error::InvalidEncoding);
        }
        let (kind, type_url, value) = match self.transfer {
            Transfer::Native { recipient, amount } => {
                let amount = i64::try_from(amount.to_u64()?).map_err(|_| Error::Overflow)?;
                if amount == 0 || self.fee_limit != 0 {
                    return Err(Error::InvalidAmount);
                }
                (
                    1,
                    NATIVE_URL,
                    proto::Native {
                        owner: self.owner.0.to_vec(),
                        recipient: recipient.0.to_vec(),
                        amount,
                    }
                    .encode_to_vec(),
                )
            }
            Transfer::Token {
                contract,
                recipient,
                amount,
            } => {
                if Some(contract) != policy.token_contract || self.fee_limit == 0 {
                    return Err(Error::UnsupportedTransfer);
                }
                (
                    31,
                    TRIGGER_URL,
                    proto::Trigger {
                        owner: self.owner.0.to_vec(),
                        contract: contract.0.to_vec(),
                        data: encode_transfer_call(recipient.evm(), amount)?.to_vec(),
                    }
                    .encode_to_vec(),
                )
            }
        };
        Ok(proto::Raw {
            ref_block_bytes: self.ref_block_bytes.to_vec(),
            ref_block_hash: self.ref_block_hash.to_vec(),
            expiration: self.expiration_ms,
            contracts: vec![proto::Contract {
                kind,
                parameter: Some(proto::Any {
                    type_url: type_url.into(),
                    value,
                }),
            }],
            timestamp: self.timestamp_ms,
            fee_limit: self.fee_limit,
        })
    }
    pub fn raw_bytes(&self, policy: Policy) -> Result<Vec<u8>> {
        Ok(self.raw(policy)?.encode_to_vec())
    }
    pub fn txid(&self, policy: Policy) -> Result<[u8; 32]> {
        Ok(Sha256::digest(self.raw_bytes(policy)?).into())
    }
    pub fn signed_bytes(&self, policy: Policy, signature: &[u8; 65]) -> Result<Vec<u8>> {
        let recovered = Address::from_evm(recover_address(&self.txid(policy)?, signature)?);
        if recovered != self.owner {
            return Err(Error::InvalidSignature);
        }
        Ok(proto::Signed {
            raw: Some(self.raw(policy)?),
            signatures: vec![signature.to_vec()],
        }
        .encode_to_vec())
    }
    pub fn decode_raw(bytes: &[u8], policy: Policy) -> Result<Self> {
        let raw: proto::Raw = strict_decode(bytes)?;
        if raw.contracts.len() != 1 {
            return Err(Error::UnsupportedTransfer);
        }
        let contract = &raw.contracts[0];
        let parameter = contract.parameter.as_ref().ok_or(Error::InvalidEncoding)?;
        let (owner, transfer) = match (contract.kind, parameter.type_url.as_str()) {
            (1, NATIVE_URL) => {
                let native: proto::Native = strict_decode(&parameter.value)?;
                if native.amount <= 0 {
                    return Err(Error::InvalidAmount);
                }
                (
                    Address::from_bytes(&native.owner)?,
                    Transfer::Native {
                        recipient: Address::from_bytes(&native.recipient)?,
                        amount: Amount::from_u64(native.amount as u64),
                    },
                )
            }
            (31, TRIGGER_URL) => {
                // Any call_value/token_id/permission/memo fields are absent from
                // our schema and rejected by re-encoding, including explicit zero.
                let trigger: proto::Trigger = strict_decode(&parameter.value)?;
                let (recipient, amount) = decode_transfer_call(&trigger.data)?;
                (
                    Address::from_bytes(&trigger.owner)?,
                    Transfer::Token {
                        contract: Address::from_bytes(&trigger.contract)?,
                        recipient: Address::from_evm(recipient),
                        amount,
                    },
                )
            }
            _ => return Err(Error::UnsupportedTransfer),
        };
        let tx = Self {
            owner,
            transfer,
            ref_block_bytes: raw
                .ref_block_bytes
                .try_into()
                .map_err(|_| Error::InvalidEncoding)?,
            ref_block_hash: raw
                .ref_block_hash
                .try_into()
                .map_err(|_| Error::InvalidEncoding)?,
            timestamp_ms: raw.timestamp,
            expiration_ms: raw.expiration,
            fee_limit: raw.fee_limit,
        };
        if tx.raw_bytes(policy)? != bytes {
            return Err(Error::InvalidEncoding);
        }
        Ok(tx)
    }
}

#[cfg(test)]
mod tests;
