use super::*;

// BIP39/Trezor public vector: zero entropy, never use for funds.
const WORDS: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art";
const PASSWORD: &[u8] = b"public-test-password-only";

#[test]
fn tron_restart_replay_account_binding_expiry_and_pending_lock() {
    use chain_tron::{Address, Transaction, Transfer, mainnet::{Quote,policy}};
    let dir=TestDir::new(); let mut wallet=pending(&dir); wallet.confirm_backup(WORDS).unwrap(); drop(wallet);
    let journal=dir.0.join("tron-test.sqlite"); wallet_journal::Journal::open(&journal).unwrap();
    let mut wallet=Session::unlock(&dir.0,PASSWORD).unwrap();
    let sender=wallet.tron_address(&dir.0).unwrap();
    let quote=|| {let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
        Quote {tx:Transaction {owner:Address::parse(&sender).unwrap(),ref_block_bytes:[1,2],ref_block_hash:[3;8],timestamp_ms:now,expiration_ms:now+60000,fee_limit:30_000_000,
            transfer:Transfer::Token {contract:policy().token_contract.unwrap(),recipient:Address::parse("TW1dU4L3eNm7Lw8WvieLKEHpXWAussRG9Z").unwrap(),amount:wallet_core::Amount::from_u64(1_000_001)}},bandwidth_fee:400_000}};
    let signed=wallet.sign_tron(&dir.0,&journal,quote()).unwrap();
    let bytes=wallet.release_tron(&dir.0,&journal,&signed.id).unwrap();
    Quote::parse(&signed.summary).unwrap().verify_signed(&bytes,&signed.hash).unwrap();
    assert!(wallet.sign_tron(&dir.0,&journal,quote()).is_err());
    wallet.add_account(&dir.0).unwrap();
    assert_ne!(wallet.tron_address(&dir.0).unwrap(),sender);
    assert!(wallet.release_tron(&dir.0,&journal,&signed.id).is_err());
    assert!(wallet.sign_tron(&dir.0,&journal,quote()).is_err());
    wallet.select_account(&dir.0,0).unwrap(); drop(wallet);
    let mut wallet=Session::unlock(&dir.0,PASSWORD).unwrap();
    assert_eq!(wallet.release_tron(&dir.0,&journal,&signed.id).unwrap(),bytes);
    wallet.lock(); assert!(wallet.release_tron(&dir.0,&journal,&signed.id).is_err()); drop(wallet);
    let ledger=wallet_journal::Journal::open_existing(&journal).unwrap();
    assert!(ledger.tron_observe(&signed.id,&signed.hash,"expired").is_err());
    ledger.tron_observe(&signed.id,&signed.hash,"finalized").unwrap(); drop(ledger);
    let mut wallet=Session::unlock(&dir.0,PASSWORD).unwrap();
    assert!(wallet.release_tron(&dir.0,&journal,&signed.id).is_err());
    let mut stale=quote(); stale.tx.timestamp_ms-=120_000; stale.tx.expiration_ms-=120_000;
    assert!(wallet.sign_tron(&dir.0,&journal,stale).is_err());
}

#[test]
fn solana_restart_replay_account_binding_and_lock() {
    use chain_solana::{Address,Asset,Quote,Transfer};
    let dir=TestDir::new(); let mut wallet=pending(&dir); wallet.confirm_backup(WORDS).unwrap(); drop(wallet);
    let journal=dir.0.join("solana-test.sqlite"); wallet_journal::Journal::open(&journal).unwrap();
    let mut wallet=Session::unlock(&dir.0,PASSWORD).unwrap();
    let sender=wallet.solana_address(&dir.0).unwrap();
    let fixture:Vec<_>=include_str!("../../../../tests/fixtures/solana-vectors.txt").lines().find(|l|!l.starts_with('#')).unwrap().split('|').collect();
    let quote=||Quote {transfer:Transfer {sender:Address::parse(&sender).unwrap(),recipient:Address::parse(fixture[5]).unwrap(),asset:Asset::Usdt,amount:1_000_001,blockhash:Address::parse(fixture[6]).unwrap(),create_recipient_account:true},last_valid_height:1000,fee:5000,rent:2_039_280};
    let signed=wallet.sign_solana(&dir.0,&journal,quote()).unwrap();
    let bytes=wallet.release_solana(&dir.0,&journal,&signed.id).unwrap();
    assert!(wallet.sign_solana(&dir.0,&journal,quote()).is_err());
    wallet.add_account(&dir.0).unwrap(); wallet.select_account(&dir.0,1).unwrap();
    assert_ne!(wallet.solana_address(&dir.0).unwrap(),sender);
    assert!(wallet.release_solana(&dir.0,&journal,&signed.id).is_err());
    assert!(wallet.sign_solana(&dir.0,&journal,quote()).is_err());
    wallet.select_account(&dir.0,0).unwrap(); drop(wallet);
    let mut wallet=Session::unlock(&dir.0,PASSWORD).unwrap();
    assert_eq!(wallet.release_solana(&dir.0,&journal,&signed.id).unwrap(),bytes);
    wallet.lock(); assert!(wallet.release_solana(&dir.0,&journal,&signed.id).is_err());
    drop(wallet);
    let ledger=wallet_journal::Journal::open_existing(&journal).unwrap();
    ledger.solana_observe(&signed.id,&signed.hash,"finalized").unwrap(); drop(ledger);
    let mut wallet=Session::unlock(&dir.0,PASSWORD).unwrap();
    assert!(wallet.release_solana(&dir.0,&journal,&signed.id).is_err());
    wallet.expires=Instant::now(); assert!(wallet.sign_solana(&dir.0,&journal,quote()).is_err());
}

