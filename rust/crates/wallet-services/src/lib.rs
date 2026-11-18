//! Public RPC transport. Never link this crate into the signer or trusted GUI DLL.
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    io,
    ptr::{null, null_mut},
};
use wallet_core::Amount;
use windows_sys::Win32::Networking::WinHttp::*;
pub mod history;
pub mod history_cache;
pub mod usdt;
pub mod solana;
pub mod tron;
static PROXY: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Called once by the network worker before any request or helper thread starts.
pub fn configure_proxy(authority: String) -> io::Result<()> {
    PROXY
        .set(authority)
        .map_err(|_| io::Error::other("proxy already configured"))
}

struct Http(*mut std::ffi::c_void);
impl Drop for Http {
    fn drop(&mut self) {
        unsafe {
            WinHttpCloseHandle(self.0);
        }
    }
}
fn handle(value: *mut std::ffi::c_void) -> io::Result<Http> {
    if value.is_null() {
        Err(io::Error::last_os_error())
    } else {
        Ok(Http(value))
    }
}
fn check(value: i32) -> io::Result<()> {
    if value == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

#[derive(Clone, Copy)]
pub enum Network {
    Bsc,
    BscHistory,
    Tron,
    Solana,
    SolanaSecondary,
}

fn quantity(value: &Value) -> io::Result<Amount> {
    let text = value
        .as_str()
        .ok_or_else(|| io::Error::other("quantity type"))?;
    let digits = text
        .strip_prefix("0x")
        .ok_or_else(|| io::Error::other("quantity prefix"))?;
    if digits.is_empty()
        || digits.len() > 64
        || (digits.len() > 1 && digits.starts_with('0'))
        || !digits.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(io::Error::other("invalid quantity"));
    }
    let padded = format!("{digits:0>64}");
    let mut bytes = [0; 32];
    hex::decode_to_slice(padded, &mut bytes).map_err(|_| io::Error::other("quantity hex"))?;
    Ok(Amount::from_be_bytes(bytes))
}
fn quantity_u64(value: &Value) -> io::Result<u64> {
    quantity(value)?
        .to_u64()
        .map_err(|_| io::Error::other("quantity overflow"))
}
fn address(text: &str) -> io::Result<String> {
    chain_evm::Address::parse(text)
        .map(|value| value.checksum())
        .map_err(|_| io::Error::other("invalid BSC address"))
}
fn amount_text(value: Amount, decimals: u8) -> io::Result<String> {
    value
        .format_units(decimals)
        .map_err(|_| io::Error::other("amount format"))
}
fn mainnet() -> io::Result<()> {
    if bsc_read("eth_chainId", json!([]))?.as_str() != Some("0x38") {
        return Err(io::Error::other("wrong BSC chain"));
    }
    Ok(())
}

pub fn bnb_balance(account: &str) -> io::Result<Value> {
    let account = address(account)?;
    mainnet()?;
    let balance = quantity(&bsc_read("eth_getBalance", json!([account, "latest"]))?)?;
    Ok(
        json!({"chain_id":56,"asset":"BNB","address":account,"balance":amount_text(balance,18)?,"balance_wei":amount_text(balance,0)?}),
    )
}
pub fn prepare_bnb(sender: &str, recipient: &str, amount: &str) -> io::Result<Value> {
    prepare_bnb_inner(sender, recipient, Some(amount), &mut bsc_read)
}
pub fn prepare_all_bnb(sender: &str, recipient: &str) -> io::Result<Value> {
    prepare_bnb_inner(sender, recipient, None, &mut bsc_read)
}

