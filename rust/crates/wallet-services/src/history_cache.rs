//! Disposable public display cache. Never used by signing or payment recovery.
use super::*;
use std::path::Path;
type Scanner<'a> = dyn FnMut(&str, &str, Option<u64>) -> io::Result<Value> + 'a;

#[derive(Default)]
struct Snapshot {
    high: Option<u64>,
    pending: String,
    older: String,
    entries: Vec<Value>,
}
fn invalid() -> io::Error {
    io::Error::other("invalid history cache")
}
fn cursor(value: &Value) -> io::Result<String> {
    let s = value.as_str().ok_or_else(invalid)?;
    if !history::valid_cursor(s) {
        return Err(invalid());
    }
    Ok(s.to_owned())
}
fn entry_valid(value: &Value, account: &str) -> bool {
    let valid = || -> io::Result<()> {
        {
            let s = value["h"].as_str().ok_or_else(invalid)?;
            if s.len() != 66
                || !s.starts_with("0x")
                || !s[2..].bytes().all(|c| c.is_ascii_hexdigit())
            {
                return Err(invalid());
            }
        }
        let from = address(value["from"].as_str().ok_or_else(invalid)?)?;
        let to = address(value["to"].as_str().ok_or_else(invalid)?)?;
        if from != account && to != account {
            return Err(invalid());
        }
        for field in ["i", "b", "t"] {
            if value[field]
                .as_u64()
                .filter(|v| *v <= i64::MAX as u64)
                .is_none()
            {
                return Err(invalid());
            }
        }
        if value["t"].as_u64().unwrap() > 253_402_300_799 {
            return Err(invalid());
        }
        let text = value["a"].as_str().ok_or_else(invalid)?;
        let amount = Amount::parse_units(text, 18).map_err(|_| invalid())?;
        if amount_text(amount, 18)? != text {
            return Err(invalid());
        }
        Ok(())
    };
    valid().is_ok()
}
impl Snapshot {
    fn parse(value: Value, account: &str) -> io::Result<Self> {
        if value["version"] != 1 || value["account"] != account || value["chain"] != 56 {
            return Err(invalid());
        }
        let high = if value["high"].is_null() {
            None
        } else {
            Some(
                value["high"]
                    .as_u64()
                    .filter(|n| *n < i64::MAX as u64)
                    .ok_or_else(invalid)?,
            )
        };
        let entries = value["entries"]
            .as_array()
            .filter(|a| a.len() <= 300)
            .ok_or_else(invalid)?
            .clone();
        if !entries.iter().all(|v| entry_valid(v, account)) {
            return Err(invalid());
        }
        Ok(Self {
            high,
            pending: cursor(&value["pending"])?,
            older: cursor(&value["older"])?,
            entries,
        })
    }
    fn merge(&mut self, values: &[Value], account: &str) -> io::Result<()> {
        for value in values {
            if !entry_valid(value, account) {
                return Err(invalid());
            }
            if let Some(old) = self
                .entries
                .iter_mut()
                .find(|old| old["h"] == value["h"] && old["i"] == value["i"])
            {
                *old = value.clone();
            } else {
                self.entries.push(value.clone());
            }
        }
        self.entries.sort_by_key(|v| {
            std::cmp::Reverse((v["b"].as_u64().unwrap(), v["i"].as_u64().unwrap()))
        });
        self.entries.truncate(300);
        Ok(())
    }
    fn page(&self, asset: &str, account: &str, offset: usize, cached: bool, more_sync: bool) -> Value {
        let entries: Vec<_> = self.entries.iter().skip(offset).take(3).cloned().collect();
        let next = if offset + entries.len() < self.entries.len() {
            format!("0:{}", offset + entries.len())
        } else if self.entries.len() < 300 {
            self.older.clone()
        } else {
            String::new()
        };
        json!({"chain_id":56,"account":account,"asset":asset,"finalized":true,
            "entries":entries,"next":next,"cached":cached,"more_sync":more_sync})
    }
}

