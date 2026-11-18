//! Dedicated inherited-pipe protocol, deliberately separate from public mock IPC.
//! No arbitrary filenames, paths, private-key export, hashes or signing requests.
use std::{
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use wallet_secrets::{Error, Session};
use zeroize::Zeroizing;

const MAX_FRAME: usize = 2048;
const VERSION: u8 = 2;
const INVALID: u16 = 1;
const AUTH: u16 = 2;
const STATE: u16 = 3;
const STORAGE: u16 = 4;
type Reply = Result<Zeroizing<Vec<u8>>, u16>;

pub fn serve() -> io::Result<()> {
    wallet_sandbox::verify_offline_token()?;
    // Never trust command-line/environment paths for the vault root.
    let root = wallet_sandbox::current_container_directory()?.join("profiles");
    std::fs::create_dir_all(&root)?;
    let session: Mutex<Option<Session>> = Mutex::new(None);
    let stop = AtomicBool::new(false);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            while !stop.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(250));
                if let Ok(mut guard) = session.lock()
                    && let Some(value) = guard.as_mut()
                {
                    value.expire();
                }
            }
        });
        let result = exchange(&root, &session, io::stdin(), io::stdout());
        stop.store(true, Ordering::Release);
        result
    })
}

fn exchange(
    root: &Path,
    session: &Mutex<Option<Session>>,
    mut input: impl Read,
    mut output: impl Write,
) -> io::Result<()> {
    let mut expected = 1u64;
    loop {
        let mut length = [0; 4];
        match input.read(&mut length[..1])? {
            0 => return Ok(()),
            1 => (),
            _ => unreachable!(),
        }
        input.read_exact(&mut length[1..])?;
        let length = u32::from_le_bytes(length) as usize;
        if !(10..=MAX_FRAME).contains(&length) {
            return Err(io::Error::other("invalid frame"));
        }
        let mut frame = Zeroizing::new(vec![0; length]);
        input.read_exact(&mut frame)?;
        let id = u64::from_le_bytes(frame[2..10].try_into().unwrap());
        if frame[0] != VERSION || id != expected {
            return Err(io::Error::other("session sequence"));
        }
        expected = expected
            .checked_add(1)
            .ok_or_else(|| io::Error::other("sequence exhausted"))?;
        let mut guard = session
            .lock()
            .map_err(|_| io::Error::other("session unavailable"))?;
        let result = dispatch(root, &mut guard, frame[1], &frame[10..]);
        let (code, body) = match result {
            Ok(body) => (0u16, body),
            Err(code) => (code, Zeroizing::new(Vec::new())),
        };
        let mut reply = Zeroizing::new(Vec::with_capacity(12 + body.len()));
        reply.extend([VERSION, frame[1]]);
        reply.extend(id.to_le_bytes());
        reply.extend(code.to_le_bytes());
        reply.extend_from_slice(&body);
        output.write_all(&(reply.len() as u32).to_le_bytes())?;
        output.write_all(&reply)?;
        output.flush()?;
    }
}