#[cfg(test)]
mod maximum_tests {
    use super::*;
    #[test]
    fn maximum_quote_uses_same_fee_and_rejects_insufficient_or_changed_balance() {
        for balance in [20_999u64, 21_000, 21_001, 1_000_000] {
            for changed in [false, true] {
                let mut estimated = false;
                let mut rpc = |method: &str, params: Value| -> io::Result<Value> {
                    Ok(match method {
                        "eth_chainId" => json!("0x38"),
                        "eth_getCode" => json!("0x"),
                        "eth_getTransactionCount" => json!("0x0"),
                        "eth_gasPrice" => json!("0x1"),
                        "eth_getBalance" => json!(format!("0x{:x}", if changed && params[1]=="latest" {0} else {balance})),
                        "eth_estimateGas" => {
                            estimated = true;
                            assert_eq!(params[0]["gasPrice"], "0x1");
                            assert_eq!(params[0]["value"], format!("0x{:x}",balance-21000));
                            json!("0x5208")
                        },
                        _ => panic!("unexpected RPC"),
                    })
                };
                let result = prepare_bnb_inner("0x0000000000000000000000000000000000000001",
                    "0x0000000000000000000000000000000000000002", None, &mut rpc);
                if balance>21000 && !changed {
                    let quote = result.unwrap();
                    let amount = Amount::parse_units(quote["amount"].as_str().unwrap(),18).unwrap();
                    let fee = Amount::parse_units(quote["maximum_fee"].as_str().unwrap(),18).unwrap();
                    assert_eq!(amount.checked_add(fee).unwrap(),Amount::from_u64(balance));
                    assert!(estimated);
                } else { assert!(result.is_err()); }
            }
        }
    }
}
fn prepare_bnb_inner(sender: &str, recipient: &str, requested: Option<&str>, rpc: &mut dyn FnMut(&str, Value) -> io::Result<Value>) -> io::Result<Value> {
    let sender = address(sender)?;
    let recipient = address(recipient)?;
    if recipient == "0x0000000000000000000000000000000000000000" {
        return Err(io::Error::other("zero recipient"));
    }
    if quantity_u64(&rpc("eth_chainId", json!([]))?)? != 56 { return Err(io::Error::other("wrong BSC chain")); }
    if rpc("eth_getCode", json!([recipient, "latest"]))?.as_str() != Some("0x") {
        return Err(io::Error::other(
            "first native release supports ordinary recipients only",
        ));
    }
    let nonce = quantity_u64(&rpc(
        "eth_getTransactionCount",
        json!([sender, "pending"]),
    )?)?;
    let gas_price = quantity_u64(&rpc("eth_gasPrice", json!([]))?)?;
    if gas_price == 0 || gas_price > 1_000_000_000_000 {
        return Err(io::Error::other("gas price out of supported range"));
    }
    let maximum_fee = Amount::from_u64(gas_price)
        .checked_mul_u64(21000).map_err(|_| io::Error::other("fee overflow"))?;
    let balance = quantity(&rpc("eth_getBalance", json!([sender, "pending"]))?)?;
    let amount = match requested {
        Some(text) => Amount::parse_units(text, 18).map_err(|_| io::Error::other("invalid amount"))?,
        None => balance.checked_sub(maximum_fee).map_err(|_| io::Error::other("BNB 余额不足以支付手续费"))?,
    };
    if amount == Amount::ZERO { return Err(io::Error::other("扣除手续费后没有可发送的 BNB")); }
    let value_hex = hex::encode(amount.to_be_bytes());
    let value_hex = format!("0x{}", value_hex.trim_start_matches('0'));
    let estimate = quantity_u64(&rpc(
        "eth_estimateGas",
        json!([{"from":sender,"to":recipient,"value":value_hex,"gasPrice":format!("0x{gas_price:x}")}]),
    )?)?;
    if estimate != 21000 {
        return Err(io::Error::other(
            "native recipient requires unsupported gas",
        ));
    }
    let maximum_fee = Amount::from_u64(gas_price)
        .checked_mul_u64(21000)
        .map_err(|_| io::Error::other("fee overflow"))?;
    let required = amount
        .checked_add(maximum_fee)
        .map_err(|_| io::Error::other("total overflow"))?;
    let balance = quantity(&rpc("eth_getBalance", json!([sender, "latest"]))?)?;
    if balance < required {
        return Err(io::Error::other("insufficient BNB including fee"));
    }
    Ok(
        json!({"chain_id":56,"asset":"BNB","sender":sender,"recipient":recipient,"amount":amount_text(amount,18)?,"nonce":nonce.to_string(),"gas_price_wei":gas_price.to_string(),"gas_limit":"21000","maximum_fee":amount_text(maximum_fee,18)?}),
    )
}

