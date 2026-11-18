//! Bounded, typed Solana mainnet transfers. No I/O, key derivation or blind signing.
use curve25519_dalek::edwards::CompressedEdwardsY;
use sha2::{Digest, Sha256};
use wallet_core::{Amount, Error, Result};

pub const GENESIS: &str = "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d";
pub const USDT_MINT: &str = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB";
pub const TOKEN_PROGRAM: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
pub const ASSOCIATED_PROGRAM: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";
pub const SYSTEM_PROGRAM: &str = "11111111111111111111111111111111";
pub const MAX_TRANSACTION: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Address(pub [u8; 32]);
impl Address {
    pub fn parse(text: &str) -> Result<Self> {
        if !(32..=44).contains(&text.len()) {
            return Err(Error::InvalidAddress);
        }
        let mut bytes = [0; 32];
        if bs58::decode(text)
            .onto(&mut bytes)
            .map_err(|_| Error::InvalidAddress)?
            != 32
        {
            return Err(Error::InvalidAddress);
        }
        let address = Self(bytes);
        if address.base58() != text {
            return Err(Error::InvalidAddress);
        }
        Ok(address)
    }
    pub fn base58(self) -> String {
        bs58::encode(self.0).into_string()
    }
    pub fn wallet(text: &str) -> Result<Self> {
        let value = Self::parse(text)?;
        if value.0 == [0; 32] || CompressedEdwardsY(value.0).decompress().is_none() {
            return Err(Error::InvalidAddress);
        }
        Ok(value)
    }
    pub fn usdt_account(self) -> Result<Self> {
        let token = Self::parse(TOKEN_PROGRAM)?;
        let mint = Self::parse(USDT_MINT)?;
        let program = Self::parse(ASSOCIATED_PROGRAM)?;
        for bump in (0..=255u8).rev() {
            let mut hash = Sha256::new();
            for bytes in [
                &self.0[..],
                &token.0,
                &mint.0,
                &[bump],
                &program.0,
                b"ProgramDerivedAddress",
            ] {
                hash.update(bytes);
            }
            let bytes: [u8; 32] = hash.finalize().into();
            if CompressedEdwardsY(bytes).decompress().is_none() {
                return Ok(Self(bytes));
            }
        }
        Err(Error::InvalidAddress)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asset {
    Sol,
    Usdt,
}
impl Asset {
    pub fn parse(text: &str) -> Result<Self> {
        match text {
            "SOL" => Ok(Self::Sol),
            "USDT" => Ok(Self::Usdt),
            _ => Err(Error::UnsupportedTransfer),
        }
    }
    pub fn symbol(self) -> &'static str {
        if self == Self::Sol { "SOL" } else { "USDT" }
    }
    pub fn decimals(self) -> u8 {
        if self == Self::Sol { 9 } else { 6 }
    }
    pub fn amount(self, text: &str) -> Result<u64> {
        let value = Amount::parse_units(text, self.decimals())?.to_u64()?;
        if value == 0 {
            return Err(Error::InvalidAmount);
        }
        Ok(value)
    }
}

#[derive(Clone, Debug)]
pub struct Transfer {
    pub sender: Address,
    pub recipient: Address,
    pub asset: Asset,
    pub amount: u64,
    pub blockhash: Address,
    pub create_recipient_account: bool,
}
struct Instruction {
    program: Address,
    accounts: Vec<(Address, bool)>,
    data: Vec<u8>,
}
fn short(output: &mut Vec<u8>, mut value: usize) {
    loop {
        let byte = (value & 127) as u8;
        value >>= 7;
        output.push(byte | if value > 0 { 128 } else { 0 });
        if value == 0 {
            break;
        }
    }
}
impl Transfer {
    pub fn message(&self) -> Result<Vec<u8>> {
        Address::wallet(&self.sender.base58())?;
        Address::wallet(&self.recipient.base58())?;
        if self.amount == 0 || self.sender == self.recipient || self.blockhash.0 == [0; 32] {
            return Err(Error::UnsupportedTransfer);
        }
        let system = Address::parse(SYSTEM_PROGRAM)?;
        let mut instructions = Vec::new();
        if self.asset == Asset::Sol {
            if self.create_recipient_account {
                return Err(Error::UnsupportedTransfer);
            }
            let mut data = 2u32.to_le_bytes().to_vec();
            data.extend(self.amount.to_le_bytes());
            instructions.push(Instruction {
                program: system,
                accounts: vec![(self.sender, true), (self.recipient, true)],
                data,
            });
        } else {
            let token = Address::parse(TOKEN_PROGRAM)?;
            let mint = Address::parse(USDT_MINT)?;
            let from = self.sender.usdt_account()?;
            let to = self.recipient.usdt_account()?;
            if self.create_recipient_account {
                instructions.push(Instruction {
                    program: Address::parse(ASSOCIATED_PROGRAM)?,
                    accounts: vec![
                        (self.sender, true),
                        (to, true),
                        (self.recipient, false),
                        (mint, false),
                        (system, false),
                        (token, false),
                    ],
                    data: vec![1], // CreateIdempotent; never recreate an arbitrary token account.
                });
            }
            let mut data = vec![12]; // SPL Token TransferChecked, fixed mint decimals.
            data.extend(self.amount.to_le_bytes());
            data.push(6);
            instructions.push(Instruction {
                program: token,
                accounts: vec![
                    (from, true),
                    (mint, false),
                    (to, true),
                    (self.sender, false),
                ],
                data,
            });
        }
        let mut keys = vec![(self.sender, true)];
        for instruction in &instructions {
            for (address, writable) in std::iter::once((instruction.program, false))
                .chain(instruction.accounts.iter().copied())
            {
                if let Some(entry) = keys.iter_mut().find(|entry| entry.0 == address) {
                    entry.1 |= writable;
                } else {
                    keys.push((address, writable));
                }
            }
        }
        // One writable signer first, then unsigned writable, then readonly.
        keys[1..].sort_by_key(|entry| !entry.1);
        let readonly = keys.iter().filter(|entry| !entry.1).count() as u8;
        let mut message = vec![1, 0, readonly];
        short(&mut message, keys.len());
        for (address, _) in &keys {
            message.extend(address.0);
        }
        message.extend(self.blockhash.0);
        short(&mut message, instructions.len());
        for instruction in instructions {
            let index = |address| keys.iter().position(|entry| entry.0 == address).unwrap() as u8;
            message.push(index(instruction.program));
            short(&mut message, instruction.accounts.len());
            for (address, _) in instruction.accounts {
                message.push(index(address));
            }
            short(&mut message, instruction.data.len());
            message.extend(instruction.data);
        }
        if message.len() + 65 > MAX_TRANSACTION {
            return Err(Error::InvalidEncoding);
        }
        Ok(message)
    }
    pub fn signed(&self, signature: &[u8; 64]) -> Result<Vec<u8>> {
        let message = self.message()?;
        ed25519_dalek::VerifyingKey::from_bytes(&self.sender.0)
            .map_err(|_| Error::InvalidSignature)?
            .verify_strict(&message, &ed25519_dalek::Signature::from_bytes(signature))
            .map_err(|_| Error::InvalidSignature)?;
        let mut bytes = vec![1];
        bytes.extend(signature);
        bytes.extend(message);
        Ok(bytes)
    }
    pub fn unsigned(&self) -> Result<Vec<u8>> {
        let mut bytes = vec![1];
        bytes.extend([0; 64]);
        bytes.extend(self.message()?);
        Ok(bytes)
    }
}

pub fn signature(text: &str) -> Result<[u8; 64]> {
    if !(64..=88).contains(&text.len()) {
        return Err(Error::InvalidSignature);
    }
    let mut bytes = [0; 64];
    if bs58::decode(text)
        .onto(&mut bytes)
        .map_err(|_| Error::InvalidSignature)?
        != 64
        || bs58::encode(bytes).into_string() != text
    {
        return Err(Error::InvalidSignature);
    }
    Ok(bytes)
}

pub struct Quote {
    pub transfer: Transfer,
    pub last_valid_height: u64,
    pub fee: u64,
    pub rent: u64,
}
impl Quote {
    pub fn summary(&self) -> Result<String> {
        self.transfer
            .message()
            .map_err(|_| Error::InvalidEncoding)?;
        if self.last_valid_height == 0
            || self.last_valid_height > i64::MAX as u64
            || self.fee == 0
            || self.fee > 1_000_000
            || self.rent > 5_000_000
            || (self.transfer.create_recipient_account != (self.rent > 0))
        {
            return Err(Error::InvalidEncoding);
        }
        Ok(format!(
            "{}\nsolana-mainnet\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
            self.transfer.asset.symbol(),
            self.transfer.sender.base58(),
            self.transfer.recipient.base58(),
            wallet_core::Amount::from_u64(self.transfer.amount)
                .format_units(self.transfer.asset.decimals())
                .map_err(|_| Error::InvalidEncoding)?,
            self.transfer.blockhash.base58(),
            self.last_valid_height,
            self.fee,
            self.rent,
            u8::from(self.transfer.create_recipient_account)
        ))
    }
    pub fn parse(summary: &str) -> Result<Self> {
        let parts: Vec<_> = summary.split('\n').collect();
        if parts.len() != 10 || parts[1] != "solana-mainnet" {
            return Err(Error::InvalidEncoding);
        }
        let asset = Asset::parse(parts[0]).map_err(|_| Error::InvalidEncoding)?;
        let result = Self {
            transfer: Transfer {
                sender: Address::wallet(parts[2]).map_err(|_| Error::InvalidEncoding)?,
                recipient: Address::wallet(parts[3]).map_err(|_| Error::InvalidEncoding)?,
                asset,
                amount: asset.amount(parts[4]).map_err(|_| Error::InvalidEncoding)?,
                blockhash: Address::parse(parts[5]).map_err(|_| Error::InvalidEncoding)?,
                create_recipient_account: match parts[9] {
                    "0" => false,
                    "1" => true,
                    _ => return Err(Error::InvalidEncoding),
                },
            },
            last_valid_height: parts[6].parse().map_err(|_| Error::InvalidEncoding)?,
            fee: parts[7].parse().map_err(|_| Error::InvalidEncoding)?,
            rent: parts[8].parse().map_err(|_| Error::InvalidEncoding)?,
        };
        if result.summary()? != summary {
            return Err(Error::InvalidEncoding);
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn official_sdk_public_vectors() {
        for line in include_str!("../../../../tests/fixtures/solana-vectors.txt")
            .lines()
            .filter(|l| !l.starts_with('#'))
        {
            let f: Vec<_> = line.split('|').collect();
            let sender = Address::parse(f[2]).unwrap();
            assert_eq!(sender.usdt_account().unwrap().base58(), f[3]);
            let tx = Transfer {
                sender,
                recipient: Address::parse(f[5]).unwrap(),
                asset: Asset::parse(f[4]).unwrap(),
                amount: f[7].parse().unwrap(),
                blockhash: Address::parse(f[6]).unwrap(),
                create_recipient_account: f[8] == "1",
            };
            assert_eq!(
                hex::encode(tx.message().unwrap()),
                f[9],
                "{} {} {}",
                f[1],
                f[4],
                f[8]
            );
            let signed = hex::decode(f[10]).unwrap();
            assert_eq!(
                tx.signed(signed[1..65].try_into().unwrap()).unwrap(),
                signed
            );
            let mut wrong = tx.clone();
            wrong.amount += 1;
            assert!(wrong.signed(signed[1..65].try_into().unwrap()).is_err());
        }
    }
    #[test]
    fn exact_amounts_and_wrong_chain_addresses() {
        assert_eq!(Asset::Sol.amount("0.000000001"), Ok(1));
        assert_eq!(Asset::Usdt.amount("1.000001"), Ok(1_000_001));
        for value in ["0", "-1", "1e6", "0.0000001", "18446744073709.551616"] {
            assert!(Asset::Usdt.amount(value).is_err());
        }
        assert!(Address::parse("0x0000000000000000000000000000000000000001").is_err());
        assert!(Address::wallet(SYSTEM_PROGRAM).is_err());
        assert!(Address::wallet(&Address([7; 32]).usdt_account().unwrap().base58()).is_err());
    }
}