#[test]
fn new_wallet_defaults_to_twelve_words() {
    let dir = TestDir::new();
    let mut wallet = Session::create_new(&dir.0, PASSWORD).unwrap();
    let words = wallet.backup_words().unwrap();
    assert_eq!(words.split_whitespace().count(), 12);
    let addresses = wallet.confirm_backup_addresses(&words).unwrap();
    drop(wallet);
    assert_eq!(Session::account_addresses(&dir.0, PASSWORD).unwrap(), addresses);
    assert_eq!(Session::export_backup(&dir.0, PASSWORD).unwrap(), words);
}

#[test]
fn all_bip39_lengths_survive_restore_restart_backup_and_signing() {
    for length in [16, 20, 24, 28, 32] {
        let dir = TestDir::new();
        let words = Zeroizing::new(Mnemonic::from_entropy(&vec![0; length]).unwrap().to_string());
        let mut wallet = Session::restore_new(&dir.0, PASSWORD, &words).unwrap();
        assert_eq!(wallet.backup_words().unwrap().as_str(), words.as_str());
        drop(wallet);
        wallet = Session::open(&dir.0, PASSWORD).unwrap();
        let addresses = wallet.confirm_backup_addresses(&words).unwrap();
        // Independently computed with ethers HDNodeWallet, standard account zero.
        let expected = match length {
            16 => "0x9858EfFD232B4033E47d90003D41EC34EcaEda94",
            20 => "0x54bE88525B024a20229Fe7f6F62DC0884e4aAA0a",
            24 => "0x197A1bEE163923815Ba58EaD0F14B3Fcd8C5926d",
            28 => "0x15cc2210a0AFbb1A2d80Dd3D585aA94eBa6c10E0",
            32 => "0xF278cF59F82eDcf871d630F28EcC8056f25C1cdb",
            _ => unreachable!(),
        };
        assert_eq!(addresses.bsc, expected);
        drop(wallet);
        assert_eq!(Session::export_backup(&dir.0, PASSWORD).unwrap().as_str(), words.as_str());
        assert_eq!(Session::account_addresses(&dir.0, PASSWORD).unwrap(), addresses);
        let journal = dir.0.join("length-test.sqlite");
        let request = BnbRequest { sender: &addresses.bsc, recipient: &addresses.bsc,
            amount: "0.000000000000000001", nonce: 0, gas_price_wei: 1_000_000_000 };
        let signed = Session::sign_bnb(&dir.0, PASSWORD, &journal, request).unwrap();
        let raw = Session::release_bnb(&dir.0, PASSWORD, &journal, &signed.id).unwrap();
        assert_eq!(hex::encode(chain_evm::keccak256(&raw)), signed.hash);
        for suffix in ["", "-wal", "-shm"] {
            let path = PathBuf::from(format!("{}{suffix}", journal.display()));
            if path.exists() { fs::remove_file(path).unwrap(); }
        }
    }
}