/// Only called after user confirmation and durable broadcasting state in the signer.
pub fn broadcast_bnb(raw_hex: &str, expected_hash: &str) -> io::Result<Value> {
    if raw_hex.len() > 1024 || raw_hex.is_empty() || expected_hash.len() != 64 {
        return Err(io::Error::other("invalid signed payload"));
    }
    let bytes = hex::decode(raw_hex).map_err(|_| io::Error::other("invalid signed hex"))?;
    if hex::encode(chain_evm::keccak256(&bytes)) != expected_hash {
        return Err(io::Error::other("payload hash mismatch"));
    }
    mainnet()?;
    let response = post(
        Network::Bsc,
        "/",
        &json!({"jsonrpc":"2.0","id":1,"method":"eth_sendRawTransaction","params":[format!("0x{raw_hex}")]}),
    )?;
    if response.get("id").and_then(Value::as_u64) != Some(1)
        || response.get("result").and_then(Value::as_str) != Some(&format!("0x{expected_hash}"))
    {
        return Err(io::Error::other(
            "broadcast result unknown; query original hash",
        ));
    }
    Ok(json!({"hash":expected_hash,"state":"submitted"}))
}

pub fn bnb_status(hash: &str) -> io::Result<Value> {
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(io::Error::other("invalid transaction hash"));
    }
    mainnet()?;
    let receipt = bsc_read("eth_getTransactionReceipt", json!([format!("0x{hash}")]))?;
    if receipt.is_null() {
        return Ok(json!({"hash":hash,"state":"unknown"}));
    }
    if receipt.get("transactionHash").and_then(Value::as_str) != Some(&format!("0x{hash}")) {
        return Err(io::Error::other("receipt identity mismatch"));
    }
    let number = receipt
        .get("blockNumber")
        .ok_or_else(|| io::Error::other("receipt block missing"))?;
    let height = quantity_u64(number)?;
    let block_hash = receipt
        .get("blockHash")
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other("receipt block hash missing"))?;
    let canonical = bsc_read("eth_getBlockByNumber", json!([number, false]))?;
    if canonical.get("hash").and_then(Value::as_str) != Some(block_hash) {
        return Ok(json!({"hash":hash,"state":"unknown"}));
    }
    let finalized = bsc_read("eth_getBlockByNumber", json!(["finalized", false]))?;
    let final_height = quantity_u64(
        finalized
            .get("number")
            .ok_or_else(|| io::Error::other("finality unavailable"))?,
    )?;
    let success = match receipt.get("status").and_then(Value::as_str) {
        Some("0x1") => true,
        Some("0x0") => false,
        _ => return Err(io::Error::other("receipt status missing")),
    };
    let state = if final_height < height {
        "included"
    } else if success {
        "finalized"
    } else {
        "reverted"
    };
    if success {
        let tx = bsc_read("eth_getTransactionByHash", json!([format!("0x{hash}")]))?;
        if tx["hash"] != receipt["transactionHash"]
            || tx["blockHash"] != receipt["blockHash"]
            || tx["to"] != receipt["to"]
            || tx["from"] != receipt["from"]
        {
            return Err(io::Error::other("transaction and receipt disagree"));
        }
        let to = address(tx["to"].as_str().unwrap_or(""))?;
        if to == chain_evm::bsc::USDT.checksum() {
            usdt::verify_transfer(&receipt, &tx)?;
        } else if tx["input"].as_str() != Some("0x") {
            return Err(io::Error::other("unsupported transaction receipt"));
        }
    }
    Ok(
        json!({"hash":hash,"state":state,"block_number":height.to_string(),"block_hash":block_hash,"finalized_height":final_height.to_string()}),
    )
}

fn post(network: Network, path: &str, body: &Value) -> io::Result<Value> {
    TRANSPORT.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = Some(Transport::new()?);
        }
        let result = slot.as_mut().unwrap().post(network, path, body);
        if result.is_err() {
            // Discard a failed session, but never retry a possibly submitted transaction here.
            *slot = None;
        }
        result
    })
}

thread_local! {
    // Each contained worker handles one operation. Reuse HTTPS connections for its
    // sequential RPC calls without sharing handles between threads or processes.
    static TRANSPORT: RefCell<Option<Transport>> = const { RefCell::new(None) };
}

struct Transport {
    // Child handles must be dropped before their parent session.
    bsc: Option<Http>,
    history: Option<Http>,
    tron: Option<Http>,
    solana: Option<Http>,
    solana_secondary: Option<Http>,
    session: Http,
}

