//! Typed BSC-native signing; durable encrypted payload before release for broadcast.
use super::*;
use chain_evm::{Address, LegacyTransfer, Policy, Transfer};
use wallet_core::Amount;
use wallet_journal::{Intent, Journal};

pub struct BnbRequest<'a> {
    pub sender: &'a str,
    pub recipient: &'a str,
    pub amount: &'a str,
    pub nonce: u64,
    pub gas_price_wei: u64,
}
pub struct SignedIntent {
    pub id: String,
    pub hash: String,
    pub summary: String,
}
const POLICY: Policy = Policy {
    chain_id: 56,
    token_contract: None,
};

fn payload_key(entropy: &[u8]) -> Zeroizing<[u8; 32]> {
    // Fixed domain plus original BIP39 entropy. Distinct from the password-derived vault key.
    let mut source = Zeroizing::new(b"Web3Rust signed payload encryption v1\0".to_vec());
    source.extend_from_slice(entropy);
    Zeroizing::new(chain_evm::keccak256(&source))
}
fn aad(id: &str, hash: &str, summary: &str) -> Vec<u8> {
    format!("W3TX1\0{id}\0{hash}\0{summary}").into_bytes()
}
fn active(directory: &Path, password: &[u8]) -> Result<(File, Material)> {
    let lock = storage::acquire(directory)?;
    let material = unseal(
        &read_file(&directory.join(ACTIVE))?,
        password,
        Phase::Active,
    )?;
    Ok((lock, material))
}
impl Session {
    /// Caller must already show and confirm all exact fields. No network or broadcast here.
    pub fn sign_bnb(
        directory: &Path,
        password: &[u8],
        journal: &Path,
        request: BnbRequest<'_>,
    ) -> Result<SignedIntent> {
        Self::sign_bsc(directory, password, journal, request, false, 21000)
    }

    pub fn sign_usdt(
        directory: &Path,
        password: &[u8],
        journal: &Path,
        request: BnbRequest<'_>,
        gas_limit: u64,
    ) -> Result<SignedIntent> {
        if !chain_evm::bsc::valid_token_gas(gas_limit) {
            return Err(Error::InvalidFormat);
        }
        Self::sign_bsc(directory, password, journal, request, true, gas_limit)
    }

    fn sign_bsc(
        directory: &Path,
        password: &[u8],
        journal: &Path,
        request: BnbRequest<'_>,
        usdt: bool,
        gas_limit: u64,
    ) -> Result<SignedIntent> {
        if !directory.is_absolute() {
            return Err(Error::InvalidFormat);
        }
        let (_lock, material) = active(directory, password)?;
        Self::sign_material(&material, journal, request, usdt, gas_limit, 0)
    }

    pub fn sign_unlocked(
        &mut self,
        directory: &Path,
        journal: &Path,
        request: BnbRequest<'_>,
        usdt: bool,
        gas_limit: u64,
    ) -> Result<SignedIntent> {
        self.check_spending_profile(directory)?;
        if (usdt && !chain_evm::bsc::valid_token_gas(gas_limit)) || (!usdt && gas_limit != 21000) {
            return Err(Error::InvalidFormat);
        }
        let index = self.selected_account;
        Self::sign_material(self.material()?, journal, request, usdt, gas_limit, index)
    }