#[test]
fn active_backup_requires_password_and_preserves_wallet() {
    let dir = TestDir::new();
    let mut wallet = pending(&dir);
    drop(wallet);
    assert_eq!(Session::export_backup(&dir.0, PASSWORD).err(), Some(Error::NotActive));
    wallet = Session::open(&dir.0, PASSWORD).unwrap();
    wallet.confirm_backup(WORDS).unwrap();
    drop(wallet);
    let before = fs::read(dir.0.join(ACTIVE)).unwrap();
    assert_eq!(Session::export_backup(&dir.0, b"incorrect-password").err(), Some(Error::Authentication));
    let words = Session::export_backup(&dir.0, PASSWORD).unwrap();
    assert_eq!(words.as_str(), WORDS);
    assert_eq!(before, fs::read(dir.0.join(ACTIVE)).unwrap());
    let restored = TestDir::new();
    let mut restored_wallet = Session::restore_new(&restored.0, PASSWORD, &words).unwrap();
    let addresses = restored_wallet.confirm_backup_addresses(&words).unwrap();
    assert_eq!(addresses.bsc, Session::account_addresses(&dir.0, PASSWORD).unwrap().bsc);
}

#[test]
fn unlocked_spending_is_bound_to_profile_expiry_and_lock() {
    let dir = TestDir::new();
    let mut wallet = pending(&dir);
    let path = dir.0.join("session-test.sqlite");
    assert_eq!(
        wallet.unlocked_addresses(&dir.0).err(),
        Some(Error::NotActive)
    );
    let accounts = wallet.confirm_backup_addresses(WORDS).unwrap();
    assert!(wallet.material.is_some());
    assert_eq!(wallet.backup_words().err(), Some(Error::NotPending));
    let request = || BnbRequest {
        sender: &accounts.bsc,
        recipient: &accounts.bsc,
        amount: "1.234567890123456789",
        nonce: 7,
        gas_price_wei: 1_000_000_000,
    };
    assert_eq!(
        wallet
            .sign_unlocked(&dir.0.join("other"), &path, request(), true, 60000)
            .err(),
        Some(Error::Authentication)
    );
    let signed = wallet
        .sign_unlocked(&dir.0, &path, request(), true, 60000)
        .unwrap();
    let raw = wallet.release_unlocked(&dir.0, &path, &signed.id).unwrap();
    assert_eq!(hex::encode(chain_evm::keccak256(&raw)), signed.hash);
    wallet.expires = Instant::now();
    assert_eq!(
        wallet.release_unlocked(&dir.0, &path, &signed.id).err(),
        Some(Error::Locked)
    );
    assert_eq!(
        wallet
            .sign_unlocked(&dir.0, &path, request(), true, 60000)
            .err(),
        Some(Error::Locked)
    );
    assert!(wallet.material.is_none());
    drop(wallet);
    assert!(Session::unlock(&dir.0, b"incorrect-password").is_err());
    let mut wallet = Session::unlock(&dir.0, PASSWORD).unwrap();
    assert_eq!(
        &*raw,
        &*wallet.release_unlocked(&dir.0, &path, &signed.id).unwrap()
    );
    wallet.lock();
    assert_eq!(
        wallet.release_unlocked(&dir.0, &path, &signed.id).err(),
        Some(Error::Locked)
    );
    drop(wallet);
    for suffix in ["", "-wal", "-shm"] {
        let file = PathBuf::from(format!("{}{suffix}", path.display()));
        if file.exists() {
            fs::remove_file(file).unwrap();
        }
    }
}

#[test]
fn bnb_signed_journal_restart_releases_identical_payload_and_blocks_second_payment() {
    let dir = TestDir::new();
    let mut wallet = pending(&dir);
    wallet.confirm_backup(WORDS).unwrap();
    drop(wallet);
    let accounts = Session::account_addresses(&dir.0, PASSWORD).unwrap();
    let journal_path = dir.0.join("public-journal.sqlite");
    let request = || BnbRequest {
        sender: &accounts.bsc,
        recipient: &accounts.bsc,
        amount: "0.000000000000000001",
        nonce: 7,
        gas_price_wei: 1_000_000_000,
    };
    assert!(Session::sign_bnb(&dir.0, b"incorrect-password", &journal_path, request()).is_err());
    assert!(!journal_path.exists());
    let receipt = Session::sign_bnb(&dir.0, PASSWORD, &journal_path, request()).unwrap();
    let first = Session::release_bnb(&dir.0, PASSWORD, &journal_path, &receipt.id).unwrap();
    assert_eq!(hex::encode(chain_evm::keccak256(&first)), receipt.hash);
    let oracle: Vec<_> = include_str!("../../../../tests/fixtures/bnb-journal.txt")
        .lines()
        .filter(|line| !line.starts_with('#'))
        .collect();
    assert_eq!(receipt.hash, oracle[0]);
    assert_eq!(hex::encode(&*first), oracle[1]);
    let (decoded, recovered) = chain_evm::decode_signed_transfer(
        &first,
        chain_evm::Policy {
            chain_id: 56,
            token_contract: None,
        },
    )
    .unwrap();
    assert_eq!(recovered.checksum(), accounts.bsc);
    assert_eq!(decoded.nonce, 7);
    assert!(Session::sign_bnb(&dir.0, PASSWORD, &journal_path, request()).is_err());
    let second = Session::release_bnb(&dir.0, PASSWORD, &journal_path, &receipt.id).unwrap();
    assert_eq!(&*first, &*second);
    let journal = wallet_journal::Journal::open(&journal_path).unwrap();
    let record = journal.record(&receipt.id).unwrap();
    assert_eq!(record.attempts, 2);
    assert_eq!(record.state, "broadcasting");
    assert!(
        !record
            .ciphertext
            .windows(first.len())
            .any(|part| part == first.as_slice())
    );
    drop(journal);
    fs::remove_file(&journal_path).unwrap();
    for suffix in ["-wal", "-shm"] {
        let path = PathBuf::from(format!("{}{suffix}", journal_path.display()));
        if path.exists() {
            fs::remove_file(path).unwrap();
        }
    }
}