struct Fields<'a> {
    data: &'a [u8],
}
impl<'a> Fields<'a> {
    fn take(&mut self, max: usize) -> Result<&'a [u8], u16> {
        if self.data.len() < 2 {
            return Err(INVALID);
        }
        let length = u16::from_le_bytes(self.data[..2].try_into().unwrap()) as usize;
        if length > max || self.data.len() < length + 2 {
            return Err(INVALID);
        }
        let value = &self.data[2..length + 2];
        self.data = &self.data[length + 2..];
        Ok(value)
    }
    fn text(&mut self, max: usize) -> Result<&'a str, u16> {
        std::str::from_utf8(self.take(max)?).map_err(|_| INVALID)
    }
    fn number(&mut self) -> Result<u64, u16> {
        let text = self.text(20)?;
        if text.is_empty()
            || (text.len() > 1 && text.starts_with('0'))
            || !text.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(INVALID);
        }
        text.parse().map_err(|_| INVALID)
    }
    fn done(&self) -> Result<(), u16> {
        if self.data.is_empty() {
            Ok(())
        } else {
            Err(INVALID)
        }
    }
    fn profile(&mut self, root: &Path) -> Result<PathBuf, u16> {
        let name = self.text(48)?;
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(INVALID);
        }
        // Prefix also excludes Windows reserved device names such as CON and NUL.
        Ok(root.join(format!("wallet-{name}")))
    }
}
fn error(value: Error) -> u16 {
    match value {
        Error::PasswordLength | Error::InvalidBackup => INVALID,
        Error::Authentication => AUTH,
        Error::Locked => 5,
        Error::NotPending => 6,
        Error::NotActive => 7,
        Error::Busy => 8,
        Error::AlreadyExists => 9,
        _ => STORAGE,
    }
}
fn journal_path(root: &Path, initialize: bool) -> Result<PathBuf, u16> {
    let path = root
        .parent()
        .ok_or(STORAGE)?
        .join("transfer-journal.sqlite");
    if !path.try_exists().map_err(|_| STORAGE)? {
        // Missing ledger with any existing profile is a recovery condition, never a fresh start.
        if !initialize
            || std::fs::read_dir(root)
                .map_err(|_| STORAGE)?
                .next()
                .is_some()
        {
            return Err(STORAGE);
        }
        wallet_journal::Journal::open(&path).map_err(|_| STORAGE)?;
    } else {
        wallet_journal::Journal::open_existing(&path).map_err(|_| STORAGE)?;
    }
    Ok(path)
}
fn dispatch(root: &Path, session: &mut Option<Session>, opcode: u8, bytes: &[u8]) -> Reply {
    let mut fields = Fields { data: bytes };
    match opcode {
        30 => {
            fields.done()?;
            if let Some(value) = session.as_mut() {
                value.user_activity().map_err(error)?;
            }
            Ok(Zeroizing::new(Vec::new()))
        }
        31 => {
            let directory=fields.profile(root)?; fields.done()?;
            let address=session.as_mut().ok_or(STATE)?.tron_address(&directory).map_err(error)?;
            Ok(Zeroizing::new(address.into_bytes()))
        }
        32 => {
            let directory=fields.profile(root)?;
            let summary=fields.text(900)?; fields.done()?;
            let quote=chain_tron::mainnet::Quote::parse(summary).map_err(|_|INVALID)?;
            let signed=session.as_mut().ok_or(STATE)?.sign_tron(&directory,&journal_path(root,false)?,quote).map_err(error)?;
            Ok(Zeroizing::new(format!("{}\n{}\n{}",signed.id,signed.hash,signed.summary).into_bytes()))
        }
        33 => {
            let directory=fields.profile(root)?; let id=fields.text(32)?; fields.done()?;
            session.as_mut().ok_or(STATE)?.release_tron(&directory,&journal_path(root,false)?,id).map_err(error)
        }
        34 => {
            let directory=fields.profile(root)?; fields.done()?;
            let address=session.as_mut().ok_or(STATE)?.tron_address(&directory).map_err(error)?;
            let journal=wallet_journal::Journal::open_existing(&journal_path(root,false)?).map_err(|_|STORAGE)?;
            let records=journal.tron_history(&address).map_err(|_|STORAGE)?;
            let entries:Vec<_>=records.into_iter().map(|r|serde_json::json!({"id":r.id,"signature":r.signature,"summary":r.summary,"state":r.state})).collect();
            Ok(Zeroizing::new(serde_json::to_vec(&serde_json::json!({"address":address,"entries":entries})).map_err(|_|STORAGE)?))
        }
        35 => {
            let directory=fields.profile(root)?; let id=fields.text(32)?;
            let signature=fields.text(88)?; let state=fields.text(16)?; fields.done()?;
            let address=session.as_mut().ok_or(STATE)?.tron_address(&directory).map_err(error)?;
            let journal=wallet_journal::Journal::open_existing(&journal_path(root,false)?).map_err(|_|STORAGE)?;
            if journal.tron_record(id).map_err(|_|STORAGE)?.sender!=address {return Err(AUTH);}
            journal.tron_observe(id,signature,state).map_err(|_|STATE)?;
            Ok(Zeroizing::new(Vec::new()))
        }
        25 => {
            let directory=fields.profile(root)?; fields.done()?;
            let address=session.as_mut().ok_or(STATE)?.solana_address(&directory).map_err(error)?;
            Ok(Zeroizing::new(address.into_bytes()))
        }
        26 => {
            let directory=fields.profile(root)?;
            let summary=fields.text(900)?; fields.done()?;
            let quote=chain_solana::Quote::parse(summary).map_err(|_|INVALID)?;
            let signed=session.as_mut().ok_or(STATE)?.sign_solana(&directory,&journal_path(root,false)?,quote).map_err(error)?;
            Ok(Zeroizing::new(format!("{}\n{}\n{}",signed.id,signed.hash,signed.summary).into_bytes()))
        }
        27 => {
            let directory=fields.profile(root)?; let id=fields.text(32)?; fields.done()?;
            session.as_mut().ok_or(STATE)?.release_solana(&directory,&journal_path(root,false)?,id).map_err(error)
        }
        28 => {
            let directory=fields.profile(root)?; fields.done()?;
            let address=session.as_mut().ok_or(STATE)?.solana_address(&directory).map_err(error)?;
            let journal=wallet_journal::Journal::open_existing(&journal_path(root,false)?).map_err(|_|STORAGE)?;
            let records=journal.solana_history(&address).map_err(|_|STORAGE)?;
            let entries:Vec<_>=records.into_iter().map(|r|serde_json::json!({"id":r.id,"signature":r.signature,"summary":r.summary,"state":r.state})).collect();
            Ok(Zeroizing::new(serde_json::to_vec(&serde_json::json!({"address":address,"entries":entries})).map_err(|_|STORAGE)?))
        }
        29 => {
            let directory=fields.profile(root)?; let id=fields.text(32)?;
            let signature=fields.text(88)?; let state=fields.text(16)?; fields.done()?;
            let address=session.as_mut().ok_or(STATE)?.solana_address(&directory).map_err(error)?;
            let journal=wallet_journal::Journal::open_existing(&journal_path(root,false)?).map_err(|_|STORAGE)?;
            if journal.solana_record(id).map_err(|_|STORAGE)?.sender!=address {return Err(AUTH);}
            journal.solana_observe(id,signature,state).map_err(|_|STATE)?;
            Ok(Zeroizing::new(Vec::new()))
        }
        20 | 21 => {
            let directory = fields.profile(root)?;
            let password = fields.take(1024)?;
            fields.done()?;
            *session = None;
            if opcode == 21 {
                let addresses = Session::deletion_addresses(&directory, password).map_err(error)?;
                return Ok(Zeroizing::new(addresses.join("\n").into_bytes()));
            }
            let journal = root.parent().ok_or(STORAGE)?.join("transfer-journal.sqlite");
            Session::delete_wallet(&directory, password, &journal).map_err(error)?;
            Ok(Zeroizing::new(Vec::new()))
        }
        22..=24 => {
            let directory = fields.profile(root)?;
            let index = if opcode == 24 { Some(u32::try_from(fields.number()?).map_err(|_| INVALID)?) } else { None };
            fields.done()?;
            let session = session.as_mut().ok_or(STATE)?;
            if opcode == 22 {
                let (selected, addresses) = session.list_accounts(&directory).map_err(error)?;
                return Ok(Zeroizing::new(serde_json::to_vec(&serde_json::json!({"selected":selected,"addresses":addresses,"limit":wallet_secrets::MAX_ACCOUNTS})).map_err(|_| STORAGE)?));
            }
            let addresses = if opcode == 23 { session.add_account(&directory) }
                else { session.select_account(&directory, index.unwrap()) }.map_err(error)?;
            Ok(Zeroizing::new(format!("{}\n{}",addresses.bsc,addresses.tron).into_bytes()))
        }
        18 => {
            let directory = fields.profile(root)?;
            let password = fields.take(1024)?;
            fields.done()?;
            *session = None;
            let words = Session::export_backup(&directory, password).map_err(error)?;
            Ok(Zeroizing::new(words.as_bytes().to_vec()))
        }
        1 | 2 | 7 => {
            let directory = fields.profile(root)?;
            let password = fields.take(1024)?;
            let words = if opcode == 7 {
                Some(fields.text(256)?)
            } else {
                None
            };
            fields.done()?;
            if !(12..=1024).contains(&password.len()) {
                return Err(INVALID);
            }
            *session = None;
            if opcode == 1 || opcode == 7 {
                journal_path(root, true)?;
            }
            let value = match opcode {
                1 => Session::create_new(&directory, password),
                2 => Session::open(&directory, password),
                7 => Session::restore_new(&directory, password, words.unwrap()),
                _ => unreachable!(),
            }
            .map_err(error)?;
            let phase = value.phase() as u8;
            *session = Some(value);
            Ok(Zeroizing::new(vec![phase]))
        }
        3 => {
            fields.done()?;
            let words = session
                .as_mut()
                .ok_or(STATE)?
                .backup_words()
                .map_err(error)?;
            Ok(Zeroizing::new(words.as_bytes().to_vec()))
        }
        4 => {
            let words = fields.text(256)?;
            fields.done()?;
            let addresses = session
                .as_mut()
                .ok_or(STATE)?
                .confirm_backup_addresses(words)
                .map_err(error)?;
            Ok(Zeroizing::new(
                format!("{}\n{}", addresses.bsc, addresses.tron).into_bytes(),
            ))
        }
        5 => {
            fields.done()?;
            *session = None;
            Ok(Zeroizing::new(Vec::new()))
        }
        6 => {
            let directory = fields.profile(root)?;
            let password = fields.take(1024)?;
            fields.done()?;
            // A premature receive-address request must not destroy pending backup material.
            if session
                .as_ref()
                .is_some_and(|value| value.phase() == wallet_secrets::Phase::PendingBackup)
            {
                return Err(error(Error::NotActive));
            }
            *session = None;
            let mut unlocked = Session::unlock(&directory, password).map_err(error)?;
            let value = unlocked.unlocked_addresses(&directory).map_err(error)?;
            *session = Some(unlocked);
            Ok(Zeroizing::new(
                format!("{}\n{}", value.bsc, value.tron).into_bytes(),
            ))
        }
        8 | 14 | 15 | 16 => {
            let directory = fields.profile(root)?;
            let password = if opcode <= 14 {
                fields.take(1024)?
            } else {
                &[]
            };
            let sender = fields.text(42)?;
            let recipient = fields.text(42)?;
            let amount = fields.text(157)?;
            let nonce = fields.number()?;
            let gas_price_wei = fields.number()?;
            let gas_limit = if opcode == 14 || opcode == 16 {
                fields.number()?
            } else {
                21000
            };
            fields.done()?;
            if opcode <= 14 {
                *session = None;
            }
            let journal = journal_path(root, false)?;
            let request = wallet_secrets::BnbRequest {
                sender,
                recipient,
                amount,
                nonce,
                gas_price_wei,
            };
            let signed = if opcode >= 15 {
                session.as_mut().ok_or(STATE)?.sign_unlocked(
                    &directory,
                    &journal,
                    request,
                    opcode == 16,
                    gas_limit,
                )
            } else if opcode == 14 {
                Session::sign_usdt(&directory, password, &journal, request, gas_limit)
            } else {
                Session::sign_bnb(&directory, password, &journal, request)
            }
            .map_err(error)?;
            Ok(Zeroizing::new(
                format!("{}\n{}\n{}", signed.id, signed.hash, signed.summary).into_bytes(),
            ))
        }
        9 | 17 => {
            let directory = fields.profile(root)?;
            let password = if opcode == 9 { fields.take(1024)? } else { &[] };
            let id = fields.text(32)?;
            fields.done()?;
            let journal = journal_path(root, false)?;
            if opcode == 17 {
                session
                    .as_mut()
                    .ok_or(STATE)?
                    .release_unlocked(&directory, &journal, id)
                    .map_err(error)
            } else {
                *session = None;
                Session::release_bnb(&directory, password, &journal, id).map_err(error)
            }
        }
        19 => {
            let sender = chain_evm::Address::parse(fields.text(42)?).map_err(|_| INVALID)?.checksum();
            let cursor = fields.text(19)?;
            let before = if cursor.is_empty() { i64::MAX } else { cursor.parse::<i64>().map_err(|_| INVALID)? };
            fields.done()?;
            let journal = wallet_journal::Journal::open(&journal_path(root, false)?).map_err(|_| STORAGE)?;
            let mut records = journal.bnb_history(&sender, before).map_err(|_| STORAGE)?;
            let next = if records.len() > 3 { records[2].0.to_string() } else { String::new() };
            records.truncate(3);
            let mut values = Vec::new();
            for (_, record) in records {
                let parts: Vec<_> = record.summary.split('\n').collect();
                if parts.len()!=8 || parts[0]!="BNB" || parts[1]!="56" || parts[2]!=sender { return Err(STORAGE); }
                values.push(serde_json::json!({"h":format!("0x{}",record.hash),"i":0,"b":0,"t":0,
                    "from":parts[2],"to":parts[3],"a":parts[4],"state":record.state}));
            }
            Ok(Zeroizing::new(serde_json::to_vec(&serde_json::json!({"chain_id":56,"asset":"BNB",
                "account":sender,"local":true,"entries":values,"next":next})).map_err(|_| STORAGE)?))
        }
        10 => {
            let sender = fields.text(42)?;
            fields.done()?;
            let sender = chain_evm::Address::parse(sender)
                .map_err(|_| INVALID)?
                .checksum();
            let journal =
                wallet_journal::Journal::open(&journal_path(root, false)?).map_err(|_| STORAGE)?;
            let records = journal.history(&sender).map_err(|_| STORAGE)?;
            let values: Vec<_> = records.into_iter().map(|value| serde_json::json!({"id":value.id,"hash":value.hash,"summary":value.summary,"state":value.state,"attempts":value.attempts})).collect();
            Ok(Zeroizing::new(
                serde_json::to_vec(&serde_json::json!({"entries":values})).map_err(|_| STORAGE)?,
            ))
        }
        11 => {
            let id = fields.text(32)?;
            let hash = fields.text(64)?;
            let state = fields.text(16)?;
            let height = fields.text(20)?;
            let block_hash = fields.text(66)?;
            let finalized = fields.text(20)?;
            fields.done()?;
            let journal =
                wallet_journal::Journal::open(&journal_path(root, false)?).map_err(|_| STORAGE)?;
            journal
                .observe(id, hash, state, height, block_hash, finalized)
                .map_err(|_| STATE)?;
            Ok(Zeroizing::new(Vec::new()))
        }
        12 => {
            let id = fields.text(32)?;
            fields.done()?;
            let journal =
                wallet_journal::Journal::open(&journal_path(root, false)?).map_err(|_| STORAGE)?;
            journal.cancel_prepared(id).map_err(|_| STATE)?;
            Ok(Zeroizing::new(Vec::new()))
        }
        13 => {
            fields.done()?;
            let mut names = Vec::new();
            for entry in std::fs::read_dir(root).map_err(|_| STORAGE)? {
                let entry = entry.map_err(|_| STORAGE)?;
                if !entry.file_type().map_err(|_| STORAGE)?.is_dir() {
                    continue;
                }
                let name = entry.file_name();
                let Some(name) = name.to_str().and_then(|name| name.strip_prefix("wallet-")) else {
                    continue;
                };
                if name.len() <= 48
                    && !name.is_empty()
                    && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                {
                    names.push(name.to_owned());
                }
            }
            names.sort();
            names.truncate(16);
            Ok(Zeroizing::new(
                serde_json::to_vec(&serde_json::json!({"profiles":names})).map_err(|_| STORAGE)?,
            ))
        }
        _ => Err(INVALID),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deletion_authenticates_and_forgets_only_selected_account() {
        fn fields(values: &[&[u8]]) -> Vec<u8> {
            let mut result = Vec::new();
            for value in values {
                result.extend((value.len() as u16).to_le_bytes());
                result.extend_from_slice(value);
            }
            result
        }
        let base = std::env::temp_dir().join(format!("w3-delete-{}", std::process::id()));
        std::fs::create_dir(&base).unwrap();
        let root = base.join("profiles");
        std::fs::create_dir(&root).unwrap();
        let password = b"PUBLIC TEST PASSWORD ONLY";
        let mut session = None;
        let request = fields(&[b"test", password]);
        dispatch(&root, &mut session, 1, &request).unwrap();
        let words = dispatch(&root, &mut session, 3, &[]).unwrap();
        dispatch(&root, &mut session, 4, &fields(&[&words])).unwrap();
        let child = dispatch(&root, &mut session, 23, &fields(&[b"test"])).unwrap();
        let child = std::str::from_utf8(&child).unwrap().lines().next().unwrap();
        let listing = dispatch(&root, &mut session, 22, &fields(&[b"test"])).unwrap();
        let listing: serde_json::Value = serde_json::from_slice(&listing).unwrap();
        assert_eq!(listing["selected"], 1);
        assert_eq!(listing["addresses"][1], child);
        assert_eq!(dispatch(&root, &mut session, 24, &fields(&[b"other", b"0"])).unwrap_err(), AUTH);
        assert!(dispatch(&root, &mut session, 24, &fields(&[b"test", b"32"])).is_err());
        dispatch(&root, &mut session, 24, &fields(&[b"test", b"0"])).unwrap();
        let ledger = base.join("transfer-journal.sqlite");
        let address = dispatch(&root, &mut session, 21, &request).unwrap();
        let addresses = std::str::from_utf8(&address).unwrap();
        assert_eq!(addresses.lines().count(), 2);
        let address = addresses.lines().next().unwrap();
        {
            let journal = wallet_journal::Journal::open_existing(&ledger).unwrap();
            for (id, sender) in [("11".repeat(16), address), ("22".repeat(16), "other-account"), ("33".repeat(16), child)] {
                journal.reserve(wallet_journal::Intent { id: &id, network: 56, sender,
                    nonce: "0", summary: "PUBLIC TEST" }).unwrap();
            }
        }
        let profile = root.join("wallet-test");
        let vault = std::fs::read(profile.join("active.vault")).unwrap();
        let other = root.join("wallet-other");
        drop(Session::create_new(&other, password).unwrap());
        assert_eq!(dispatch(&root, &mut session, 20,
            &fields(&[b"test", b"WRONG TEST PASSWORD"])).unwrap_err(), AUTH);
        assert!(session.is_none());
        assert_eq!(std::fs::read(profile.join("active.vault")).unwrap(), vault);
        assert_eq!(wallet_journal::Journal::open_existing(&ledger).unwrap().history(address).unwrap().len(), 1);
        let held = Session::open(&profile, password).unwrap();
        assert_eq!(dispatch(&root, &mut session, 20, &request).unwrap_err(), 8);
        drop(held);
        let foreign = profile.join("unexpected.txt");
        std::fs::write(&foreign, b"keep").unwrap();
        assert_eq!(dispatch(&root, &mut session, 20, &request).unwrap_err(), STORAGE);
        assert_eq!(std::fs::read(profile.join("active.vault")).unwrap(), vault);
        std::fs::remove_file(foreign).unwrap();
        dispatch(&root, &mut session, 6, &request).unwrap();
        dispatch(&root, &mut session, 20, &request).unwrap();
        assert!(session.is_none());
        assert!(!profile.exists());
        assert!(other.join("pending.vault").exists());
        {
            let journal = wallet_journal::Journal::open_existing(&ledger).unwrap();
            assert!(journal.history(address).unwrap().is_empty());
            assert!(journal.history(child).unwrap().is_empty());
            assert_eq!(journal.history("other-account").unwrap().len(), 1);
        }
        let listed = dispatch(&root, &mut session, 13, &[]).unwrap();
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&listed).unwrap()["profiles"],
            serde_json::json!(["other"]));
        // Restoration after deletion remains possible, including reuse of the name.
        dispatch(&root, &mut session, 7, &fields(&[b"test", password, &words])).unwrap();
        dispatch(&root, &mut session, 20, &request).unwrap();
        dispatch(&root, &mut session, 20, &fields(&[b"other", password])).unwrap();
        assert!(wallet_journal::Journal::open_existing(&ledger).unwrap().history(address).unwrap().is_empty());
        std::fs::remove_file(ledger).unwrap();
        std::fs::remove_dir(root).unwrap();
        std::fs::remove_dir(base).unwrap();
    }
    #[test]
    fn missing_ledger_cannot_reset_existing_wallet_history() {
        let base = std::env::temp_dir().join(format!("w3-ledger-guard-{}", std::process::id()));
        std::fs::create_dir(&base).unwrap();
        let root = base.join("profiles");
        std::fs::create_dir(&root).unwrap();
        assert_eq!(journal_path(&root, false).unwrap_err(), STORAGE);
        let profile = root.join("wallet-public-test");
        std::fs::create_dir(&profile).unwrap();
        assert_eq!(journal_path(&root, true).unwrap_err(), STORAGE);
        assert!(!base.join("transfer-journal.sqlite").exists());
        std::fs::remove_dir(&profile).unwrap();
        let ledger = journal_path(&root, true).unwrap();
        assert!(ledger.exists());
        assert!(journal_path(&root, false).is_ok());
        std::fs::write(&ledger, []).unwrap();
        assert_eq!(journal_path(&root, true).unwrap_err(), STORAGE);
        assert_eq!(journal_path(&root, false).unwrap_err(), STORAGE);
        std::fs::remove_file(ledger).unwrap();
        std::fs::remove_dir(root).unwrap();
        std::fs::remove_dir(base).unwrap();
    }
    #[test]
    fn malformed_frames_close_without_reply() {
        for data in [
            vec![0, 0, 0, 0],
            vec![255, 255, 255, 255],
            vec![10, 0, 0, 0, 2, 3, 0, 0, 0, 0, 0, 0, 0, 0],
        ] {
            let mut output = Vec::new();
            assert!(
                exchange(
                    Path::new("unused"),
                    &Mutex::new(None),
                    data.as_slice(),
                    &mut output
                )
                .is_err()
            );
            assert!(output.is_empty());
        }
    }
    #[test]
    fn rejects_traversal_unknown_and_trailing_without_side_effects() {
        let root = Path::new("unused");
        let mut session = None;
        for name in ["..", "a/b", "a\\b", "C:", "", "has space"] {
            let mut bytes = (name.len() as u16).to_le_bytes().to_vec();
            bytes.extend(name.as_bytes());
            assert_eq!(
                dispatch(root, &mut session, 1, &bytes).unwrap_err(),
                INVALID
            );
            assert_eq!(dispatch(root, &mut session, 20, &bytes).unwrap_err(), INVALID);
        }
        assert_eq!(dispatch(root, &mut session, 255, &[]).unwrap_err(), INVALID);
        assert_eq!(dispatch(root, &mut session, 5, &[1]).unwrap_err(), INVALID);
        assert!(session.is_none());
    }
}