pub fn query(directory: &Path, account: &str, page: &str, cache_only: bool) -> io::Result<Value> {
    query_with(
        directory,
        account,
        page,
        cache_only,
        &mut history::transfers_since,
    )
}
/// Local-only cleanup; no RPC or explorer call is made.
pub fn forget(directory: &Path, account: &str) -> io::Result<Value> {
    let account = address(account)?;
    for asset in ["usdt", "bnb"] {
        for extension in ["json", "tmp"] {
            let path = directory.join(format!("history-56-{asset}-{}.{extension}", account.to_ascii_lowercase()));
            match std::fs::remove_file(path) {
                Ok(()) => (),
                Err(e) if e.kind() == io::ErrorKind::NotFound => (),
                Err(e) => return Err(e),
            }
        }
    }
    Ok(json!({"deleted":true,"account":account}))
}
fn query_with(directory: &Path, account: &str, page: &str, cache_only: bool, scan: &mut Scanner<'_>) -> io::Result<Value> {
    query_with_asset(directory, account, page, cache_only, "USDT", scan)
}
fn query_with_asset(
    directory: &Path,
    account: &str,
    page: &str,
    cache_only: bool,
    asset: &str,
    scan: &mut Scanner<'_>,
) -> io::Result<Value> {
    let account = address(account)?;
    if !history::valid_cursor(page) {
        return Err(invalid());
    }
    let path = directory.join(format!(
        "history-56-{}-{}.json",
        asset.to_ascii_lowercase(),
        account.to_ascii_lowercase()
    ));
    let mut state = std::fs::metadata(&path)
        .ok()
        .filter(|m| m.len() <= 256 * 1024)
        .and_then(|_| std::fs::read(&path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .and_then(|value| Snapshot::parse(value, &account).ok())
        .unwrap_or_default();
    // Cached pages and startup reads perform no network calls.
    if cache_only {
        return Ok(state.page(asset, &account, 0, true, false));
    }
    if let Some(offset) = page.strip_prefix("0:") {
        let offset = offset.parse::<usize>().map_err(|_| invalid())?;
        if offset > state.entries.len() {
            return Err(invalid());
        }
        return Ok(state.page(asset, &account, offset, true, false));
    }
    let sync = page.is_empty();
    let request = if sync { state.pending.as_str() } else { page };
    let data = scan(&account, request, if sync { state.high } else { None })?;
    if data["chain_id"] != 56
        || data["account"] != account
        || data["finalized"] != true
        || data["asset"] != asset
    {
        return Err(invalid());
    }
    let values = data["entries"]
        .as_array()
        .filter(|a| a.len() <= 3)
        .ok_or_else(invalid)?;
    let next = cursor(&data["next"])?;
    let end = data["to_block"].as_u64().ok_or_else(invalid)?;
    let head = data["head_block"].as_u64().ok_or_else(invalid)?;
    let initial = state.high.is_none();
    let more_page = next
        .strip_prefix(&format!("{end}:"))
        .is_some_and(|offset| offset != "0");
    state.merge(values, &account)?;
    let more_sync = if sync {
        if more_page {
            state.pending = next.clone();
        } else {
            state.high = Some(end);
            state.pending.clear();
        }
        if initial {
            state.older = if more_page {
                String::new()
            } else {
                next.clone()
            };
        }
        more_page || end < head
    } else {
        state.older = next.clone();
        false
    };
    let stored = json!({"version":1,"account":account,"chain":56,"high":state.high,
        "pending":state.pending,"older":state.older,"entries":state.entries});
    let bytes = serde_json::to_vec(&stored).map_err(io::Error::other)?;
    // Replace atomically on Windows; a failed write preserves the previous cache.
    let temp = path.with_extension("tmp");
    std::fs::write(&temp, bytes)?;
    let from = wide(temp.to_str().ok_or_else(invalid)?);
    let to = wide(path.to_str().ok_or_else(invalid)?);
    check(unsafe {
        windows_sys::Win32::Storage::FileSystem::MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            windows_sys::Win32::Storage::FileSystem::MOVEFILE_REPLACE_EXISTING
                | windows_sys::Win32::Storage::FileSystem::MOVEFILE_WRITE_THROUGH,
        )
    })?;
    if sync {
        let mut result = state.page(asset, &account, 0, false, more_sync);
        result["scan_from"] = data["from_block"].clone();
        result["scan_to"] = json!(end);
        Ok(result)
    } else {
        // The requested older page is returned immediately, then cached for reopening.
        Ok(
            json!({"chain_id":56,"account":account,"asset":asset,"finalized":true,
            "entries":values,"next":next,"cached":false,"more_sync":false,"scan_from":data["from_block"],"scan_to":end}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const ACCOUNT: &str = "0x1111111111111111111111111111111111111111";
    fn row(n: u64) -> Value {
        json!({"h":format!("0x{n:064x}"),"i":n,"b":n,"t":1700000000,
            "from":"0x2222222222222222222222222222222222222222","to":ACCOUNT,"a":"10.000000000000000000"})
    }
    fn page(rows: Vec<Value>, end: u64, head: u64, next: &str) -> Value {
        json!({"chain_id":56,"account":ACCOUNT,"asset":"USDT","finalized":true,
            "entries":rows,"to_block":end,"head_block":head,"next":next})
    }
    fn directory(label: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "web3rust-history-test-{}-{label}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }
    fn clean(dir: &Path) {
        for file in std::fs::read_dir(dir).unwrap() {
            std::fs::remove_file(file.unwrap().path()).unwrap();
        }
        std::fs::remove_dir(dir).unwrap();
    }
    #[test]
    #[ignore = "live read-only cold/cache/incremental benchmark"]
    fn live_cache_latency() {
        if let Ok(proxy) = std::env::var("WEB3RUST_TEST_PROXY") {
            configure_proxy(proxy).unwrap();
        }
        let dir = directory("live");
        let account = "0x0000000000000000000000000000000000000001";
        for (name, cache_only) in [("cold", false), ("cache", true), ("incremental", false)] {
            TRANSPORT.with(|slot| *slot.borrow_mut() = None);
            let start = std::time::Instant::now();
            let result = query(&dir, account, "", cache_only).unwrap();
            assert_eq!(result["chain_id"], 56);
            println!("{name}: {}ms", start.elapsed().as_millis());
        }
        clean(&dir);
    }
    #[test]
    fn restart_cache_delta_pagination_and_failed_refresh() {
        let dir = directory("restart");
        let first = query_with(&dir, ACCOUNT, "", false, &mut |_, _, after| {
            assert_eq!(after, None);
            Ok(page(vec![row(100), row(99), row(98)], 100, 100, "90:0"))
        })
        .unwrap();
        assert_eq!(first["entries"].as_array().unwrap().len(), 3);
        let start = std::time::Instant::now();
        let cached = query_with(&dir, ACCOUNT, "", true, &mut |_, _, _| {
            panic!("cache must not access network")
        })
        .unwrap();
        println!("cached history read={}us", start.elapsed().as_micros());
        assert_eq!(cached["entries"], first["entries"]);
        let update = query_with(&dir, ACCOUNT, "", false, &mut |_, _, after| {
            assert_eq!(after, Some(100));
            Ok(page(vec![row(102)], 102, 102, "100:0"))
        })
        .unwrap();
        assert_eq!(update["entries"][0]["b"], 102);
        assert_eq!(update["next"], "0:3");
        let older = query_with(&dir, ACCOUNT, "0:3", false, &mut |_, _, _| {
            panic!("cached pagination rescanned chain")
        })
        .unwrap();
        assert_eq!(older["entries"][0]["b"], 98);
        assert_eq!(older["next"], "90:0");
        assert!(
            query_with(&dir, ACCOUNT, "", false, &mut |_, _, _| Err(
                io::Error::other("offline")
            ))
            .is_err()
        );
        assert_eq!(
            query_with(&dir, ACCOUNT, "", true, &mut |_, _, _| panic!()).unwrap()["entries"],
            update["entries"]
        );
        clean(&dir);
    }
    #[test]
    fn partial_window_does_not_advance_watermark() {
        let dir = directory("pending");
        query_with(&dir, ACCOUNT, "", false, &mut |_, _, _| {
            Ok(page(vec![row(100)], 100, 100, "90:0"))
        })
        .unwrap();
        let part = query_with(&dir, ACCOUNT, "", false, &mut |_, _, after| {
            assert_eq!(after, Some(100));
            Ok(page(vec![row(104), row(103), row(102)], 104, 104, "104:3"))
        })
        .unwrap();
        assert_eq!(part["more_sync"], true);
        query_with(&dir, ACCOUNT, "", false, &mut |_, cursor, after| {
            assert_eq!(cursor, "104:3");
            assert_eq!(after, Some(100));
            Ok(page(vec![row(101)], 104, 104, "100:0"))
        })
        .unwrap();
        query_with(&dir, ACCOUNT, "", false, &mut |_, cursor, after| {
            assert_eq!(cursor, "");
            assert_eq!(after, Some(104));
            Ok(page(vec![], 104, 104, "104:0"))
        })
        .unwrap();
        clean(&dir);
    }
    #[test]
    fn forgetting_account_removes_cache_and_staging_only_for_that_account() {
        let dir = directory("forget");
        let other = dir.join("history-56-usdt-0x2222222222222222222222222222222222222222.json");
        std::fs::write(&other, b"other account").unwrap();
        for asset in ["usdt", "bnb"] {
            for ext in ["json", "tmp"] {
                std::fs::write(dir.join(format!("history-56-{asset}-{ACCOUNT}.{ext}")), b"public fixture").unwrap();
            }
        }
        assert!(forget(&dir, "../anything").is_err());
        forget(&dir, ACCOUNT).unwrap();
        forget(&dir, ACCOUNT).unwrap();
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        assert_eq!(std::fs::read(&other).unwrap(), b"other account");
        assert_eq!(query(&dir, ACCOUNT, "", true).unwrap()["entries"], json!([]));
        clean(&dir);
    }
    #[test]
    fn invalid_cache_is_discarded_and_wallets_are_separate() {
        let dir = directory("corrupt");
        let path = dir.join(format!("history-56-usdt-{ACCOUNT}.json"));
        std::fs::write(path, b"truncated").unwrap();
        assert_eq!(
            query_with(&dir, ACCOUNT, "", true, &mut |_, _, _| panic!()).unwrap()["entries"],
            json!([])
        );
        let mut bad = row(1);
        bad["to"] = json!("0x3333333333333333333333333333333333333333");
        assert!(!entry_valid(&bad, ACCOUNT));
        clean(&dir);
    }
}