struct TestDir(PathBuf);

#[test]
fn usdt_signing_matches_ethers_replays_exactly_and_blocks_cross_asset_payment() {
    let dir = TestDir::new();
    let mut wallet = pending(&dir);
    wallet.confirm_backup(WORDS).unwrap();
    drop(wallet);
    let accounts = Session::account_addresses(&dir.0, PASSWORD).unwrap();
    let path = dir.0.join("usdt-test.sqlite");
    let request = || BnbRequest {
        sender: &accounts.bsc,
        recipient: &accounts.bsc,
        amount: "1.234567890123456789",
        nonce: 7,
        gas_price_wei: 1_000_000_000,
    };
    for gas in [0, 20_999, 200_001, u64::MAX] {
        assert!(Session::sign_usdt(&dir.0, PASSWORD, &path, request(), gas).is_err());
    }
    assert!(!path.exists());
    let signed = Session::sign_usdt(&dir.0, PASSWORD, &path, request(), 60_000).unwrap();
    assert!(signed.summary.starts_with("USDT\n56\n"));
    assert!(signed.summary.ends_with(&chain_evm::bsc::USDT.checksum()));
    let raw = Session::release_bnb(&dir.0, PASSWORD, &path, &signed.id).unwrap();
    let oracle: Vec<_> = include_str!("../../../../tests/fixtures/usdt-journal.txt")
        .lines()
        .filter(|v| !v.starts_with('#'))
        .collect();
    assert_eq!(signed.hash, oracle[0]);
    assert_eq!(hex::encode(&*raw), oracle[1]);
    let (tx, sender) = chain_evm::decode_signed_transfer(&raw, chain_evm::bsc::POLICY).unwrap();
    assert_eq!(sender.checksum(), accounts.bsc);
    assert!(
        matches!(tx.transfer,chain_evm::Transfer::Token { contract, .. } if contract == chain_evm::bsc::USDT)
    );
    assert!(Session::sign_bnb(&dir.0, PASSWORD, &path, request()).is_err());
    assert!(Session::sign_usdt(&dir.0, PASSWORD, &path, request(), 60_000).is_err());
    assert_eq!(
        &*raw,
        &*Session::release_bnb(&dir.0, PASSWORD, &path, &signed.id).unwrap()
    );
    assert!(
        chain_evm::decode_signed_transfer(
            &raw,
            chain_evm::Policy {
                chain_id: 56,
                token_contract: None
            }
        )
        .is_err()
    );
    fs::remove_file(&path).unwrap();
    for suffix in ["-wal", "-shm"] {
        let side = PathBuf::from(format!("{}{suffix}", path.display()));
        if side.exists() {
            fs::remove_file(side).unwrap();
        }
    }
}
impl TestDir {
    fn new() -> Self {
        let mut id = [0; 16];
        random(&mut id).unwrap();
        Self(std::env::temp_dir().join(format!("web3rust-vault-test-{}", hex::encode(id))))
    }
}

