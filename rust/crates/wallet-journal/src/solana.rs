//! Additive Solana ledger in the existing database; EVM tables/version remain unchanged.
use crate::{Error, Journal, Result};

pub struct Record {
    pub id: String,
    pub sender: String,
    pub signature: String,
    pub summary: String,
    pub ciphertext: Vec<u8>,
    pub state: String,
}
impl Journal {
    fn solana_schema(&self) -> Result<()> {
        self.db.exec("CREATE TABLE IF NOT EXISTS solana_transfers(id TEXT PRIMARY KEY, sender TEXT NOT NULL, signature TEXT NOT NULL UNIQUE, summary TEXT NOT NULL, ciphertext BLOB NOT NULL, state TEXT NOT NULL CHECK(state IN ('signed','broadcasting','unknown','finalized','reverted','expired')), attempts INTEGER NOT NULL DEFAULT 0)")?;
        self.db.exec("CREATE UNIQUE INDEX IF NOT EXISTS solana_pending ON solana_transfers(sender) WHERE state NOT IN ('finalized','reverted','expired')")
    }
    pub fn solana_signed(&self, record: &Record) -> Result<()> {
        self.solana_schema()?;
        if record.id.len() != 32
            || !record.id.bytes().all(|b| b.is_ascii_hexdigit())
            || !(32..=44).contains(&record.sender.len())
            || !(64..=88).contains(&record.signature.len())
            || record.summary.is_empty()
            || record.summary.len() > 900
            || record.ciphertext.len() < 40
            || record.ciphertext.len() > 600
            || record.state != "signed"
        {
            return Err(Error::Invalid);
        }
        let mut stmt=self.db.prepare("INSERT INTO solana_transfers(id,sender,signature,summary,ciphertext,state) VALUES(?1,?2,?3,?4,?5,'signed')")?;
        stmt.bind_text(1, &record.id)?;
        stmt.bind_text(2, &record.sender)?;
        stmt.bind_text(3, &record.signature)?;
        stmt.bind_text(4, &record.summary)?;
        stmt.bind_blob(5, &record.ciphertext)?;
        stmt.step()?;
        Ok(())
    }
    pub fn solana_record(&self, id: &str) -> Result<Record> {
        self.solana_schema()?;
        let mut stmt = self.db.prepare(
            "SELECT id,sender,signature,summary,ciphertext,state FROM solana_transfers WHERE id=?1",
        )?;
        stmt.bind_text(1, id)?;
        if !stmt.step()? {
            return Err(Error::State);
        }
        read(&stmt)
    }
    pub fn solana_history(&self, sender: &str) -> Result<Vec<Record>> {
        self.solana_schema()?;
        // Pending first so old unresolved payments cannot disappear behind terminal rows.
        let mut stmt=self.db.prepare("SELECT id,sender,signature,summary,ciphertext,state FROM solana_transfers WHERE sender=?1 ORDER BY (state NOT IN ('finalized','reverted','expired')) DESC,rowid DESC LIMIT 2")?;
        stmt.bind_text(1, sender)?;
        let mut result = Vec::new();
        while stmt.step()? {
            result.push(read(&stmt)?);
        }
        Ok(result)
    }
    pub fn solana_begin_broadcast(&self, id: &str) -> Result<()> {
        self.solana_schema()?;
        let mut stmt=self.db.prepare("UPDATE solana_transfers SET state='broadcasting',attempts=attempts+1 WHERE id=?1 AND state IN ('signed','broadcasting','unknown') AND attempts<100")?;
        stmt.bind_text(1, id)?;
        stmt.step()?;
        if self.db.changes() != 1 {
            return Err(Error::State);
        }
        Ok(())
    }
    /// Caller must authenticate the original signature and validate mainnet RPC evidence.
    pub fn solana_observe(&self, id: &str, signature: &str, state: &str) -> Result<()> {
        if !matches!(state, "unknown" | "finalized" | "reverted" | "expired") {
            return Err(Error::Invalid);
        }
        let record = self.solana_record(id)?;
        if record.signature != signature {
            return Err(Error::State);
        }
        if matches!(record.state.as_str(), "finalized" | "reverted" | "expired") {
            return if record.state == state {
                Ok(())
            } else {
                Err(Error::State)
            };
        }
        let mut stmt=self.db.prepare("UPDATE solana_transfers SET state=?3 WHERE id=?1 AND signature=?2 AND state IN ('signed','broadcasting','unknown')")?;
        stmt.bind_text(1, id)?;
        stmt.bind_text(2, signature)?;
        stmt.bind_text(3, state)?;
        stmt.step()?;
        if self.db.changes() != 1 {
            return Err(Error::State);
        }
        Ok(())
    }
    pub fn solana_forget(&self, sender: &str) -> Result<()> {
        self.solana_schema()?;
        self.db.exec("PRAGMA secure_delete=ON")?;
        let mut stmt = self
            .db
            .prepare("DELETE FROM solana_transfers WHERE sender=?1")?;
        stmt.bind_text(1, sender)?;
        stmt.step()?;
        drop(stmt);
        let mut checkpoint = self.db.prepare("PRAGMA wal_checkpoint(TRUNCATE)")?;
        if !checkpoint.step()? || checkpoint.integer(0) != 0 {
            return Err(Error::State);
        }
        Ok(())
    }
}
fn read(stmt: &crate::sqlite::Stmt<'_>) -> Result<Record> {
    Ok(Record {
        id: stmt.text(0)?,
        sender: stmt.text(1)?,
        signature: stmt.text(2)?,
        summary: stmt.text(3)?,
        ciphertext: stmt.blob(4)?,
        state: stmt.text(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restart_pending_and_terminal_transitions() {
        let path = std::env::temp_dir().join(format!(
            "solana-journal-{}-{}.sqlite",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut record = Record {
            id: "01".repeat(16),
            sender: "A".repeat(32),
            signature: "B".repeat(88),
            summary: "SOL mainnet fixture".into(),
            ciphertext: vec![7; 100],
            state: "signed".into(),
        };
        let journal = Journal::open(&path).unwrap();
        journal.solana_signed(&record).unwrap();
        drop(journal);
        let journal = Journal::open_existing(&path).unwrap();
        assert_eq!(
            journal.solana_record(&record.id).unwrap().ciphertext,
            record.ciphertext
        );
        let first = record.id.clone();
        let signature = record.signature.clone();
        record.id = "02".repeat(16);
        record.signature = "C".repeat(88);
        assert!(journal.solana_signed(&record).is_err());
        journal.solana_begin_broadcast(&first).unwrap();
        journal
            .solana_observe(&first, &signature, "unknown")
            .unwrap();
        assert!(journal.solana_signed(&record).is_err());
        assert!(
            journal
                .solana_observe(&first, &record.signature, "finalized")
                .is_err()
        );
        journal
            .solana_observe(&first, &signature, "finalized")
            .unwrap();
        assert!(
            journal
                .solana_observe(&first, &signature, "expired")
                .is_err()
        );
        assert!(journal.solana_begin_broadcast(&first).is_err());
        journal.solana_signed(&record).unwrap();
        assert_eq!(
            journal.solana_history(&record.sender).unwrap()[0].id,
            record.id
        );
        journal.solana_forget(&record.sender).unwrap();
        assert!(journal.solana_history(&record.sender).unwrap().is_empty());
        drop(journal);
        std::fs::remove_file(path).unwrap();
    }
}
