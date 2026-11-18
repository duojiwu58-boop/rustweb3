//! Offline vault used by the signer's contained, dedicated pipe session.
//! GUI integration and live transaction verification remain prerequisites for real funds.
//! Only public test entropy is used by this crate's tests.

use argon2::{Algorithm, Argon2, Params, Version};
use bip39::{Language, Mnemonic};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use zeroize::Zeroizing;
mod accounts;
mod account_state;
mod signing;
pub mod solana;
pub mod tron;
mod storage;
pub use accounts::{AccountAddresses, BSC_PATH, TRON_PATH};
pub use signing::{BnbRequest, SignedIntent};

// Fixed, bounded suite: changing this requires a new format version.
const PREFIX: &[u8; 8] = b"W3VAULT1";
const HEADER_LEN: usize = 8 + 1 + 16 + 24;
const FILE_LEN: usize = HEADER_LEN + 32 + 16;
const PENDING: &str = "pending.vault";
const ACTIVE: &str = "active.vault";
const SESSION_TTL: Duration = Duration::from_secs(2 * 60 * 60);
pub const MAX_ACCOUNTS: u32 = 32;

// V1 is byte-for-byte unchanged. V2 authenticates variable-length BIP39 entropy.
fn valid_record(bytes: &[u8]) -> bool {
    if bytes.len() < HEADER_LEN + 16 + 16 { return false; }
    let entropy_len = bytes.len() - HEADER_LEN - 16;
    (&bytes[..8] == PREFIX && entropy_len == 32)
        || (&bytes[..8] == b"W3VAULT2" && matches!(entropy_len, 16 | 20 | 24 | 28))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    PasswordLength,
    InvalidBackup,
    InvalidFormat,
    Authentication,
    RandomSource,
    Storage,
    AlreadyExists,
    Busy,
    Locked,
    NotPending,
    NotActive,
    Crypto,
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    PendingBackup = 1,
    Active = 2,
}

// Deliberately no Debug, Display, Clone or serialization implementations.
struct Material {
    entropy: Zeroizing<Vec<u8>>,
    key: Zeroizing<[u8; 32]>,
    salt: [u8; 16],
}

/// Signer-side prototype only. Not a transport authentication/session token.
/// Call lock on GUI lock, disconnect and cancellation; expiry is checked per use.
pub struct Session {
    _profile_lock: File,
    directory: PathBuf,
    phase: Phase,
    material: Option<Material>,
    expires: Instant,
    account_count: u32,
    selected_account: u32,
}

fn password_valid(password: &[u8]) -> Result<()> {
    if !(12..=1024).contains(&password.len()) {
        return Err(Error::PasswordLength);
    }
    Ok(())
}

fn random(bytes: &mut [u8]) -> Result<()> {
    getrandom::fill(bytes).map_err(|_| Error::RandomSource)
}

fn derive(password: &[u8], salt: &[u8; 16]) -> Result<Zeroizing<[u8; 32]>> {
    password_valid(password)?;
    let params = Params::new(65_536, 3, 1, Some(32)).map_err(|_| Error::Crypto)?;
    // argon2 0.6.0's default allocated Blocks drops without wiping its arena.
    // Own the backing arena so early returns and normal completion both wipe it.
    let count = params.block_count();
    let mut memory = Zeroizing::new(Vec::<argon2::Block>::new());
    memory.try_reserve_exact(count).map_err(|_| Error::Crypto)?;
    memory.resize(count, argon2::Block::default());
    let mut key = Zeroizing::new([0; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into_with_memory(password, salt, &mut *key, memory.as_mut_slice())
        .map_err(|_| Error::Crypto)?;
    Ok(key)
}

fn seal(material: &Material, phase: Phase) -> Result<Vec<u8>> {
    let mut header = [0; HEADER_LEN];
    header[..8].copy_from_slice(if material.entropy.len() == 32 { PREFIX } else { b"W3VAULT2" });
    header[8] = phase as u8;
    header[9..25].copy_from_slice(&material.salt);
    random(&mut header[25..])?;
    let cipher = XChaCha20Poly1305::new_from_slice(&*material.key).map_err(|_| Error::Crypto)?;
    let nonce = XNonce::try_from(&header[25..]).map_err(|_| Error::Crypto)?;
    let encrypted = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: &material.entropy,
                aad: &header,
            },
        )
        .map_err(|_| Error::Crypto)?;
    let mut bytes = header.to_vec();
    bytes.extend(encrypted);
    Ok(bytes)
}