#[test]
fn multiple_accounts_persist_recover_and_bind_signing_and_replay() {
    let dir = TestDir::new();
    let journal = dir.0.with_extension("sqlite");
    let words;
    let first;
    let second;
    let signed;
    let raw;
    {
        let mut session = pending(&dir);
        words = session.backup_words().unwrap();
        first = session.confirm_backup_addresses(&words).unwrap();
        second = session.add_account(&dir.0).unwrap();
        assert_ne!(first.bsc, second.bsc);
        assert_eq!(session.list_accounts(&dir.0).unwrap(), (1, vec![first.bsc.clone(), second.bsc.clone()]));
        assert_eq!(session.select_account(&dir.0, 2).err(), Some(Error::InvalidFormat));
        let request = |sender| BnbRequest { sender, recipient: &second.bsc, amount: "0.000000000000000001", nonce: 0, gas_price_wei: 1_000_000_000 };
        assert_eq!(session.sign_unlocked(&dir.0, &journal, request(&first.bsc), false, 21000).err(), Some(Error::Authentication));
        signed = session.sign_unlocked(&dir.0, &journal, request(&second.bsc), false, 21000).unwrap();
        raw = session.release_unlocked(&dir.0, &journal, &signed.id).unwrap();
        let (_, sender) = chain_evm::decode_signed_transfer(&raw, chain_evm::Policy { chain_id: 56, token_contract: None }).unwrap();
        assert_eq!(sender.checksum(), second.bsc);
        session.select_account(&dir.0, 0).unwrap();
        assert_eq!(session.release_unlocked(&dir.0, &journal, &signed.id).err(), Some(Error::Authentication));
        session.select_account(&dir.0, 1).unwrap();
        session.expires = Instant::now();
        assert_eq!(session.add_account(&dir.0).err(), Some(Error::Locked));
        assert_eq!(session.select_account(&dir.0, 0).err(), Some(Error::Locked));
    }
    {
        let mut session = Session::unlock(&dir.0, PASSWORD).unwrap();
        assert_eq!(session.unlocked_addresses(&dir.0).unwrap(), second);
        assert_eq!(*session.release_unlocked(&dir.0, &journal, &signed.id).unwrap(), *raw);
        session.select_account(&dir.0, 0).unwrap();
        let request = BnbRequest { sender: &first.bsc, recipient: &first.bsc, amount: "1", nonce: 0, gas_price_wei: 1_000_000_000 };
        // An unresolved payment for account 02 does not lock account 01's nonce.
        session.sign_unlocked(&dir.0, &journal, request, true, 60000).unwrap();
    }
    assert_eq!(*Session::export_backup(&dir.0, PASSWORD).unwrap(), *words);
    let restored = TestDir::new();
    {
        let mut session = Session::restore_new(&restored.0, PASSWORD, &words).unwrap();
        assert_eq!(session.confirm_backup_addresses(&words).unwrap(), first);
        assert_eq!(session.add_account(&restored.0).unwrap(), second);
    }
    let other = TestDir::new();
    {
        let mut session = pending(&other);
        let words = session.backup_words().unwrap(); session.confirm_backup_addresses(&words).unwrap();
        session.add_account(&other.0).unwrap();
    }
    let listed = Session::deletion_addresses(&dir.0, PASSWORD).unwrap();
    assert_eq!(listed, vec![first.bsc.clone(), second.bsc.clone()]);
    Session::delete_wallet(&dir.0, PASSWORD, &journal).unwrap();
    assert!(!dir.0.exists()); assert!(other.0.join(ACTIVE).exists());
    {
        let ledger = wallet_journal::Journal::open_existing(&journal).unwrap();
        assert!(ledger.history(&first.bsc).unwrap().is_empty());
        assert!(ledger.history(&second.bsc).unwrap().is_empty());
    }
    fs::remove_file(journal).unwrap();
}