impl Transport {
    fn new() -> io::Result<Self> {
        unsafe {
            let agent = wide("Web3Rust/0.1");
            let proxy = PROXY.get().map(|value| wide(value));
            let session = handle(WinHttpOpen(
                agent.as_ptr(),
                if proxy.is_some() {
                    WINHTTP_ACCESS_TYPE_NAMED_PROXY
                } else {
                    WINHTTP_ACCESS_TYPE_NO_PROXY
                },
                proxy.as_ref().map_or(null(), |value| value.as_ptr()),
                null(),
                0,
            ))?;
            check(WinHttpSetTimeouts(session.0, 5000, 5000, 5000, 5000))?;
            Ok(Self {
                bsc: None,
                history: None,
                tron: None,
                solana: None,
                solana_secondary: None,
                session,
            })
        }
    }

    fn connection(&mut self, network: Network) -> io::Result<&Http> {
        let (slot, host) = match network {
            Network::Bsc => (&mut self.bsc, "bsc-dataseed.bnbchain.org"),
            Network::BscHistory => (&mut self.history, "bsc-rpc.publicnode.com"),
            Network::Tron => (&mut self.tron, "api.trongrid.io"),
            Network::Solana => (&mut self.solana, "api.mainnet-beta.solana.com"),
            Network::SolanaSecondary => (&mut self.solana_secondary, "solana-rpc.publicnode.com"),
        };
        if slot.is_none() {
            let host = wide(host);
            *slot = Some(handle(unsafe {
                WinHttpConnect(self.session.0, host.as_ptr(), 443, 0)
            })?);
        }
        Ok(slot.as_ref().unwrap())
    }