fn unseal(bytes: &[u8], password: &[u8], expected: Phase) -> Result<Material> {
    if !valid_record(bytes) || bytes[8] != expected as u8 {
        return Err(Error::InvalidFormat);
    }
    let salt = bytes[9..25].try_into().map_err(|_| Error::InvalidFormat)?;
    let key = derive(password, &salt)?;
    let cipher = XChaCha20Poly1305::new_from_slice(&*key).map_err(|_| Error::Crypto)?;
    let nonce = XNonce::try_from(&bytes[25..HEADER_LEN]).map_err(|_| Error::InvalidFormat)?;
    let plaintext = Zeroizing::new(
        cipher
            .decrypt(
                &nonce,
                Payload {
                    msg: &bytes[HEADER_LEN..],
                    aad: &bytes[..HEADER_LEN],
                },
            )
            .map_err(|_| Error::Authentication)?,
    );
    if !matches!(plaintext.len(), 16 | 20 | 24 | 28 | 32) {
        return Err(Error::InvalidFormat);
    }
    let entropy = plaintext;
    Ok(Material { entropy, key, salt })
}

fn read_file(path: &Path) -> Result<Vec<u8>> {
    let file = File::open(path).map_err(|_| Error::Storage)?;
    let mut bytes = Vec::with_capacity(FILE_LEN + 1);
    file.take((FILE_LEN + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Storage)?;
    if !valid_record(&bytes) {
        return Err(Error::InvalidFormat);
    }
    Ok(bytes)
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                Error::AlreadyExists
            } else {
                Error::Storage
            }
        })?;
    // On failure leave the encrypted partial file for explicit recovery. Never
    // overwrite or silently fall back to another wallet after uncertain I/O.
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| Error::Storage)
}

impl Session {
    /// Authenticate the address used exclusively for local cache cleanup.
    pub fn deletion_address(directory: &Path, password: &[u8]) -> Result<AccountAddresses> {
        let mut session = Self::open_with_spending(directory, password, true)?;
        accounts::from_entropy(&session.material()?.entropy)
    }
    pub fn deletion_addresses(directory: &Path, password: &[u8]) -> Result<Vec<String>> {
        let mut session = Self::open_with_spending(directory, password, true)?;
        let count = session.account_count;
        (0..count).map(|index| accounts::from_entropy_at(&session.material()?.entropy, index).map(|a| a.bsc)).collect()
    }

