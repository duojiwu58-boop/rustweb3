//! Offline TRON signing; encrypted durable payload is released only for the same account.
use super::*;
use chain_tron::{
    Address,
    mainnet::{Quote, policy},
};
fn payload_key(entropy: &[u8]) -> Zeroizing<[u8; 32]> {
    use sha2::Digest;
    let mut hash = sha2::Sha256::new();
    hash.update(b"Web3Rust TRON encrypted transfer v1\0");
    hash.update(entropy);
    Zeroizing::new(hash.finalize().into())
}
fn aad(id: &str, hash: &str, summary: &str) -> Vec<u8> {
    format!("W3TRON1\0{id}\0{hash}\0{summary}").into_bytes()
}
fn now() -> Result<i64> {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| Error::InvalidFormat)?
            .as_millis(),
    )
    .map_err(|_| Error::InvalidFormat)
}
impl Session {
    pub fn tron_address(&mut self, directory: &Path) -> Result<String> {
        self.check_spending_profile(directory)?;
        let index = self.selected_account;
        Ok(accounts::from_entropy_at(&self.material()?.entropy, index)?.tron)
    }
    pub fn sign_tron(
        &mut self,
        directory: &Path,
        journal: &Path,
        request: Quote,
    ) -> Result<signing::SignedIntent> {
        self.check_spending_profile(directory)?;
        let index = self.selected_account;
        let material = self.material()?;
        let summary = request.summary().map_err(|_| Error::InvalidFormat)?;
        request
            .check_fresh(now()?)
            .map_err(|_| Error::InvalidFormat)?;
        let mnemonic = Mnemonic::from_entropy(&material.entropy).map_err(|_| Error::Crypto)?;
        let seed = Zeroizing::new(mnemonic.to_seed_normalized(""));
        let key = accounts::derive(&seed, &format!("m/44'/195'/0'/0/{index}"))?;
        let owner = Address::from_public_key(
            key.private_key()
                .verifying_key()
                .to_sec1_point(false)
                .as_bytes(),
        )
        .map_err(|_| Error::Crypto)?;
        if owner != request.tx.owner {
            return Err(Error::Authentication);
        }
        let digest = request
            .tx
            .txid(policy())
            .map_err(|_| Error::InvalidFormat)?;
        let (sig, recovery) = key.private_key().sign_prehash_recoverable(&digest);
        let mut signature = [0u8; 65];
        signature[..64].copy_from_slice(&sig.to_bytes());
        signature[64] = recovery.to_byte();
        let bytes = Zeroizing::new(
            request
                .tx
                .signed_bytes(policy(), &signature)
                .map_err(|_| Error::Crypto)?,
        );
        let hash = hex::encode(digest);
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
            .tron_signed(&wallet_journal::tron::Record {
                id: id.clone(),
                sender: owner.base58(),
                signature: hash.clone(),
                summary: summary.clone(),
                ciphertext: stored,
                state: "signed".into(),
            })
            .map_err(|_| Error::Busy)?;
        Ok(signing::SignedIntent { id, hash, summary })
    }
    pub fn release_tron(
        &mut self,
        directory: &Path,
        journal: &Path,
        id: &str,
    ) -> Result<Zeroizing<Vec<u8>>> {
        let owner = self.tron_address(directory)?;
        let journal =
            wallet_journal::Journal::open_existing(journal).map_err(|_| Error::Storage)?;
        let record = journal.tron_record(id).map_err(|_| Error::Storage)?;
        let quote = Quote::parse(&record.summary).map_err(|_| Error::InvalidFormat)?;
        if record.sender != owner
            || quote.tx.owner.base58() != owner
            || record.ciphertext.len() < 40
        {
            return Err(Error::Authentication);
        }
        quote
            .check_fresh(now()?)
            .map_err(|_| Error::InvalidFormat)?;
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
        quote
            .verify_signed(&bytes, &record.signature)
            .map_err(|_| Error::Authentication)?;
        journal
            .tron_begin_broadcast(id)
            .map_err(|_| Error::Storage)?;
        Ok(bytes)
    }
}
