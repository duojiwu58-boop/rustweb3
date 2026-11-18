//! Read-only USDT Transfer history. Separate from the signing/payment journal.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

const TRANSFER: &str = "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";
const WINDOW: u64 = 10_000;
const PAGE: usize = 3; // Keep the existing 1800-byte isolated public response bound.
type Rpc = dyn FnMut(&str, Value) -> io::Result<Value>;

pub(crate) fn rpc(method: &str, params: Value) -> io::Result<Value> {
    if !matches!(
        method,
        "eth_chainId" | "eth_getBlockByNumber" | "eth_getLogs" | "eth_getTransactionReceipt"
    ) {
        return Err(io::Error::other("unsupported history method"));
    }
    let response = post(
        Network::BscHistory,
        "/",
        &json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}),
    )?;
    if response["jsonrpc"] != "2.0" || response["id"] != 1 || response.get("error").is_some() {
        return Err(io::Error::other("history provider unavailable"));
    }
    response
        .get("result")
        .cloned()
        .ok_or_else(|| io::Error::other("missing history result"))
}

pub fn valid_cursor(cursor: &str) -> bool {
    parse_cursor(cursor).is_ok()
}
pub(crate) fn parse_cursor(cursor: &str) -> io::Result<Option<(u64, usize)>> {
    if cursor.is_empty() {
        return Ok(None);
    }
    let (end, offset) = cursor
        .split_once(':')
        .ok_or_else(|| io::Error::other("invalid history cursor"))?;
    if end.is_empty()
        || offset.is_empty()
        || end.len() > 16
        || offset.len() > 6
        || !end
            .bytes()
            .chain(offset.bytes())
            .all(|c| c.is_ascii_digit())
    {
        return Err(io::Error::other("invalid history cursor"));
    }
    let end = end.parse::<u64>().map_err(io::Error::other)?;
    let offset = offset.parse::<usize>().map_err(io::Error::other)?;
    if end > i64::MAX as u64 || offset > 100_000 {
        return Err(io::Error::other("history cursor out of range"));
    }
    Ok(Some((end, offset)))
}
pub(crate) fn fixed_hex(value: &Value, bytes: usize) -> io::Result<String> {
    let text = value
        .as_str()
        .ok_or_else(|| io::Error::other("history hex type"))?;
    if text.len() != 2 + bytes * 2
        || !text.starts_with("0x")
        || !text[2..].bytes().all(|c| c.is_ascii_hexdigit())
    {
        return Err(io::Error::other("invalid history hex"));
    }
    Ok(text.to_ascii_lowercase())
}
fn topic_address(value: &Value) -> io::Result<String> {
    let topic = fixed_hex(value, 32)?;
    if &topic[2..26] != "000000000000000000000000" {
        return Err(io::Error::other("invalid address padding"));
    }
    address(&format!("0x{}", &topic[26..]))
}
#[derive(Clone, Debug)]
struct Event {
    block: u64,
    index: u64,
    hash: String,
    block_hash: String,
    from: String,
    to: String,
    amount: String,
}
fn decode(value: &Value, account: &str, low: u64, high: u64) -> io::Result<Event> {
    if value["removed"] != false
        || fixed_hex(&value["address"], 20)? != chain_evm::bsc::USDT.checksum().to_ascii_lowercase()
    {
        return Err(io::Error::other(
            "untrusted history contract or removed event",
        ));
    }
    let topics = value["topics"]
        .as_array()
        .filter(|v| v.len() == 3)
        .ok_or_else(|| io::Error::other("invalid Transfer topics"))?;
    if fixed_hex(&topics[0], 32)? != TRANSFER {
        return Err(io::Error::other("invalid Transfer signature"));
    }
    let from = topic_address(&topics[1])?;
    let to = topic_address(&topics[2])?;
    if from != account && to != account {
        return Err(io::Error::other("history account mismatch"));
    }
    let block = quantity_u64(&value["blockNumber"])?;
    if !(low..=high).contains(&block) {
        return Err(io::Error::other("history block outside requested range"));
    }
    let data = fixed_hex(&value["data"], 32)?;
    let mut bytes = [0; 32];
    hex::decode_to_slice(&data[2..], &mut bytes).map_err(io::Error::other)?;
    Ok(Event {
        block,
        index: quantity_u64(&value["logIndex"])?,
        hash: fixed_hex(&value["transactionHash"], 32)?,
        block_hash: fixed_hex(&value["blockHash"], 32)?,
        from,
        to,
        amount: amount_text(Amount::from_be_bytes(bytes), 18)?,
    })
}