#[test]
fn account_state_authentication_limit_and_failed_write_are_safe() {
    let dir = TestDir::new();
    let mut session = pending(&dir);
    let words = session.backup_words().unwrap(); session.confirm_backup_addresses(&words).unwrap();
    // A staging write failure cannot select an account that was not persisted.
    fs::create_dir(dir.0.join(account_state::NEXT)).unwrap();
    assert!(session.add_account(&dir.0).is_err());
    assert_eq!(session.list_accounts(&dir.0).unwrap().1.len(), 1);
    fs::remove_dir(dir.0.join(account_state::NEXT)).unwrap();
    session.add_account(&dir.0).unwrap();
    let bytes = fs::read(dir.0.join(account_state::STATE)).unwrap();
    // Exercise the IPC-size bound at the maximum without 32 redundant writes.
    account_state::write(&dir.0, session.material().unwrap(), MAX_ACCOUNTS, MAX_ACCOUNTS-1).unwrap();
    drop(session);
    let mut session = Session::unlock(&dir.0, PASSWORD).unwrap();
    assert_eq!(session.list_accounts(&dir.0).unwrap().1.len(), MAX_ACCOUNTS as usize);
    assert_eq!(session.add_account(&dir.0).err(), Some(Error::InvalidFormat));
    drop(session);
    let mut damaged = bytes.clone(); damaged[30] ^= 1;
    fs::write(dir.0.join(account_state::STATE), &damaged).unwrap();
    assert_eq!(Session::unlock(&dir.0, PASSWORD).err(), Some(Error::Authentication));
    assert_eq!(fs::read(dir.0.join(account_state::STATE)).unwrap(), damaged);
    fs::write(dir.0.join(account_state::STATE), bytes).unwrap();
    fs::write(dir.0.join(account_state::NEXT), b"interrupted staging").unwrap();
    let mut session = Session::unlock(&dir.0, PASSWORD).unwrap();
    assert_eq!(session.list_accounts(&dir.0).unwrap().0, 1);
    session.add_account(&dir.0).unwrap();
    assert!(!dir.0.join(account_state::NEXT).exists());
}
impl Drop for TestDir {
    fn drop(&mut self) {
        // Only exact files owned by this fixture, no recursive deletion.
        for file in [PENDING, ACTIVE, storage::LOCK, storage::STAGED, account_state::STATE, account_state::NEXT] {
            let _ = fs::remove_file(self.0.join(file));
        }
        let _ = fs::remove_dir(&self.0);
    }
}
fn pending(dir: &TestDir) -> Session {
    Session::persist_new(&dir.0, PASSWORD, Zeroizing::new(vec![0; 32])).unwrap()
}

#[test]
fn public_mnemonic_and_seed_vector() {
    let entropy = parse_backup(WORDS).unwrap();
    assert_eq!(*entropy, [0; 32]);
    let mnemonic = Mnemonic::from_entropy(&entropy).unwrap();
    assert_eq!(mnemonic.to_string(), WORDS);
    assert_eq!(
        hex::encode(mnemonic.to_seed("TREZOR")),
        "bda85446c68413707090a52022edd26a1c9462295029f2e60cd7c4f2bbd3097170af7a4d73245cafa9c3cca8d561a7c3de6f5d4a10be8ed2a5e608d68f92fcc8"
    );
    for invalid in [
        "",
        "abandon",
        &"a".repeat(257),
        &WORDS.replace("art", "abandon"),
    ] {
        assert!(parse_backup(invalid).is_err());
    }
}

#[test]
fn restart_retains_backup_then_active_refuses_display() {
    let dir = TestDir::new();
    let mut wallet = pending(&dir);
    let original = fs::read(dir.0.join(PENDING)).unwrap();
    assert_eq!(original.len(), FILE_LEN);
    assert!(!original.windows(32).any(|w| w == [0; 32]));
    assert_eq!(&*wallet.backup_words().unwrap(), WORDS);
    drop(wallet);
    let mut resumed = Session::open(&dir.0, PASSWORD).unwrap();
    assert_eq!(Session::open(&dir.0, PASSWORD).err(), Some(Error::Busy));
    assert_eq!(resumed.phase(), Phase::PendingBackup);
    assert_eq!(&*resumed.backup_words().unwrap(), WORDS);
    resumed.confirm_backup(WORDS).unwrap();
    assert_eq!(resumed.phase(), Phase::Active);
    assert!(resumed.material.is_none());
    assert_eq!(resumed.backup_words().err(), Some(Error::NotPending));
    drop(resumed);
    let mut active = Session::open(&dir.0, PASSWORD).unwrap();
    assert_eq!(active.phase(), Phase::Active);
    assert!(active.material.is_none());
    assert_eq!(active.backup_words().err(), Some(Error::NotPending));
    assert_eq!(fs::read(dir.0.join(PENDING)).unwrap(), original);
}

#[test]
fn wrong_password_tampering_and_truncation_never_change_files() {
    let dir = TestDir::new();
    drop(pending(&dir));
    let path = dir.0.join(PENDING);
    let original = fs::read(&path).unwrap();
    assert_eq!(
        Session::open(&dir.0, b"wrong-test-password").err(),
        Some(Error::Authentication)
    );
    assert_eq!(fs::read(&path).unwrap(), original);
    for offset in [8, 9, 25, HEADER_LEN, FILE_LEN - 1] {
        let mut changed = original.clone();
        changed[offset] ^= 1;
        fs::write(&path, &changed).unwrap();
        assert!(Session::open(&dir.0, PASSWORD).is_err());
        assert_eq!(fs::read(&path).unwrap(), changed);
    }
    for length in [0, 8, FILE_LEN - 1, FILE_LEN + 1] {
        let mut changed = original.clone();
        changed.resize(length, 0);
        fs::write(&path, &changed).unwrap();
        assert_eq!(
            Session::open(&dir.0, PASSWORD).err(),
            Some(Error::InvalidFormat)
        );
    }
}

