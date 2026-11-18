//! Offline SLIP-0010 keys and typed SOL/USDT signing. Never exposes private keys.
use super::*;
use chain_solana::Address;
use ed25519_dalek::{Signer, SigningKey};
use hmac::{Hmac, Mac};
use sha2::Sha512;

pub const PATH: &str = "m/44'/501'/0'/0'";
pub(crate) fn key_from_entropy(entropy: &[u8], index: u32) -> Result<SigningKey> {
    if index >= MAX_ACCOUNTS {
        return Err(Error::InvalidFormat);
    }
    let mnemonic = Mnemonic::from_entropy(entropy).map_err(|_| Error::Crypto)?;
    let seed = Zeroizing::new(mnemonic.to_seed_normalized(""));
    fn hmac(key: &[u8], data: &[u8]) -> Zeroizing<[u8; 64]> {
        let mut mac =
            <Hmac<Sha512> as Mac>::new_from_slice(key).expect("HMAC accepts any key length");
        mac.update(data);
        Zeroizing::new(mac.finalize().into_bytes().into())
    }
    let mut material = hmac(b"ed25519 seed", &*seed);
    for child in [44, 501, index, 0] {
        let mut data = Zeroizing::new([0u8; 37]);
        data[1..33].copy_from_slice(&material[..32]);
        data[33..].copy_from_slice(&(child | 0x80000000).to_be_bytes());
        material = hmac(&material[32..], &*data);
    }
    Ok(SigningKey::from_bytes(
        material[..32].try_into().map_err(|_| Error::Crypto)?,
    ))
}

pub type Request = chain_solana::Quote;
fn payload_key(entropy: &[u8]) -> Zeroizing<[u8; 32]> {
    use sha2::Digest;
    let mut hash = sha2::Sha256::new();
    hash.update(b"Web3Rust Solana encrypted transfer v1\0");
    hash.update(entropy);
    Zeroizing::new(hash.finalize().into())
}
fn aad(id: &str, signature: &str, summary: &str) -> Vec<u8> {
    format!("W3SOL1\0{id}\0{signature}\0{summary}").into_bytes()
}
impl Session {
    pub fn solana_address(&mut self, directory: &Path) -> Result<String> {
        self.check_spending_profile(directory)?;
        let index = self.selected_account;
        Ok(Address(
            key_from_entropy(&self.material()?.entropy, index)?
                .verifying_key()
                .to_bytes(),
        )
        .base58())
    }
    pub fn sign_solana(
        &mut self,
        directory: &Path,
        journal: &Path,
        request: Request,
    ) -> Result<signing::SignedIntent> {
        self.check_spending_profile(directory)?;
        let index = self.selected_account;
        let material = self.material()?;
        let key = key_from_entropy(&material.entropy, index)?;
        if request.transfer.sender.0 != key.verifying_key().to_bytes() {
            return Err(Error::Authentication);
        }
        let summary = request.summary().map_err(|_| Error::InvalidFormat)?;
        let message = request
            .transfer
            .message()
            .map_err(|_| Error::InvalidFormat)?;
        let signature = key.sign(&message).to_bytes();
        let bytes = Zeroizing::new(
            request
                .transfer
                .signed(&signature)
                .map_err(|_| Error::Crypto)?,
        );
        let hash = bs58::encode(signature).into_string();
        let mut identifier = [0u8; 16];
        random(&mut identifier)?;
        let id = hex::encode(identifier);
        let encryption_key = payload_key(&material.entropy);
        let cipher =
            XChaCha20Poly1305::new_from_slice(&*encryption_key).map_err(|_| Error::Crypto)?;
        let mut nonce = [0u8; 24];
        random(&mut nonce)?;
        let ciphertext = cipher
            .encrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: &bytes,
                    aad: &aad(&id, &hash, &summary),
                },
            )
            .map_err(|_| Error::Crypto)?;
        let mut stored = nonce.to_vec();
        stored.extend(ciphertext);
        wallet_journal::Journal::open_existing(journal)
            .map_err(|_| Error::Storage)?
            .solana_signed(&wallet_journal::solana::Record {
                id: id.clone(),
                sender: request.transfer.sender.base58(),
                signature: hash.clone(),
                summary: summary.clone(),
                ciphertext: stored,
                state: "signed".into(),
            })
            .map_err(|_| Error::Busy)?;
        Ok(signing::SignedIntent { id, hash, summary })
    }
    pub fn release_solana(
        &mut self,
        directory: &Path,
        journal: &Path,
        id: &str,
    ) -> Result<Zeroizing<Vec<u8>>> {
        let sender = self.solana_address(directory)?;
        let journal =
            wallet_journal::Journal::open_existing(journal).map_err(|_| Error::Storage)?;
        let record = journal.solana_record(id).map_err(|_| Error::Storage)?;
        let request = Request::parse(&record.summary).map_err(|_| Error::InvalidFormat)?;
        if record.sender != sender
            || request.transfer.sender.base58() != sender
            || record.ciphertext.len() < 40
        {
            return Err(Error::Authentication);
        }
        let key = payload_key(&self.material()?.entropy);
        let cipher = XChaCha20Poly1305::new_from_slice(&*key).map_err(|_| Error::Crypto)?;
        let nonce = XNonce::try_from(&record.ciphertext[..24]).map_err(|_| Error::InvalidFormat)?;
        let bytes = Zeroizing::new(
            cipher
                .decrypt(
                    &nonce,
                    Payload {
                        msg: &record.ciphertext[24..],
                        aad: &aad(id, &record.signature, &record.summary),
                    },
                )
                .map_err(|_| Error::Authentication)?,
        );
        let signature =
            chain_solana::signature(&record.signature).map_err(|_| Error::Authentication)?;
        if request
            .transfer
            .signed(&signature)
            .map_err(|_| Error::Authentication)?
            != *bytes
        {
            return Err(Error::Authentication);
        }
        journal
            .solana_begin_broadcast(id)
            .map_err(|_| Error::Storage)?;
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_derivation_and_signatures() {
        for line in include_str!("../../../../tests/fixtures/solana-vectors.txt")
            .lines()
            .filter(|l| !l.starts_with('#'))
        {
            let f: Vec<_> = line.split('|').collect();
            let key = key_from_entropy(&hex::decode(f[0]).unwrap(), f[1].parse().unwrap()).unwrap();
            assert_eq!(Address(key.verifying_key().to_bytes()).base58(), f[2]);
            let signed = hex::decode(f[10]).unwrap();
            assert_eq!(
                key.sign(&hex::decode(f[9]).unwrap()).to_bytes(),
                signed[1..65]
            );
        }
        assert!(key_from_entropy(&[0; 16], MAX_ACCOUNTS).is_err());
    }
}