pub fn transfers(account: &str, cursor: &str) -> io::Result<Value> {
    transfers_since(account, cursor, None)
}
pub fn transfers_since(account: &str, cursor: &str, after: Option<u64>) -> io::Result<Value> {
    // The two address filters are independent. Reuse this thread's connection
    // for outgoing logs while one bounded worker reads incoming logs.
    let mut incoming: Option<(Value, io::Result<Value>)> = None;
    transfers_with_options(account, cursor, after, &mut move |method, params| {
        if method != "eth_getLogs" {
            return rpc(method, params);
        }
        if let Some((expected, result)) = incoming.take() {
            if expected != params {
                return Err(io::Error::other("history filter mismatch"));
            }
            return result;
        }
        let mut other = params.clone();
        other[0]["topics"] = json!([TRANSFER, null, params[0]["topics"][1]]);
        let (outgoing, received) = std::thread::scope(|scope| {
            let worker = scope.spawn(|| rpc("eth_getLogs", other.clone()));
            let outgoing = rpc(method, params);
            let received = worker
                .join()
                .unwrap_or_else(|_| Err(io::Error::other("history reader failed")));
            (outgoing, received)
        });
        incoming = Some((other, received));
        outgoing
    })
}
#[cfg(test)]
fn transfers_with(account: &str, cursor: &str, rpc: &mut Rpc) -> io::Result<Value> {
    transfers_with_options(account, cursor, None, rpc)
}
fn transfers_with_options(
    account: &str,
    cursor: &str,
    after: Option<u64>,
    rpc: &mut Rpc,
) -> io::Result<Value> {
    let account = address(account)?;
    let cursor = parse_cursor(cursor)?;
    if rpc("eth_chainId", json!([]))? != "0x38" {
        return Err(io::Error::other("history wrong chain"));
    }
    let head = rpc("eth_getBlockByNumber", json!(["finalized", false]))?;
    let finalized = quantity_u64(&head["number"])?;
    let head_time = quantity_u64(&head["timestamp"])?;
    let latest = after.map_or(finalized, |height| {
        finalized.min(height.saturating_add(WINDOW))
    });
    let (end, offset) = cursor.unwrap_or((latest, 0));
    if end > finalized {
        return Err(io::Error::other(
            "history cursor is ahead of finalized chain",
        ));
    }
    if after.is_some_and(|height| height > finalized) {
        return Err(io::Error::other("cached height ahead of chain"));
    }
    let start = end
        .saturating_sub(WINDOW - 1)
        .max(after.map_or(0, |height| height + 1));
    let topic = format!("0x{:0>64}", account[2..].to_ascii_lowercase());
    let mut events: BTreeMap<(u64, u64), Event> = BTreeMap::new();
    for topics in [json!([TRANSFER, topic]), json!([TRANSFER, null, topic])] {
        if start > end {
            break;
        }
        let value = rpc(
            "eth_getLogs",
            json!([{"address":chain_evm::bsc::USDT.checksum(),
            "fromBlock":format!("0x{start:x}"),"toBlock":format!("0x{end:x}"),"topics":topics}]),
        )?;
        let logs = value
            .as_array()
            .ok_or_else(|| io::Error::other("history logs type"))?;
        for log in logs {
            let event = decode(log, &account, start, end)?;
            if let Some(old) = events.get(&(event.block, event.index)) {
                if old.hash != event.hash
                    || old.from != event.from
                    || old.to != event.to
                    || old.amount != event.amount
                    || old.block_hash != event.block_hash
                {
                    return Err(io::Error::other("conflicting duplicate history event"));
                }
            } else {
                events.insert((event.block, event.index), event);
            }
        }
    }
    if offset > events.len() {
        return Err(io::Error::other("history cursor changed; refresh history"));
    }
    let total = events.len();
    let mut blocks: BTreeMap<u64, (String, u64)> = BTreeMap::new();
    let mut entries = Vec::new();
    let mut ids = BTreeSet::new();
    for event in events.into_values().rev().skip(offset).take(PAGE) {
        if let std::collections::btree_map::Entry::Vacant(slot) = blocks.entry(event.block) {
            let block = rpc(
                "eth_getBlockByNumber",
                json!([format!("0x{:x}", event.block), false]),
            )?;
            if quantity_u64(&block["number"])? != event.block {
                return Err(io::Error::other("history block identity"));
            }
            slot.insert((
                fixed_hex(&block["hash"], 32)?,
                quantity_u64(&block["timestamp"])?,
            ));
        }
        let (hash, time) = &blocks[&event.block];
        if hash != &event.block_hash || *time > head_time || *time > 253_402_300_799 {
            return Err(io::Error::other("history block hash or timestamp mismatch"));
        }
        if !ids.insert((event.hash.clone(), event.index)) {
            return Err(io::Error::other("duplicate history identity"));
        }
        entries.push(
            json!({"h":event.hash,"i":event.index,"b":event.block,"t":time,
            "from":event.from,"to":event.to,"a":event.amount}),
        );
    }
    let next = if offset + entries.len() < total {
        format!("{end}:{}", offset + entries.len())
    } else if start > 0 {
        format!("{}:0", start - 1)
    } else {
        String::new()
    };
    let result = json!({"chain_id":56,"account":account,"asset":"USDT","entries":entries,
        "next":next,"from_block":start,"to_block":end,"head_block":finalized,"finalized":true});
    if serde_json::to_vec(&result).map_err(io::Error::other)?.len() > 1800 {
        return Err(io::Error::other("history page too large"));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    const ACCOUNT: &str = "0x1111111111111111111111111111111111111111";
    const OTHER: &str = "0x2222222222222222222222222222222222222222";
    fn event(index: u64, from: &str, to: &str, amount: u128) -> Value {
        json!({"address":chain_evm::bsc::USDT.checksum(),"removed":false,
            "topics":[TRANSFER,format!("0x{:0>64}",&from[2..]),format!("0x{:0>64}",&to[2..])],
            "data":format!("0x{amount:064x}"),"blockNumber":"0x186a0","logIndex":format!("0x{index:x}"),
            "transactionHash":format!("0x{:064x}",index+1),"blockHash":format!("0x{}","ab".repeat(32))})
    }
    fn fixture(method: &str, params: Value) -> io::Result<Value> {
        Ok(match method {
            "eth_chainId" => json!("0x38"),
            "eth_getBlockByNumber" => {
                json!({"number":"0x186a0","timestamp":"0x6553f100","hash":format!("0x{}","ab".repeat(32))})
            }
            "eth_getLogs" => {
                let mine = event(2, ACCOUNT, ACCOUNT, 3);
                if params[0]["topics"].as_array().unwrap().len() == 2 {
                    json!([event(1, ACCOUNT, OTHER, 2_000_000_000_000_000_000), mine])
                } else {
                    json!([
                        event(0, OTHER, ACCOUNT, 10_000_000_000_000_000_000),
                        mine,
                        event(3, OTHER, ACCOUNT, 1)
                    ])
                }
            }
            _ => panic!("unexpected read"),
        })
    }
    #[test]
    fn incremental_bounds_and_unchanged_head_skip_logs() {
        transfers_with_options(ACCOUNT, "", Some(99999), &mut |method, params| {
            if method == "eth_getLogs" {
                assert_eq!(params[0]["fromBlock"], "0x186a0");
                assert_eq!(params[0]["toBlock"], "0x186a0");
            }
            fixture(method, params)
        })
        .unwrap();
        let result = transfers_with_options(ACCOUNT, "", Some(100000), &mut |method, params| {
            assert_ne!(method, "eth_getLogs");
            fixture(method, params)
        })
        .unwrap();
        assert!(result["entries"].as_array().unwrap().is_empty());
    }
    #[test]
    fn incoming_outgoing_self_transfer_pagination_and_exact_amounts() {
        let first = transfers_with(ACCOUNT, "", &mut fixture).unwrap();
        assert_eq!(first["entries"].as_array().unwrap().len(), 3);
        assert_eq!(first["entries"][0]["a"], "0.000000000000000001");
        assert_eq!(first["entries"][1]["from"], ACCOUNT);
        assert_eq!(first["entries"][1]["to"], ACCOUNT);
        assert_eq!(first["entries"][2]["from"], ACCOUNT);
        assert_eq!(first["next"], "100000:3");
        let second =
            transfers_with(ACCOUNT, first["next"].as_str().unwrap(), &mut fixture).unwrap();
        assert_eq!(second["entries"].as_array().unwrap().len(), 1);
        assert_eq!(second["entries"][0]["a"], "10.000000000000000000");
        assert_eq!(second["next"], "90000:0");
        assert!(serde_json::to_vec(&first).unwrap().len() <= 1800);
    }
    #[test]
    fn rejects_foreign_removed_malformed_events_and_changed_chain() {
        let good = event(0, OTHER, ACCOUNT, 10);
        assert!(decode(&good, ACCOUNT, 50001, 100000).is_ok());
        for (key, value) in [
            ("removed", json!(true)),
            ("address", json!(OTHER)),
            ("data", json!("0x01")),
            ("blockNumber", json!("0x186a1")),
            ("transactionHash", json!("oops")),
        ] {
            let mut bad = good.clone();
            bad[key] = value;
            assert!(decode(&bad, ACCOUNT, 50001, 100000).is_err());
        }
        let mut bad = good.clone();
        bad["topics"][2] = json!(format!("0x1{}", "0".repeat(63)));
        assert!(decode(&bad, ACCOUNT, 50001, 100000).is_err());
        assert!(decode(&good, OTHER, 100001, 150000).is_err());
        assert!(
            transfers_with(ACCOUNT, "", &mut |method, params| {
                if method == "eth_chainId" {
                    Ok(json!("0x1"))
                } else {
                    fixture(method, params)
                }
            })
            .is_err()
        );
        assert!(
            transfers_with(ACCOUNT, "", &mut |method, params| {
                let mut value = fixture(method, params.clone())?;
                if method == "eth_getBlockByNumber" && params[0] != "finalized" {
                    value["hash"] = json!(format!("0x{}", "cd".repeat(32)));
                }
                Ok(value)
            })
            .is_err()
        );
        for cursor in ["password", ":0", "1:", "-1:0", "1:2:3", "1:100001"] {
            assert!(!valid_cursor(cursor));
        }
        assert!(transfers_with(ACCOUNT, "100001:0", &mut fixture).is_err());
        assert!(transfers_with(ACCOUNT, "100000:5", &mut fixture).is_err());
    }
    #[test]
    #[ignore = "read-only latency diagnosis; public test address only"]
    fn live_history_profile() {
        use std::time::Instant;
        if let Ok(proxy) = std::env::var("WEB3RUST_TEST_PROXY") {
            configure_proxy(proxy).unwrap();
        }
        let account = "0x0000000000000000000000000000000000000001";
        for round in 1..=2 {
            TRANSPORT.with(|slot| *slot.borrow_mut() = None);
            let started = Instant::now();
            usdt::balances(account).unwrap();
            println!(
                "round={round} dual_balance_ms={}",
                started.elapsed().as_millis()
            );
            let started = Instant::now();
            rpc("eth_chainId", json!([])).unwrap();
            println!(
                "round={round} history_chain_ms={}",
                started.elapsed().as_millis()
            );
            let started = Instant::now();
            let head = rpc("eth_getBlockByNumber", json!(["finalized", false])).unwrap();
            let end = quantity_u64(&head["number"]).unwrap();
            println!(
                "round={round} history_head_ms={}",
                started.elapsed().as_millis()
            );
            for size in [100u64, 1000, 10000] {
                let started = Instant::now();
                let result = rpc(
                    "eth_getLogs",
                    json!([{
                        "address":chain_evm::bsc::USDT.checksum(),
                        "fromBlock":format!("0x{:x}", end-size+1),
                        "toBlock":format!("0x{end:x}"),
                        "topics":[TRANSFER,null,format!("0x{:0>64}", &account[2..])]
                    }]),
                );
                println!(
                    "round={round} logs_blocks={size} ms={} ok={}",
                    started.elapsed().as_millis(),
                    result.is_ok()
                );
            }
            TRANSPORT.with(|slot| *slot.borrow_mut() = None);
            let started = Instant::now();
            let result = transfers(account, "");
            println!(
                "round={round} full_history_ms={} ok={}",
                started.elapsed().as_millis(),
                result.is_ok()
            );
        }
    }
    #[test]
    #[ignore = "live public history provider; no private wallet"]
    fn live_history_read() {
        let result = transfers("0x0000000000000000000000000000000000000001", "").unwrap();
        assert_eq!(result["chain_id"], 56);
        assert!(result["entries"].is_array());
        println!("PASS: finalized history provider, bounded page and cursor");
    }
}