#[test]
fn failed_activation_keeps_pending_and_blocks_silent_fallback() {
    let dir = TestDir::new();
    let mut wallet = pending(&dir);
    let original = fs::read(dir.0.join(PENDING)).unwrap();
    assert_eq!(
        wallet.confirm_backup("wrong words"),
        Err(Error::InvalidBackup)
    );
    assert!(!dir.0.join(ACTIVE).exists());
    // Simulate a crash after active-file creation but before full write/flush.
    fs::write(dir.0.join(ACTIVE), b"partial").unwrap();
    assert_eq!(wallet.confirm_backup(WORDS), Err(Error::AlreadyExists));
    assert_eq!(wallet.phase(), Phase::PendingBackup);
    drop(wallet);
    assert_eq!(
        Session::open(&dir.0, PASSWORD).err(),
        Some(Error::InvalidFormat)
    );
    assert_eq!(fs::read(dir.0.join(PENDING)).unwrap(), original);
}

#[test]
fn restore_is_exact_and_never_overwrites_existing_profile() {
    let dir = TestDir::new();
    let mut restored = Session::restore_new(&dir.0, PASSWORD, WORDS).unwrap();
    assert_eq!(&*restored.backup_words().unwrap(), WORDS);
    let original = fs::read(dir.0.join(PENDING)).unwrap();
    assert_eq!(
        Session::restore_new(&dir.0, PASSWORD, WORDS).err(),
        Some(Error::AlreadyExists)
    );
    assert_eq!(fs::read(dir.0.join(PENDING)).unwrap(), original);
    assert_eq!(
        Session::create_new(Path::new("relative"), b"short").err(),
        Some(Error::PasswordLength)
    );
    assert!(Session::restore_new(Path::new("relative"), PASSWORD, WORDS).is_err());
    let oversized = vec![b'x'; 1025];
    assert_eq!(
        Session::restore_new(&TestDir::new().0, &oversized, WORDS).err(),
        Some(Error::PasswordLength)
    );
}

#[test]
fn lock_and_expiry_clear_secret_material() {
    let dir = TestDir::new();
    let mut wallet = pending(&dir);
    wallet.expires = Instant::now();
    assert_eq!(wallet.backup_words().err(), Some(Error::Locked));
    assert!(wallet.material.is_none());
    assert_eq!(wallet.confirm_backup(WORDS), Err(Error::Locked));
    drop(wallet);
    let mut reopened = Session::open(&dir.0, PASSWORD).unwrap();
    reopened.lock();
    assert_eq!(reopened.backup_words().err(), Some(Error::Locked));
    assert!(reopened.material.is_none());
}

#[test]
fn user_activity_renews_two_hours_but_never_revives_expired_or_locked_keys() {
    assert_eq!(SESSION_TTL, Duration::from_secs(7200));
    let dir = TestDir::new();
    let mut wallet = pending(&dir);
    wallet.expires = Instant::now() + Duration::from_secs(1);
    let old = wallet.expires;
    wallet.backup_words().unwrap();
    assert_eq!(wallet.expires, old, "ordinary reads must not renew idle timeout");
    let before = Instant::now();
    wallet.user_activity().unwrap();
    assert!(wallet.expires >= before + SESSION_TTL);
    wallet.expires = Instant::now();
    assert_eq!(wallet.user_activity(), Err(Error::Locked));
    assert!(wallet.material.is_none());
    drop(wallet);
    let mut wallet = Session::open(&dir.0, PASSWORD).unwrap();
    wallet.lock();
    assert_eq!(wallet.user_activity(), Err(Error::Locked));
}