    fn post(&mut self, network: Network, path: &str, body: &Value) -> io::Result<Value> {
        let data = serde_json::to_vec(body).map_err(io::Error::other)?;
        if data.len() > 16384 {
            return Err(io::Error::other("request too large"));
        }
        unsafe {
            let connection = self.connection(network)?;
            let verb = wide("POST");
            let path = wide(path);
            let request = handle(WinHttpOpenRequest(
                connection.0,
                verb.as_ptr(),
                path.as_ptr(),
                null(),
                null(),
                null(),
                WINHTTP_FLAG_SECURE,
            ))?;
            if matches!(network, Network::BscHistory) {
                check(WinHttpSetTimeouts(request.0, 5000, 5000, 5000, 12000))?;
            }
            let disabled: u32 = WINHTTP_DISABLE_REDIRECTS
                | WINHTTP_DISABLE_COOKIES
                | WINHTTP_DISABLE_AUTHENTICATION;
            check(WinHttpSetOption(
                request.0,
                WINHTTP_OPTION_DISABLE_FEATURE,
                (&disabled as *const u32).cast(),
                4,
            ))?;
            let headers = wide("Content-Type: application/json\r\nAccept: application/json\r\n");
            check(WinHttpSendRequest(
                request.0,
                headers.as_ptr(),
                (headers.len() - 1) as u32,
                data.as_ptr().cast(),
                data.len() as u32,
                data.len() as u32,
                0,
            ))?;
            check(WinHttpReceiveResponse(request.0, null_mut()))?;
            let mut status: u32 = 0;
            let mut length: u32 = 4;
            check(WinHttpQueryHeaders(
                request.0,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                null(),
                (&mut status as *mut u32).cast(),
                &mut length,
                null_mut(),
            ))?;
            if status != 200 {
                return Err(io::Error::other(format!("RPC HTTP status {status}")));
            }
            let mut response = Vec::new();
            loop {
                let mut buffer = [0u8; 8192];
                let mut read = 0;
                check(WinHttpReadData(
                    request.0,
                    buffer.as_mut_ptr().cast(),
                    buffer.len() as u32,
                    &mut read,
                ))?;
                if read == 0 {
                    break;
                }
                if response.len() + read as usize > 2 * 1024 * 1024 {
                    return Err(io::Error::other("RPC response too large"));
                }
                response.extend_from_slice(&buffer[..read as usize]);
            }
            serde_json::from_slice(&response).map_err(|_| io::Error::other("invalid RPC JSON"))
        }
    }
}
pub fn bsc_read(method: &str, params: Value) -> io::Result<Value> {
    if !matches!(
        method,
        "eth_chainId"
            | "eth_blockNumber"
            | "eth_getBlockByNumber"
            | "eth_getBalance"
            | "eth_getTransactionCount"
            | "eth_gasPrice"
            | "eth_estimateGas"
            | "eth_getCode"
            | "eth_getTransactionReceipt"
            | "eth_getTransactionByHash"
            | "eth_call"
    ) {
        return Err(io::Error::other("unsupported read method"));
    }
    let response = post(
        Network::Bsc,
        "/",
        &json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}),
    )?;
    if response.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        || response.get("id").and_then(Value::as_u64) != Some(1)
        || response.get("error").is_some()
    {
        return Err(io::Error::other("RPC error or identity mismatch"));
    }
    response
        .get("result")
        .cloned()
        .ok_or_else(|| io::Error::other("missing RPC result"))
}
pub fn public_probe() -> io::Result<String> {
    let chain = bsc_read("eth_chainId", json!([]))?;
    if chain.as_str() != Some("0x38") {
        return Err(io::Error::other("BSC mainnet chain identity mismatch"));
    }
    let height = bsc_read("eth_blockNumber", json!([]))?;
    let finalized = bsc_read("eth_getBlockByNumber", json!(["finalized", false]))?;
    let tron = post(Network::Tron, "/wallet/getnowblock", &json!({}))?;
    let tron_solid = post(Network::Tron, "/walletsolidity/getnowblock", &json!({}))?;
    let number = |value: &Value| {
        value
            .pointer("/block_header/raw_data/number")
            .and_then(Value::as_u64)
            .ok_or_else(|| io::Error::other("TRON block number missing"))
    };
    let latest = number(&tron)?;
    let solid = number(&tron_solid)?;
    if solid > latest {
        return Err(io::Error::other("TRON inconsistent block order"));
    }
    Ok(format!(
        "BSC mainnet chain=56 latest={} finalized={}\nTRON latest={} solidified={}\n",
        height
            .as_str()
            .ok_or_else(|| io::Error::other("BSC height missing"))?,
        finalized
            .get("number")
            .and_then(Value::as_str)
            .ok_or_else(|| io::Error::other("BSC finalized unavailable"))?,
        latest,
        solid
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "live read-only BSC connection reuse benchmark"]
    fn live_connection_reuse() {
        use std::time::Instant;
        for reuse in [false, true] {
            TRANSPORT.with(|slot| *slot.borrow_mut() = None);
            let start = Instant::now();
            let mut samples = Vec::new();
            for _ in 0..5 {
                if !reuse {
                    TRANSPORT.with(|slot| *slot.borrow_mut() = None);
                }
                let request_start = Instant::now();
                assert_eq!(bsc_read("eth_chainId", json!([])).unwrap(), json!("0x38"));
                samples.push(request_start.elapsed().as_millis());
            }
            println!(
                "reuse={reuse}: five read requests total={}ms samples={samples:?}",
                start.elapsed().as_millis()
            );
        }
        TRANSPORT.with(|slot| *slot.borrow_mut() = None);
        let start = Instant::now();
        let result = usdt::balances("0x0000000000000000000000000000000000000000").unwrap();
        assert_eq!(result["chain_id"], 56);
        assert_eq!(result["usdt_decimals"], 18);
        assert!(result["balance"].is_string() && result["usdt_balance"].is_string());
        println!(
            "complete dual-balance query={}ms",
            start.elapsed().as_millis()
        );
    }

    #[test]
    fn rpc_quantities_preserve_full_uint256_and_reject_noncanonical() {
        assert_eq!(quantity(&json!("0x0")).unwrap(), Amount::ZERO);
        assert_eq!(
            quantity(&json!(format!("0x{}", "f".repeat(64))))
                .unwrap()
                .to_be_bytes(),
            [255; 32]
        );
        for value in [
            json!("0x"),
            json!("0x00"),
            json!("0x01"),
            json!("123"),
            json!(12),
            json!(format!("0x1{}", "0".repeat(64))),
        ] {
            assert!(quantity(&value).is_err());
        }
        assert!(quantity_u64(&json!("0x10000000000000000")).is_err());
    }
}
