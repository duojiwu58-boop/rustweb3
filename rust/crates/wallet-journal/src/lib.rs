//! Durable transfer intent ledger. No plaintext signed payload or wallet secrets.
use std::path::Path;
mod sqlite;
pub mod solana;
pub mod tron;
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Database(i32),
    Invalid,
    State,
}
pub type Result<T> = std::result::Result<T, Error>;
pub struct Journal {
    db: sqlite::Db,
}
pub struct Intent<'a> {
    pub id: &'a str,
    pub network: i64,
    pub sender: &'a str,
    pub nonce: &'a str,
    pub summary: &'a str,
}
#[derive(Debug)]
pub struct SignedRecord {
    pub hash: String,
    pub summary: String,
    pub ciphertext: Vec<u8>,
    pub attempts: i64,
    pub state: String,
}
pub struct HistoryRecord {
    pub id: String,
    pub hash: String,
    pub summary: String,
    pub state: String,
    pub attempts: i64,
}
impl Journal {
    /// Existing wallet data must never silently initialize a replaced or truncated ledger.
    pub fn open_existing(path: &Path) -> Result<Self> {
        if std::fs::metadata(path).map_err(|_| Error::State)?.len() == 0 {
            return Err(Error::State);
        }
        let db = sqlite::Db::open(path)?;
        if db.scalar("PRAGMA user_version")? != "2" {
            return Err(Error::State);
        }
        drop(db);
        Self::open(path)
    }
    pub fn open(path: &Path) -> Result<Self> {
        let db = sqlite::Db::open(path)?;
        if db.scalar("PRAGMA journal_mode=WAL")? != "wal" {
            return Err(Error::State);
        }
        db.exec("PRAGMA synchronous=FULL")?;
        if db.scalar("PRAGMA synchronous")? != "2" {
            return Err(Error::State);
        }
        db.exec("PRAGMA trusted_schema=OFF")?;
        db.exec("PRAGMA temp_store=MEMORY")?;
        let version = db.scalar("PRAGMA user_version")?;
        if version != "0" && version != "2" {
            return Err(Error::State);
        }
        db.exec("CREATE TABLE IF NOT EXISTS transfers(id TEXT PRIMARY KEY, network INTEGER NOT NULL, sender TEXT NOT NULL, nonce TEXT NOT NULL, summary TEXT NOT NULL, state TEXT NOT NULL CHECK(state IN ('prepared','signed','broadcasting','unknown','included','finalized','reverted','cancelled')), hash TEXT UNIQUE, ciphertext BLOB, attempts INTEGER NOT NULL DEFAULT 0)")?;
        db.exec("CREATE UNIQUE INDEX IF NOT EXISTS reserved_nonce ON transfers(network,sender,nonce) WHERE state != 'cancelled'")?;
        // Conservative first release: one unresolved transfer per chain/account, across profiles.
        db.exec("CREATE UNIQUE INDEX IF NOT EXISTS unresolved_sender ON transfers(network,sender) WHERE state NOT IN ('finalized','reverted','cancelled')")?;
        db.exec("CREATE TABLE IF NOT EXISTS observations(id TEXT NOT NULL, hash TEXT NOT NULL, state TEXT NOT NULL, block_number TEXT NOT NULL, block_hash TEXT NOT NULL, finalized_height TEXT NOT NULL)")?;
        db.exec("PRAGMA user_version=2")?;
        Ok(Self { db })
    }
    pub fn runtime_version(&self) -> Result<String> {
        self.db.scalar("SELECT sqlite_version()")
    }
    /// Explicitly forget a wallet locally, including unresolved signed payloads.
    /// This does not cancel or otherwise alter any transaction on the network.
    pub fn forget_wallet(&self, bsc: &str, tron: &str) -> Result<()> {
        if bsc.is_empty() || tron.is_empty() { return Err(Error::Invalid); }
        self.db.exec("PRAGMA secure_delete=ON")?;
        self.db.exec("BEGIN IMMEDIATE")?;
        let result = (|| {
            for sql in [
                "DELETE FROM observations WHERE id IN (SELECT id FROM transfers WHERE (network=56 AND sender=?1) OR (network=728126428 AND sender=?2))",
                "DELETE FROM transfers WHERE (network=56 AND sender=?1) OR (network=728126428 AND sender=?2)",
            ] {
                let mut statement = self.db.prepare(sql)?;
                statement.bind_text(1, bsc)?;
                statement.bind_text(2, tron)?;
                statement.step()?;
            }
            self.db.exec("COMMIT")
        })();
        if result.is_err() { let _ = self.db.exec("ROLLBACK"); }
        result?;
        // Drop old WAL pages too; a busy checkpoint is a retryable cleanup failure.
        let mut checkpoint = self.db.prepare("PRAGMA wal_checkpoint(TRUNCATE)")?;
        if !checkpoint.step()? || checkpoint.integer(0) != 0 { return Err(Error::State); }
        Ok(())
    }
    pub fn reserve(&self, intent: Intent<'_>) -> Result<()> {
        if intent.id.len() != 32
            || !intent.id.bytes().all(|b| b.is_ascii_hexdigit())
            || !matches!(intent.network, 56 | 728126428)
            || intent.sender.is_empty()
            || intent.nonce.is_empty()
            || intent.summary.is_empty()
        {
            return Err(Error::Invalid);
        }
        let mut statement = self.db.prepare("INSERT INTO transfers(id,network,sender,nonce,summary,state) VALUES(?1,?2,?3,?4,?5,'prepared')")?;
        statement.bind_text(1, intent.id)?;
        statement.bind_integer(2, intent.network)?;
        statement.bind_text(3, intent.sender)?;
        statement.bind_text(4, intent.nonce)?;
        statement.bind_text(5, intent.summary)?;
        statement.step()?;
        Ok(())
    }
    /// Commit encrypted bytes and the exact transaction hash before giving the caller success.
    pub fn signed(&self, id: &str, hash: &str, ciphertext: &[u8]) -> Result<()> {
        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) || ciphertext.len() < 40
        {
            return Err(Error::Invalid);
        }
        let mut statement = self.db.prepare("UPDATE transfers SET state='signed',hash=?2,ciphertext=?3 WHERE id=?1 AND state='prepared' AND hash IS NULL AND ciphertext IS NULL")?;
        statement.bind_text(1, id)?;
        statement.bind_text(2, hash)?;
        statement.bind_blob(3, ciphertext)?;
        statement.step()?;
        if self.db.changes() != 1 {
            return Err(Error::State);
        }
        Ok(())
    }
    pub fn record(&self, id: &str) -> Result<SignedRecord> {
        let mut statement = self.db.prepare("SELECT hash,summary,ciphertext,attempts,state FROM transfers WHERE id=?1 AND state != 'prepared'")?;
        statement.bind_text(1, id)?;
        if !statement.step()? {
            return Err(Error::State);
        }
        Ok(SignedRecord {
            hash: statement.text(0)?,
            summary: statement.text(1)?,
            ciphertext: statement.blob(2)?,
            attempts: statement.integer(3),
            state: statement.text(4)?,
        })
    }
    /// Must succeed before any decrypted broadcast bytes leave the trusted process.
    pub fn begin_broadcast(&self, id: &str) -> Result<()> {
        let mut statement = self.db.prepare("UPDATE transfers SET state='broadcasting', attempts=attempts+1 WHERE id=?1 AND state IN ('signed','broadcasting','unknown') AND ciphertext IS NOT NULL AND attempts < 100")?;
        statement.bind_text(1, id)?;
        statement.step()?;
        if self.db.changes() != 1 {
            return Err(Error::State);
        }
        Ok(())
    }
    /// A transport error retains the same hash/payload and holds the account's nonce slot.
    pub fn unknown(&self, id: &str) -> Result<()> {
        let mut statement = self
            .db
            .prepare("UPDATE transfers SET state='unknown' WHERE id=?1 AND state='broadcasting'")?;
        statement.bind_text(1, id)?;
        statement.step()?;
        if self.db.changes() != 1 {
            return Err(Error::State);
        }
        Ok(())
    }

    pub fn history(&self, sender: &str) -> Result<Vec<HistoryRecord>> {
        let mut statement = self.db.prepare("SELECT id,COALESCE(hash,''),summary,state,attempts FROM transfers WHERE network=56 AND sender=?1 ORDER BY rowid DESC LIMIT 3")?;
        statement.bind_text(1, sender)?;
        let mut records = Vec::new();
        while statement.step()? {
            records.push(HistoryRecord {
                id: statement.text(0)?,
                hash: statement.text(1)?,
                summary: statement.text(2)?,
                state: statement.text(3)?,
                attempts: statement.integer(4),
            });
        }
        Ok(records)
    }

    /// Stable descending pagination, scoped to locally signed BNB transfers.
    pub fn bnb_history(&self, sender: &str, before: i64) -> Result<Vec<(i64, HistoryRecord)>> {
        if before <= 0 { return Err(Error::Invalid); }
        let mut statement = self.db.prepare("SELECT rowid,id,hash,summary,state,attempts FROM transfers WHERE network=56 AND sender=?1 AND rowid<?2 AND substr(summary,1,4)=('BNB'||char(10)) AND hash IS NOT NULL ORDER BY rowid DESC LIMIT 4")?;
        statement.bind_text(1, sender)?;
        statement.bind_integer(2, before)?;
        let mut records = Vec::new();
        while statement.step()? {
            records.push((statement.integer(0), HistoryRecord {
                id: statement.text(1)?, hash: statement.text(2)?, summary: statement.text(3)?,
                state: statement.text(4)?, attempts: statement.integer(5),
            }));
        }
        Ok(records)
    }

    /// Only an intent whose bytes have never been released can be cancelled.
    pub fn cancel_prepared(&self, id: &str) -> Result<()> {
        let mut statement = self.db.prepare("UPDATE transfers SET state='cancelled' WHERE id=?1 AND state='prepared' AND hash IS NULL AND ciphertext IS NULL")?;
        statement.bind_text(1, id)?;
        statement.step()?;
        if self.db.changes() != 1 {
            return Err(Error::State);
        }
        Ok(())
    }

    /// Caller supplies checked public RPC evidence, not an approval to replace a payment.
    pub fn observe(
        &self,
        id: &str,
        hash: &str,
        state: &str,
        height: &str,
        block_hash: &str,
        final_height: &str,
    ) -> Result<()> {
        if !matches!(state, "included" | "finalized" | "reverted")
            || hash.len() != 64
            || block_hash.len() != 66
            || !block_hash.starts_with("0x")
            || !block_hash[2..].bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(Error::Invalid);
        }
        let block: u64 = height.parse().map_err(|_| Error::Invalid)?;
        let final_block: u64 = final_height.parse().map_err(|_| Error::Invalid)?;
        if state != "included" && final_block < block {
            return Err(Error::Invalid);
        }
        let existing = self.record(id)?;
        if existing.hash != hash {
            return Err(Error::State);
        }
        if matches!(existing.state.as_str(), "finalized" | "reverted") {
            return if existing.state == state {
                Ok(())
            } else {
                Err(Error::State)
            };
        }
        self.db.exec("BEGIN IMMEDIATE")?;
        let result = (|| {
            let mut update = self.db.prepare("UPDATE transfers SET state=?3 WHERE id=?1 AND hash=?2 AND state IN ('signed','broadcasting','unknown','included')")?;
            update.bind_text(1, id)?;
            update.bind_text(2, hash)?;
            update.bind_text(3, state)?;
            update.step()?;
            if self.db.changes() != 1 {
                return Err(Error::State);
            }
            let mut insert = self
                .db
                .prepare("INSERT INTO observations VALUES(?1,?2,?3,?4,?5,?6)")?;
            for (index, value) in [id, hash, state, height, block_hash, final_height]
                .iter()
                .enumerate()
            {
                insert.bind_text(index as i32 + 1, value)?;
            }
            insert.step()?;
            self.db.exec("COMMIT")
        })();
        if result.is_err() {
            let _ = self.db.exec("ROLLBACK");
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn forget_removes_payload_observations_and_pending_but_keeps_other_accounts() {
        let path = std::env::temp_dir().join(format!("w3-forget-{}.sqlite", std::process::id()));
        {
            let journal = Journal::open(&path).unwrap();
            for n in 1..=4 {
                let id = format!("{n:032x}");
                let sender = if n == 4 { "OTHER" } else { "SELECTED" };
                journal.reserve(Intent { id: &id, network: 56, sender, nonce: &n.to_string(), summary: "PUBLIC TEST" }).unwrap();
                if n != 3 {
                    let hash = format!("{n:064x}");
                    journal.signed(&id, &hash, &[5; 80]).unwrap();
                    journal.observe(&id, &hash, "finalized", "1", &format!("0x{}", "ab".repeat(32)), "1").unwrap();
                }
            }
            journal.forget_wallet("SELECTED", "TRON-SELECTED").unwrap();
            journal.forget_wallet("SELECTED", "TRON-SELECTED").unwrap();
            assert!(journal.history("SELECTED").unwrap().is_empty());
            assert_eq!(journal.history("OTHER").unwrap().len(), 1);
            assert_eq!(journal.db.scalar("SELECT count(*) FROM observations").unwrap(), "1");
            assert_eq!(journal.db.scalar("SELECT count(*) FROM transfers").unwrap(), "1");
            assert!(journal.record(&format!("{:032x}", 1)).is_err());
        }
        assert!(Journal::open_existing(&path).unwrap().history("SELECTED").unwrap().is_empty());
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn bnb_pages_survive_restart_and_exclude_other_assets_accounts_and_unsigned() {
        let path=std::env::temp_dir().join(format!("web3rust-bnb-pages-{}.sqlite",std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        {
            let journal=Journal::open(&path).unwrap();
            for n in 1..=8 {
                let id=format!("{n:032x}");
                let sender=if n==7 {"OTHER"} else {"SENDER"};
                let summary=if n==6 {"USDT\n56\nSENDER"} else {"BNB\n56\nSENDER"};
                journal.reserve(Intent {id:&id,network:56,sender,nonce:&n.to_string(),summary}).unwrap();
                if n==8 { break; }
                let hash=format!("{n:064x}");
                journal.signed(&id,&hash,&[0;80]).unwrap();
                journal.observe(&id,&hash,if n==4 {"reverted"} else {"finalized"},"1",&format!("0x{}","ab".repeat(32)),"1").unwrap();
            }
        }
        {
            let journal=Journal::open_existing(&path).unwrap();
            let first=journal.bnb_history("SENDER",i64::MAX).unwrap();
            assert_eq!(first.iter().map(|r|r.0).collect::<Vec<_>>(),vec![5,4,3,2]);
            assert_eq!(first[1].1.state,"reverted");
            let next=journal.bnb_history("SENDER",first[2].0).unwrap();
            assert_eq!(next.iter().map(|r|r.0).collect::<Vec<_>>(),vec![2,1]);
            assert!(journal.bnb_history("ABSENT",i64::MAX).unwrap().is_empty());
            assert!(journal.bnb_history("SENDER",0).is_err());
        }
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn commit_restart_and_unknown_preserve_identity_and_nonce_hold() {
        let path = std::env::temp_dir().join(format!(
            "web3rust-journal-public-{}-{}.sqlite",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let id = "00112233445566778899aabbccddeeff";
        {
            let journal = Journal::open(&path).unwrap();
            assert!(!journal.runtime_version().unwrap().is_empty());
            journal
                .reserve(Intent {
                    id,
                    network: 56,
                    sender: "PUBLIC-SENDER",
                    nonce: "7",
                    summary: "PUBLIC-INTENT",
                })
                .unwrap();
            assert!(journal.begin_broadcast(id).is_err());
            assert!(journal.record(id).is_err());
            journal.signed(id, &"ab".repeat(32), &[7; 80]).unwrap();
            assert!(journal.signed(id, &"cd".repeat(32), &[8; 80]).is_err());
        }
        {
            let journal = Journal::open(&path).unwrap();
            journal.begin_broadcast(id).unwrap();
            journal.unknown(id).unwrap();
            assert!(
                journal
                    .reserve(Intent {
                        id: "10112233445566778899aabbccddeeff",
                        network: 56,
                        sender: "PUBLIC-SENDER",
                        nonce: "8",
                        summary: "SECOND PAYMENT"
                    })
                    .is_err()
            );
            let record = journal.record(id).unwrap();
            assert_eq!(record.state, "unknown");
            assert_eq!(record.hash, "ab".repeat(32));
            assert_eq!(record.ciphertext, vec![7; 80]);
            assert_eq!(record.attempts, 1);
            journal.begin_broadcast(id).unwrap();
        }
        {
            let journal = Journal::open(&path).unwrap();
            let record = journal.record(id).unwrap();
            assert_eq!(record.state, "broadcasting");
            assert_eq!(record.attempts, 2);
            assert!(journal.cancel_prepared(id).is_err());
            assert!(
                journal
                    .observe(
                        id,
                        &"cd".repeat(32),
                        "finalized",
                        "1",
                        &format!("0x{}", "11".repeat(32)),
                        "1"
                    )
                    .is_err()
            );
            assert!(
                journal
                    .observe(
                        id,
                        &"ab".repeat(32),
                        "finalized",
                        "2",
                        &format!("0x{}", "11".repeat(32)),
                        "1"
                    )
                    .is_err()
            );
            assert_eq!(journal.record(id).unwrap().state, "broadcasting");
            journal
                .observe(
                    id,
                    &"ab".repeat(32),
                    "included",
                    "2",
                    &format!("0x{}", "11".repeat(32)),
                    "1",
                )
                .unwrap();
            journal
                .observe(
                    id,
                    &"ab".repeat(32),
                    "finalized",
                    "2",
                    &format!("0x{}", "11".repeat(32)),
                    "2",
                )
                .unwrap();
            journal
                .observe(
                    id,
                    &"ab".repeat(32),
                    "finalized",
                    "2",
                    &format!("0x{}", "11".repeat(32)),
                    "2",
                )
                .unwrap();
            let next = "20112233445566778899aabbccddeeff";
            journal
                .reserve(Intent {
                    id: next,
                    network: 56,
                    sender: "PUBLIC-SENDER",
                    nonce: "8",
                    summary: "PUBLIC NEXT",
                })
                .unwrap();
            journal.cancel_prepared(next).unwrap();
            journal
                .reserve(Intent {
                    id: "30112233445566778899aabbccddeeff",
                    network: 56,
                    sender: "PUBLIC-SENDER",
                    nonce: "8",
                    summary: "PUBLIC NEXT",
                })
                .unwrap();
        }
        std::fs::remove_file(&path).unwrap();
        for suffix in ["-wal", "-shm"] {
            let file = std::path::PathBuf::from(format!("{}{suffix}", path.display()));
            if file.exists() {
                std::fs::remove_file(file).unwrap();
            }
        }
    }
}