#[test]
fn original_restart_and_restored_profile_have_identical_chain_addresses() {
    let original_dir = TestDir::new();
    let mut original = pending(&original_dir);
    assert_eq!(
        Session::account_addresses(&original_dir.0, PASSWORD).err(),
        Some(Error::Busy)
    );
    drop(original);
    assert_eq!(
        Session::account_addresses(&original_dir.0, PASSWORD).err(),
        Some(Error::NotActive)
    );
    original = Session::open(&original_dir.0, PASSWORD).unwrap();
    original.confirm_backup(WORDS).unwrap();
    drop(original);
    let expected = Session::account_addresses(&original_dir.0, PASSWORD).unwrap();
    let restored_dir = TestDir::new();
    let mut restored = Session::restore_new(&restored_dir.0, PASSWORD, WORDS).unwrap();
    restored.confirm_backup(WORDS).unwrap();
    drop(restored);
    assert_eq!(
        Session::account_addresses(&restored_dir.0, PASSWORD).unwrap(),
        expected
    );
    assert_eq!(
        Session::account_addresses(&original_dir.0, b"incorrect-password").err(),
        Some(Error::Authentication)
    );
    let fixture = include_str!("../../../../tests/fixtures/hd-accounts.txt");
    let fields: Vec<_> = fixture.lines().nth(1).unwrap().split('|').collect();
    assert_eq!(expected.bsc, fields[1]);
    assert_eq!(expected.tron, fields[2]);
}

#[test]
fn staged_activation_resumes_after_restart_without_replacing_wallet() {
    let dir = TestDir::new();
    let mut wallet = pending(&dir);
    let original = fs::read(dir.0.join(PENDING)).unwrap();
    let bytes = seal(wallet.material().unwrap(), Phase::Active).unwrap();
    write_new(&dir.0.join(storage::STAGED), &bytes).unwrap();
    drop(wallet); // crash boundary after flush, before rename
    assert!(!dir.0.join(ACTIVE).exists());
    let mut resumed = Session::open(&dir.0, PASSWORD).unwrap();
    assert_eq!(resumed.phase(), Phase::PendingBackup);
    resumed.confirm_backup(WORDS).unwrap();
    assert_eq!(fs::read(dir.0.join(ACTIVE)).unwrap(), bytes);
    assert!(!dir.0.join(storage::STAGED).exists());
    assert_eq!(fs::read(dir.0.join(PENDING)).unwrap(), original);
}

#[test]
fn partial_staging_does_not_become_active_or_destroy_pending() {
    let dir = TestDir::new();
    let mut wallet = pending(&dir);
    let original = fs::read(dir.0.join(PENDING)).unwrap();
    fs::write(dir.0.join(storage::STAGED), b"partial").unwrap();
    assert_eq!(wallet.confirm_backup(WORDS), Err(Error::InvalidFormat));
    assert!(!dir.0.join(ACTIVE).exists());
    assert_eq!(&*wallet.backup_words().unwrap(), WORDS);
    assert_eq!(fs::read(dir.0.join(PENDING)).unwrap(), original);
}

#[test]
fn authenticated_stage_for_different_entropy_is_rejected() {
    let dir = TestDir::new();
    let mut wallet = pending(&dir);
    let material = wallet.material().unwrap();
    let other = Material {
        entropy: Zeroizing::new(vec![1; 32]),
        key: Zeroizing::new(*material.key),
        salt: material.salt,
    };
    write_new(
        &dir.0.join(storage::STAGED),
        &seal(&other, Phase::Active).unwrap(),
    )
    .unwrap();
    assert_eq!(wallet.confirm_backup(WORDS), Err(Error::InvalidBackup));
    assert!(!dir.0.join(ACTIVE).exists());
}

#[test]
fn native_commit_never_replaces_an_existing_active_file() {
    let dir = TestDir::new();
    let _wallet = pending(&dir);
    fs::write(dir.0.join(storage::STAGED), b"candidate").unwrap();
    fs::write(dir.0.join(ACTIVE), b"existing").unwrap();
    assert_eq!(storage::commit(&dir.0), Err(Error::AlreadyExists));
    assert_eq!(fs::read(dir.0.join(ACTIVE)).unwrap(), b"existing");
    assert_eq!(fs::read(dir.0.join(storage::STAGED)).unwrap(), b"candidate");
}

#[test]
fn profile_lock_is_enforced_in_a_separate_process() {
    use std::os::windows::process::CommandExt;
    let dir = TestDir::new();
    let wallet = pending(&dir);
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "tests::profile_lock_probe", "--ignored"])
        .env("W3_PUBLIC_LOCK_TEST_DIR", &dir.0)
        .creation_flags(0x08000000)
        .output()
        .unwrap();
    assert!(output.status.success(), "lock probe failed");
    drop(wallet);
    assert!(Session::open(&dir.0, PASSWORD).is_ok());
}

#[test]
#[ignore = "invoked with a public fixture path by profile_lock_is_enforced_in_a_separate_process"]
fn profile_lock_probe() {
    let directory = PathBuf::from(std::env::var_os("W3_PUBLIC_LOCK_TEST_DIR").unwrap());
    assert_eq!(storage::acquire(&directory).err(), Some(Error::Busy));
}