    fn sign_material(
        material: &Material,
        journal: &Path,
        request: BnbRequest<'_>,
        usdt: bool,
        gas_limit: u64,
        index: u32,
    ) -> Result<SignedIntent> {
        if !journal.is_absolute()
            || request.gas_price_wei == 0
            || request.gas_price_wei > 1_000_000_000_000
        {
            return Err(Error::InvalidFormat);
        }
        let recipient = Address::parse(request.recipient).map_err(|_| Error::InvalidFormat)?;
        if recipient.bytes() == [0; 20] || (usdt && recipient == chain_evm::bsc::USDT) {
            return Err(Error::InvalidFormat);
        }
        let amount = Amount::parse_units(request.amount, 18).map_err(|_| Error::InvalidFormat)?;
        if amount == Amount::ZERO {
            return Err(Error::InvalidFormat);
        }
        let accounts = accounts::from_entropy_at(&material.entropy, index)?;
        let sender = Address::parse(request.sender).map_err(|_| Error::InvalidFormat)?;
        if sender.checksum() != accounts.bsc {
            return Err(Error::Authentication);
        }
        let tx = LegacyTransfer {
            chain_id: 56,
            nonce: request.nonce,
            gas_price: Amount::from_u64(request.gas_price_wei),
            gas_limit,
            transfer: if usdt {
                Transfer::Token {
                    contract: chain_evm::bsc::USDT,
                    recipient,
                    amount,
                }
            } else {
                Transfer::Native { recipient, amount }
            },
        };
        let policy = if usdt { chain_evm::bsc::POLICY } else { POLICY };
        let hash_to_sign = tx.signing_hash(policy).map_err(|_| Error::InvalidFormat)?;
        let mut summary = format!(
            "{}\n56\n{}\n{}\n{}\n{}\n{}\n{}",
            if usdt { "USDT" } else { "BNB" },
            sender.checksum(),
            recipient.checksum(),
            amount.format_units(18).map_err(|_| Error::InvalidFormat)?,
            request.gas_price_wei,
            gas_limit,
            request.nonce
        );
        if usdt {
            summary.push('\n');
            summary.push_str(&chain_evm::bsc::USDT.checksum());
        }
        let journal = Journal::open(journal).map_err(|_| Error::Storage)?;
        let mut identifier = [0; 16];
        random(&mut identifier)?;
        let id = hex::encode(identifier);
        journal
            .reserve(Intent {
                id: &id,
                network: 56,
                sender: &sender.checksum(),
                nonce: &request.nonce.to_string(),
                summary: &summary,
            })
            .map_err(|_| Error::Busy)?;
        let mnemonic = Mnemonic::from_entropy(&material.entropy).map_err(|_| Error::Crypto)?;
        let seed = Zeroizing::new(mnemonic.to_seed_normalized(""));
        let key = accounts::derive(&seed, &accounts.bsc_path)?;
        let (signature, recovery) = key.private_key().sign_prehash_recoverable(&hash_to_sign);
        let mut bytes = Zeroizing::new([0; 65]);
        bytes[..64].copy_from_slice(&signature.to_bytes());
        bytes[64] = recovery.to_byte();
        let signed = Zeroizing::new(
            tx.signed_bytes(policy, sender, &bytes)
                .map_err(|_| Error::Crypto)?,
        );
        let hash = hex::encode(chain_evm::keccak256(&signed));
        let encryption_key = payload_key(&material.entropy);
        let cipher =
            XChaCha20Poly1305::new_from_slice(&*encryption_key).map_err(|_| Error::Crypto)?;
        let mut nonce = [0; 24];
        random(&mut nonce)?;
        let encrypted = cipher
            .encrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: &signed,
                    aad: &aad(&id, &hash, &summary),
                },
            )
            .map_err(|_| Error::Crypto)?;
        let mut stored = nonce.to_vec();
        stored.extend(encrypted);
        journal
            .signed(&id, &hash, &stored)
            .map_err(|_| Error::Storage)?;
        Ok(SignedIntent { id, hash, summary })
    }

    /// Marks broadcasting durably before returning the same original signed bytes.
    /// Releasing again can only return the identical hash/payload, never a new nonce.
    pub fn release_bnb(
        directory: &Path,
        password: &[u8],
        journal: &Path,
        id: &str,
    ) -> Result<Zeroizing<Vec<u8>>> {
        let (_lock, material) = active(directory, password)?;
        Self::release_material(&material, journal, id, 0)
    }
    pub fn release_unlocked(
        &mut self,
        directory: &Path,
        journal: &Path,
        id: &str,
    ) -> Result<Zeroizing<Vec<u8>>> {
        self.check_spending_profile(directory)?;
        let index = self.selected_account;
        Self::release_material(self.material()?, journal, id, index)
    }
    fn release_material(
        material: &Material,
        journal: &Path,
        id: &str,
        index: u32,
    ) -> Result<Zeroizing<Vec<u8>>> {
        if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::InvalidFormat);
        }
        let journal = Journal::open(journal).map_err(|_| Error::Storage)?;
        let record = journal.record(id).map_err(|_| Error::Storage)?;
        let accounts = accounts::from_entropy_at(&material.entropy, index)?;
        if !(record
            .summary
            .starts_with(&format!("BNB\n56\n{}\n", accounts.bsc))
            || record
                .summary
                .starts_with(&format!("USDT\n56\n{}\n", accounts.bsc)))
            || record.ciphertext.len() < 40
        {
            return Err(Error::Authentication);
        }
        let encryption_key = payload_key(&material.entropy);
        let cipher =
            XChaCha20Poly1305::new_from_slice(&*encryption_key).map_err(|_| Error::Crypto)?;
        let nonce = XNonce::try_from(&record.ciphertext[..24]).map_err(|_| Error::InvalidFormat)?;
        let signed = Zeroizing::new(
            cipher
                .decrypt(
                    &nonce,
                    Payload {
                        msg: &record.ciphertext[24..],
                        aad: &aad(id, &record.hash, &record.summary),
                    },
                )
                .map_err(|_| Error::Authentication)?,
        );
        if hex::encode(chain_evm::keccak256(&signed)) != record.hash {
            return Err(Error::Authentication);
        }
        journal.begin_broadcast(id).map_err(|_| Error::Storage)?;
        Ok(signed)
    }
}