    /// Remove known vault files and this account's journal after fresh authentication.
    pub fn delete_wallet(directory: &Path, password: &[u8], journal: &Path) -> Result<()> {
        if !directory.is_absolute() { return Err(Error::Storage); }
        fn plain(path: &Path, directory: bool) -> Result<()> {
            let metadata = fs::symlink_metadata(path).map_err(|_| Error::Storage)?;
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 { return Err(Error::Storage); }
            }
            if metadata.file_type().is_symlink() || metadata.is_dir() != directory {
                return Err(Error::Storage);
            }
            Ok(())
        }
        plain(directory, true)?;
        // Reject unexpected contents before authentication or any removal.
        for entry in fs::read_dir(directory).map_err(|_| Error::Storage)? {
            let entry = entry.map_err(|_| Error::Storage)?;
            if ![PENDING, ACTIVE, storage::STAGED, storage::LOCK, account_state::STATE, account_state::NEXT]
                .iter().any(|name| entry.file_name() == *name) {
                return Err(Error::Storage);
            }
            plain(&entry.path(), false)?;
        }
        let mut session = Self::open_with_spending(directory, password, true)?;
        if journal.try_exists().map_err(|_| Error::Storage)? {
            let journal = wallet_journal::Journal::open_existing(journal).map_err(|_| Error::Storage)?;
            for index in 0..session.account_count {
                let addresses = accounts::from_entropy_at(&session.material()?.entropy, index)?;
                journal.forget_wallet(&addresses.bsc, &addresses.tron).map_err(|_| Error::Storage)?;
                journal.tron_forget(&addresses.tron).map_err(|_| Error::Storage)?;
                let address = solana::key_from_entropy(&session.material()?.entropy, index)?.verifying_key().to_bytes();
                journal.solana_forget(&chain_solana::Address(address).base58()).map_err(|_| Error::Storage)?;
            }
        }
        // Keep the authoritative record until stale/staged copies have been removed.
        let authoritative = if session.phase == Phase::Active { ACTIVE } else { PENDING };
        for name in [storage::STAGED, account_state::NEXT, PENDING, ACTIVE].into_iter()
            .filter(|name| *name != authoritative).chain([authoritative]) {
            match fs::remove_file(directory.join(name)) {
                Ok(()) => (),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(_) => return Err(Error::Storage),
            }
        }
        // Keep account metadata until the vault is removed so a failed deletion
        // before that point can retry cleanup for every account.
        if directory.join(account_state::STATE).try_exists().map_err(|_| Error::Storage)? {
            fs::remove_file(directory.join(account_state::STATE)).map_err(|_| Error::Storage)?;
        }
        drop(session);
        fs::remove_file(directory.join(storage::LOCK)).map_err(|_| Error::Storage)?;
        fs::remove_dir(directory).map_err(|_| Error::Storage)
    }

    /// Re-authenticate an active wallet for offline backup without retaining a session.
    pub fn export_backup(directory: &Path, password: &[u8]) -> Result<Zeroizing<String>> {
        let mut session = Self::unlock(directory, password)?;
        let mnemonic = Mnemonic::from_entropy(&session.material()?.entropy)
            .map_err(|_| Error::Crypto)?;
        Ok(Zeroizing::new(mnemonic.to_string()))
    }

    /// Password-authenticated public addresses from a committed active record.
    /// Does not return private keys, create a session, or authorize spending.
    pub fn account_addresses(directory: &Path, password: &[u8]) -> Result<AccountAddresses> {
        if !directory.is_absolute() {
            return Err(Error::Storage);
        }
        let _profile_lock = storage::acquire(directory)?;
        if !directory
            .join(ACTIVE)
            .try_exists()
            .map_err(|_| Error::Storage)?
        {
            return Err(Error::NotActive);
        }
        let material = unseal(
            &read_file(&directory.join(ACTIVE))?,
            password,
            Phase::Active,
        )?;
        accounts::from_entropy(&material.entropy)
    }
    /// Generates 128 bits (12 BIP39 words) and persists before allowing backup display.
    /// The caller must provide a new absolute profile directory under trusted ACLs.
    pub fn create_new(directory: &Path, password: &[u8]) -> Result<Self> {
        password_valid(password)?;
        let mut entropy = Zeroizing::new(vec![0; 16]);
        random(&mut entropy)?;
        Self::persist_new(directory, password, entropy)
    }

    fn persist_new(
        directory: &Path,
        password: &[u8],
        entropy: Zeroizing<Vec<u8>>,
    ) -> Result<Self> {
        if !directory.is_absolute() {
            return Err(Error::Storage);
        }
        let mut salt = [0; 16];
        random(&mut salt)?;
        let material = Material {
            entropy,
            key: derive(password, &salt)?,
            salt,
        };
        let bytes = seal(&material, Phase::PendingBackup)?;
        fs::create_dir(directory).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                Error::AlreadyExists
            } else {
                Error::Storage
            }
        })?;
        let profile_lock = storage::acquire(directory)?;
        write_new(&directory.join(PENDING), &bytes)?;
        Ok(Self {
            _profile_lock: profile_lock,
            directory: directory.to_owned(),
            phase: Phase::PendingBackup,
            material: Some(material),
            expires: Instant::now() + SESSION_TTL,
            account_count: 1,
            selected_account: 0,
        })
    }

    /// Restore an English BIP39 mnemonic of any standard length, empty passphrase, into a
    /// NEW profile. Never replaces an existing profile, including an empty one.
    pub fn restore_new(directory: &Path, password: &[u8], words: &str) -> Result<Self> {
        let entropy = parse_backup(words)?;
        Self::persist_new(directory, password, entropy)
    }

    pub fn open(directory: &Path, password: &[u8]) -> Result<Self> {
        Self::open_with_spending(directory, password, false)
    }

    /// Password is consumed only at unlock; active material stays in the offline
    /// signer until lock, expiry, or process exit. No password is retained.
    pub fn unlock(directory: &Path, password: &[u8]) -> Result<Self> {
        let value = Self::open_with_spending(directory, password, true)?;
        if value.phase != Phase::Active {
            return Err(Error::NotActive);
        }
        Ok(value)
    }

    fn open_with_spending(directory: &Path, password: &[u8], spending: bool) -> Result<Self> {
        if !directory.is_absolute() {
            return Err(Error::Storage);
        }
        let profile_lock = storage::acquire(directory)?;
        // Even a malformed/partial active file blocks fallback to pending.
        let active_exists = directory
            .join(ACTIVE)
            .try_exists()
            .map_err(|_| Error::Storage)?;
        let (name, phase) = if active_exists {
            (ACTIVE, Phase::Active)
        } else {
            (PENDING, Phase::PendingBackup)
        };
        let material = unseal(&read_file(&directory.join(name))?, password, phase)?;
        let (account_count, selected_account) = account_state::read(directory, &material)?;
        Ok(Self {
            _profile_lock: profile_lock,
            directory: directory.to_owned(),
            phase,
            // Status-only open retains no active spending material.
            material: if phase == Phase::PendingBackup || spending {
                Some(material)
            } else {
                None
            },
            expires: Instant::now() + SESSION_TTL,
            account_count,
            selected_account,
        })
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn unlocked_addresses(&mut self, directory: &Path) -> Result<AccountAddresses> {
        self.check_spending_profile(directory)?;
        let index = self.selected_account;
        accounts::from_entropy_at(&self.material()?.entropy, index)
    }
    pub fn list_accounts(&mut self, directory: &Path) -> Result<(u32, Vec<String>)> {
        self.check_spending_profile(directory)?;
        let count = self.account_count;
        let addresses = (0..count).map(|index| accounts::from_entropy_at(&self.material()?.entropy, index).map(|a| a.bsc)).collect::<Result<_>>()?;
        Ok((self.selected_account, addresses))
    }
    pub fn select_account(&mut self, directory: &Path, index: u32) -> Result<AccountAddresses> {
        self.check_spending_profile(directory)?;
        if index >= self.account_count { return Err(Error::InvalidFormat); }
        let addresses = accounts::from_entropy_at(&self.material()?.entropy, index)?;
        let count = self.account_count;
        account_state::write(directory, self.material()?, count, index)?;
        self.selected_account = index;
        Ok(addresses)
    }
    pub fn add_account(&mut self, directory: &Path) -> Result<AccountAddresses> {
        self.check_spending_profile(directory)?;
        let index = self.account_count;
        if index >= MAX_ACCOUNTS { return Err(Error::InvalidFormat); }
        let addresses = accounts::from_entropy_at(&self.material()?.entropy, index)?;
        account_state::write(directory, self.material()?, index + 1, index)?;
        self.account_count = index + 1;
        self.selected_account = index;
        Ok(addresses)
    }
    fn check_spending_profile(&mut self, directory: &Path) -> Result<()> {
        self.expire();
        if self.phase != Phase::Active {
            return Err(Error::NotActive);
        }
        if self.material.is_none() {
            return Err(Error::Locked);
        }
        if self.directory != directory {
            return Err(Error::Authentication);
        }
        Ok(())
    }
    pub fn lock(&mut self) {
        self.material = None;
    }

    /// Explicit foreground user activity only; background queries never renew.
    pub fn user_activity(&mut self) -> Result<()> {
        self.material()?;
        self.expires = Instant::now() + SESSION_TTL;
        Ok(())
    }

    /// Called by the owning signer's timer even while no requests arrive.
    pub fn expire(&mut self) {
        if Instant::now() >= self.expires {
            self.lock();
        }
    }

    fn material(&mut self) -> Result<&Material> {
        self.expire();
        self.material.as_ref().ok_or(Error::Locked)
    }

    /// Returned buffer zeroizes on drop. Caller must not log/copy/serialize it.
    pub fn backup_words(&mut self) -> Result<Zeroizing<String>> {
        if self.phase != Phase::PendingBackup {
            return Err(Error::NotPending);
        }
        // Another session may already have activated the profile.
        if self
            .directory
            .join(ACTIVE)
            .try_exists()
            .map_err(|_| Error::Storage)?
        {
            self.lock();
            return Err(Error::NotPending);
        }
        let material = self.material()?;
        let mnemonic = Mnemonic::from_entropy(&material.entropy).map_err(|_| Error::Crypto)?;
        Ok(Zeroizing::new(mnemonic.to_string()))
    }

    /// Return public receive addresses only after successful backup activation.
    pub fn confirm_backup_addresses(&mut self, words: &str) -> Result<AccountAddresses> {
        let entropy = parse_backup(words)?;
        let addresses = accounts::from_entropy(&entropy)?;
        self.activate(words, true)?;
        Ok(addresses)
    }

    /// Writes a separate authenticated active record, preserving the pending recovery file.
    pub fn confirm_backup(&mut self, words: &str) -> Result<()> {
        self.activate(words, false)
    }
    fn activate(&mut self, words: &str, spending: bool) -> Result<()> {
        if self.phase != Phase::PendingBackup {
            return Err(Error::NotPending);
        }
        let supplied = parse_backup(words)?;
        let material = self.material()?;
        if *supplied != *material.entropy {
            return Err(Error::InvalidBackup);
        }
        if self
            .directory
            .join(ACTIVE)
            .try_exists()
            .map_err(|_| Error::Storage)?
        {
            return Err(Error::AlreadyExists);
        }
        let stage = self.directory.join(storage::STAGED);
        if stage.try_exists().map_err(|_| Error::Storage)? {
            let bytes = read_file(&stage)?;
            // Resume only this exact pending wallet's authenticated activation.
            let material = self.material()?;
            if !valid_record(&bytes)
                || bytes[8] != Phase::Active as u8
                || bytes[9..25] != material.salt
            {
                return Err(Error::InvalidFormat);
            }
            let cipher =
                XChaCha20Poly1305::new_from_slice(&*material.key).map_err(|_| Error::Crypto)?;
            let nonce =
                XNonce::try_from(&bytes[25..HEADER_LEN]).map_err(|_| Error::InvalidFormat)?;
            let recovered = Zeroizing::new(
                cipher
                    .decrypt(
                        &nonce,
                        Payload {
                            msg: &bytes[HEADER_LEN..],
                            aad: &bytes[..HEADER_LEN],
                        },
                    )
                    .map_err(|_| Error::Authentication)?,
            );
            if recovered.as_slice() != material.entropy.as_slice() {
                return Err(Error::InvalidBackup);
            }
        } else {
            let bytes = seal(self.material()?, Phase::Active)?;
            write_new(&stage, &bytes)?;
        }
        // A complete stage found after interruption must also be flushed before
        // publishing it; completeness alone is not a persistence guarantee.
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(&stage)
            .and_then(|file| file.sync_all())
            .map_err(|_| Error::Storage)?;
        storage::commit(&self.directory)?;
        self.phase = Phase::Active;
        if spending {
            self.expires = Instant::now() + SESSION_TTL;
        } else {
            self.lock();
        }
        Ok(())
    }
}

fn parse_backup(words: &str) -> Result<Zeroizing<Vec<u8>>> {
    if words.len() > 256 {
        return Err(Error::InvalidBackup);
    }
    let mnemonic = Mnemonic::parse_in_normalized(Language::English, words)
        .map_err(|_| Error::InvalidBackup)?;
    Ok(Zeroizing::new(mnemonic.to_entropy()))
}

#[cfg(test)]
mod tests;
