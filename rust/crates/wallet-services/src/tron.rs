//! Mainnet USDT only. Public queries and exact signed-payload broadcast.
use super::{Network, post};
use chain_tron::{
    Address, Transaction, Transfer,
    mainnet::{self, GENESIS, Quote, USDT},
};
use serde_json::{Value, json};
use std::io;
use wallet_core::Amount;
fn invalid(s: &str) -> io::Error {
    io::Error::other(format!("TRON: {s}"))
}
fn checked<T>(r: wallet_core::Result<T>) -> io::Result<T> {
    r.map_err(|e| invalid(&e.to_string()))
}
fn rpc(path: &str, data: Value) -> io::Result<Value> {
    thread_local! {
        static LAST_REQUEST: std::cell::Cell<Option<std::time::Instant>> = const { std::cell::Cell::new(None) };
    }
    // Public TronGrid endpoints may rate-limit shared IPs. Never auto-retry broadcast.
    let mut attempt = 0;
    let value = loop {
        // Unauthenticated public endpoints have stricter burst limits. Workers
        // handle one operation each; pace all calls within that operation.
        LAST_REQUEST.with(|last| {
            if let Some(start) = last.get() {
                let interval = std::time::Duration::from_millis(1200);
                if let Some(wait) = interval.checked_sub(start.elapsed()) {
                    std::thread::sleep(wait);
                }
            }
            last.set(Some(std::time::Instant::now()));
        });
        match post(Network::Tron, path, &data) {
            Ok(v) => break v,
            Err(e)
                if path != "/wallet/broadcasthex"
                    && e.to_string() == "RPC HTTP status 429"
                    && attempt < 2 =>
            {
                attempt += 1;
                std::thread::sleep(std::time::Duration::from_secs(attempt));
            }
            Err(e) => return Err(invalid(&format!("{path}: {e}"))),
        }
    };
    if value.get("Error").is_some() || value.get("error").is_some() {
        return Err(invalid("RPC error"));
    }
    Ok(value)
}
type Call<'a> = dyn FnMut(&str, Value) -> io::Result<Value> + 'a;
fn mainnet(call: &mut Call<'_>) -> io::Result<()> {
    if call("/wallet/getblockbynum", json!({"num":0}))?["blockID"] != GENESIS {
        return Err(invalid("wrong genesis"));
    }
    Ok(())
}
fn number(v: &Value) -> io::Result<u64> {
    v.as_u64().ok_or_else(|| invalid("missing integer"))
}
fn text(v: &Value) -> io::Result<&str> {
    v.as_str().ok_or_else(|| invalid("missing text"))
}
fn hex32(v: &Value) -> io::Result<[u8; 32]> {
    hex::decode(text(v)?)
        .map_err(|_| invalid("invalid hex"))?
        .try_into()
        .map_err(|_| invalid("expected 32 bytes"))
}
fn units(n: u64) -> io::Result<String> {
    checked(Amount::from_u64(n).format_units(6))
}
fn account(call: &mut Call<'_>, owner: Address) -> io::Result<Value> {
    let v = call(
        "/wallet/getaccount",
        json!({"address":owner.base58(),"visible":true}),
    )?;
    if !v.is_object() || (!v.as_object().unwrap().is_empty() && v["address"] != owner.base58()) {
        return Err(invalid("account identity mismatch"));
    }
    Ok(v)
}
fn trx(v: &Value) -> io::Result<u64> {
    match v.get("balance") {
        Some(v) => number(v),
        None => Ok(0),
    }
}
fn constant(
    call: &mut Call<'_>,
    owner: Address,
    selector: &str,
    parameter: String,
) -> io::Result<Value> {
    let v = call(
        "/wallet/triggerconstantcontract",
        json!({"owner_address":owner.base58(),"contract_address":USDT,"function_selector":selector,"parameter":parameter,"visible":true}),
    )?;
    if v["result"]["result"] != true
        || v.pointer("/transaction/ret/0/ret")
            .is_some_and(|r| r != "SUCCESS")
        || v.pointer("/transaction/ret/0/contractRet")
            .is_some_and(|r| r != "SUCCESS")
    {
        return Err(invalid("USDT simulation failed"));
    }
    Ok(v)
}
fn token_balance(call: &mut Call<'_>, owner: Address) -> io::Result<Amount> {
    let d = constant(call, owner, "decimals()", String::new())?;
    if Amount::from_be_bytes(hex32(&d["constant_result"][0])?) != Amount::from_u64(6) {
        return Err(invalid("USDT decimals mismatch"));
    }
    let v = constant(
        call,
        owner,
        "balanceOf(address)",
        format!("{:0>64}", hex::encode(&owner.bytes()[1..])),
    )?;
    Ok(Amount::from_be_bytes(hex32(&v["constant_result"][0])?))
}
pub fn balances(owner: &str) -> io::Result<Value> {
    balances_with(owner, &mut rpc)
}
fn balances_with(owner: &str, call: &mut Call<'_>) -> io::Result<Value> {
    let owner = checked(Address::parse(owner))?;
    mainnet(call)?;
    let a = account(call, owner)?;
    let balance = token_balance(call, owner)?;
    Ok(
        json!({"network":"tron-mainnet","address":owner.base58(),"contract":USDT,"usdt_decimals":6,
        "trx":units(trx(&a)?)?,"usdt":checked(balance.format_units(6))?,"active":a.get("address").is_some()}),
    )
}
fn now() -> io::Result<i64> {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| invalid("clock"))?
            .as_millis(),
    )
    .map_err(|_| invalid("clock"))
}
pub fn prepare(sender: &str, recipient: &str, asset_amount: &str) -> io::Result<Value> {
    prepare_with(sender, recipient, asset_amount, &mut rpc, now()?)
}
fn prepare_with(
    sender: &str,
    recipient: &str,
    asset_amount: &str,
    call: &mut Call<'_>,
    now_ms: i64,
) -> io::Result<Value> {
    let amount = checked(mainnet::amount(
        asset_amount
            .strip_prefix("USDT\n")
            .ok_or_else(|| invalid("USDT only"))?,
    ))?;
    let owner = checked(Address::parse(sender))?;
    let recipient = checked(Address::parse(recipient))?;
    if owner == recipient
        || recipient == mainnet::policy().token_contract.unwrap()
        || recipient.bytes()[1..] == [0; 20]
    {
        return Err(invalid("invalid recipient"));
    }
    mainnet(call)?;
    let a = account(call, owner)?;
    if a.get("address").is_none() {
        return Err(invalid("请先转入 TRX 激活发送账户"));
    }
    // Only ordinary, default owner-permission accounts are supported.
    if a.get("type").is_some_and(|t| t != "Normal" && t != 0) {
        return Err(invalid("sender must be an ordinary account"));
    }
    if let Some(p) = a.get("owner_permission") {
        if p["threshold"] != 1
            || p["keys"].as_array().map(Vec::len) != Some(1)
            || p["keys"][0]["address"] != sender
            || p["keys"][0]["weight"] != 1
        {
            return Err(invalid("modified/multisig owner permissions unsupported"));
        }
    }
    if token_balance(call, owner)? < amount {
        return Err(invalid("USDT 余额不足"));
    }
    let parameter = format!(
        "{:0>64}{}",
        hex::encode(&recipient.bytes()[1..]),
        hex::encode(amount.to_be_bytes())
    );
    let simulated = constant(call, owner, "transfer(address,uint256)", parameter)?;
    // Legacy USDT has no return value; accept empty data or ABI true, never false.
    let result = text(&simulated["constant_result"][0])?;
    if !result.is_empty()
        && Amount::from_be_bytes(hex32(&simulated["constant_result"][0])?) != Amount::from_u64(1)
    {
        return Err(invalid("USDT transfer returned false"));
    }
    let energy = number(&simulated["energy_used"])?;
    let params = call("/wallet/getchainparameters", json!({}))?;
    let param = |name: &str| -> io::Result<u64> {
        let values = params["chainParameter"]
            .as_array()
            .ok_or_else(|| invalid("chain parameters"))?;
        let v = values
            .iter()
            .find(|v| v["key"] == name)
            .ok_or_else(|| invalid("fee parameter missing"))?;
        let n = number(&v["value"])?;
        if n == 0 {
            return Err(invalid("zero fee parameter"));
        }
        Ok(n)
    };
    let fee = energy
        .checked_mul(param("getEnergyFee")?)
        .and_then(|v| v.checked_mul(2))
        .ok_or_else(|| invalid("fee overflow"))?;
    let block = call("/walletsolidity/getnowblock", json!({}))?;
    let id = hex32(&block["blockID"])?;
    let height = number(&block["block_header"]["raw_data"]["number"])?;
    if id[..8] != height.to_be_bytes() {
        return Err(invalid("block identity mismatch"));
    }
    let timestamp = number(&block["block_header"]["raw_data"]["timestamp"])?;
    if timestamp > (now_ms + 15_000) as u64 || (now_ms as u64).saturating_sub(timestamp) > 120_000 {
        return Err(invalid("stale solid block or incorrect local clock"));
    }
    let tx = Transaction {
        owner,
        ref_block_bytes: id[6..8].try_into().unwrap(),
        ref_block_hash: id[8..16].try_into().unwrap(),
        timestamp_ms: now_ms,
        expiration_ms: now_ms + 60_000,
        fee_limit: i64::try_from(fee).map_err(|_| invalid("fee overflow"))?,
        transfer: Transfer::Token {
            contract: mainnet::policy().token_contract.unwrap(),
            recipient,
            amount,
        },
    };
    // Full signed envelope + protocol result allowance, without assuming free bandwidth.
    let bandwidth = (checked(tx.raw_bytes(mainnet::policy()))?.len() as u64 + 140)
        .checked_mul(param("getTransactionFee")?)
        .ok_or_else(|| invalid("bandwidth overflow"))?;
    let quote = Quote {
        tx,
        bandwidth_fee: bandwidth,
    };
    let summary = checked(quote.summary())?;
    if trx(&a)?
        < fee
            .checked_add(bandwidth)
            .ok_or_else(|| invalid("fee overflow"))?
    {
        return Err(invalid("TRX 不足以覆盖能量上限和带宽预留"));
    }
    Ok(
        json!({"network":"tron-mainnet","summary":summary,"energy_trx":units(fee)?,"bandwidth_trx":units(bandwidth)?}),
    )
}
pub fn broadcast(raw: &str, txid: &str, summary: &str) -> io::Result<Value> {
    let quote = checked(Quote::parse(summary))?;
    if raw.len() > 1024 {
        return Err(invalid("transaction size"));
    }
    let bytes = hex::decode(raw).map_err(|_| invalid("transaction hex"))?;
    checked(quote.verify_signed(&bytes, txid))?;
    checked(quote.check_fresh(now()?))?;
    mainnet(&mut rpc)?;
    let response = rpc("/wallet/broadcasthex", json!({"transaction":raw}))?;
    if response["result"] != true || response["txid"] != txid {
        return Err(invalid("广播未获确认，请查询原交易，勿重复付款"));
    }
    Ok(json!({"signature":txid,"state":"unknown"}))
}
pub fn status(txid: &str, summary: &str) -> io::Result<Value> {
    status_with(txid, summary, &mut rpc)
}
fn status_with(txid: &str, summary: &str, call: &mut Call<'_>) -> io::Result<Value> {
    let quote = checked(Quote::parse(summary))?;
    if hex::encode(checked(quote.tx.txid(mainnet::policy()))?) != txid {
        return Err(invalid("txid mismatch"));
    }
    mainnet(call)?;
    let tx = call("/walletsolidity/gettransactionbyid", json!({"value":txid}))?;
    let mut state = "unknown";
    if tx.get("txID").is_some() {
        if tx["txID"] != txid
            || text(&tx["raw_data_hex"])?
                != hex::encode(checked(quote.tx.raw_bytes(mainnet::policy()))?)
        {
            return Err(invalid("solid transaction mismatch"));
        }
        let info = call(
            "/walletsolidity/gettransactioninfobyid",
            json!({"value":txid}),
        )?;
        if info["id"] != txid {
            return Err(invalid("solid receipt mismatch"));
        }
        number(&info["blockNumber"])?;
        match text(&info["receipt"]["result"])? {
            "SUCCESS" => {
                if tx["ret"][0]["contractRet"] != "SUCCESS"
                    || info.get("result").is_some_and(|r| r != "SUCCESS" && r != 0)
                {
                    return Err(invalid("contradictory receipt"));
                }
                let Transfer::Token {
                    contract,
                    recipient,
                    amount,
                } = quote.tx.transfer
                else {
                    unreachable!()
                };
                let topic = hex::encode(chain_evm::keccak256(b"Transfer(address,address,uint256)"));
                let from = format!("{:0>64}", hex::encode(&quote.tx.owner.bytes()[1..]));
                let to = format!("{:0>64}", hex::encode(&recipient.bytes()[1..]));
                let logs = info["log"]
                    .as_array()
                    .ok_or_else(|| invalid("USDT Transfer log missing"))?;
                if !logs.iter().any(|l| {
                    l["address"] == hex::encode(&contract.bytes()[1..])
                        && l["topics"] == json!([topic, from, to])
                        && l["data"] == hex::encode(amount.to_be_bytes())
                }) {
                    return Err(invalid("USDT Transfer log mismatch"));
                }
                state = "finalized";
            }
            failure @ ("REVERT"
            | "OUT_OF_ENERGY"
            | "OUT_OF_TIME"
            | "ILLEGAL_OPERATION"
            | "BAD_JUMP_DESTINATION"
            | "OUT_OF_MEMORY"
            | "STACK_TOO_SMALL"
            | "STACK_TOO_LARGE"
            | "JVM_STACK_OVER_FLOW"
            | "UNKNOWN"
            | "TRANSFER_FAILED") => {
                if tx["ret"][0]["contractRet"] != failure || info["result"] != "FAILED" {
                    return Err(invalid("failure receipt mismatch"));
                }
                state = "reverted";
            }
            _ => return Err(invalid("unsupported receipt result")),
        }
    } else if tx.as_object().is_none_or(|v| !v.is_empty()) {
        return Err(invalid("invalid lookup response"));
    }
    // Absence/expiry is not evidence of non-payment. Keep the durable pending lock.
    Ok(json!({"network":"tron-mainnet","signature":txid,"state":state}))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Quote, String) {
        let f: Vec<_> = include_str!("../../../../tests/fixtures/tron-usdt.txt")
            .lines()
            .filter(|l| !l.starts_with('#'))
            .collect();
        (
            Quote {
                tx: Transaction::decode_signed(&hex::decode(f[4]).unwrap(), mainnet::policy())
                    .unwrap(),
                bandwidth_fee: 400_000,
            },
            f[3].into(),
        )
    }
    #[test]
    fn quote_checks_simulation_fees_balance_and_network() {
        let (q, _) = fixture();
        let Transfer::Token { recipient, .. } = q.tx.transfer else {
            unreachable!()
        };
        let owner = q.tx.owner.base58();
        for scenario in [
            "ok",
            "wrong-genesis",
            "false",
            "revert",
            "low-trx",
            "low-usdt",
            "stale",
            "decimals",
            "permissions",
        ] {
            let mut call = |path: &str, data: Value| -> io::Result<Value> {
                Ok(match path {
                    "/wallet/getblockbynum" => {
                        json!({"blockID":if scenario=="wrong-genesis" {"bad"} else {GENESIS}})
                    }
                    "/wallet/getaccount" => {
                        json!({"address":owner,"balance":if scenario=="low-trx" {1} else {100_000_000},
                    "owner_permission":{"threshold":if scenario=="permissions" {2} else {1},"keys":[{"address":owner,"weight":1}]}})
                    }
                    "/wallet/triggerconstantcontract" => {
                        let selector = data["function_selector"].as_str().unwrap();
                        let value = match selector {
                            "decimals()" => {
                                if scenario == "decimals" {
                                    18
                                } else {
                                    6
                                }
                            }
                            "balanceOf(address)" => {
                                if scenario == "low-usdt" {
                                    0
                                } else {
                                    10_000_000
                                }
                            }
                            "transfer(address,uint256)" => {
                                if scenario == "false" {
                                    0
                                } else {
                                    1
                                }
                            }
                            _ => panic!("selector"),
                        };
                        json!({"result":{"result":true},"constant_result":[format!("{value:064x}")],"energy_used":65000,
                        "transaction":{"ret":[{"ret":if scenario=="revert" {"FAILED"} else {"SUCCESS"}}]}})
                    }
                    "/wallet/getchainparameters" => {
                        json!({"chainParameter":[{"key":"getEnergyFee","value":100},{"key":"getTransactionFee","value":1000}]})
                    }
                    "/walletsolidity/getnowblock" => {
                        json!({"blockID":format!("0000000000007b3b{}{}","b21ace8d6ac20e7e","00".repeat(16)),"block_header":{"raw_data":{"number":31547,"timestamp":if scenario=="stale" {1} else {1_800_000_000_000u64}}}})
                    }
                    _ => panic!("unexpected {path}"),
                })
            };
            let result = prepare_with(
                &owner,
                &recipient.base58(),
                "USDT\n1.234567",
                &mut call,
                1_800_000_000_000,
            );
            if scenario == "ok" {
                let value = result.unwrap();
                let parsed = Quote::parse(value["summary"].as_str().unwrap()).unwrap();
                assert_eq!(parsed.tx.fee_limit, 13_000_000);
                assert!(parsed.bandwidth_fee > 300_000);
            } else {
                assert!(result.is_err(), "{scenario}");
            }
        }
    }
    #[test]
    fn finality_requires_exact_solid_transaction_receipt_and_transfer_event() {
        let (quote, txid) = fixture();
        let Transfer::Token {
            contract,
            recipient,
            amount,
        } = quote.tx.transfer
        else {
            unreachable!()
        };
        for scenario in [
            "success",
            "missing",
            "wrong-raw",
            "wrong-log",
            "revert",
            "inconsistent",
        ] {
            let mut call = |path: &str, _: Value| -> io::Result<Value> {
                Ok(match path {
                    "/wallet/getblockbynum" => json!({"blockID":GENESIS}),
                    "/walletsolidity/gettransactionbyid" => {
                        if scenario == "missing" {
                            json!({})
                        } else {
                            json!({"txID":txid,"raw_data_hex":if scenario=="wrong-raw" {String::new()} else {hex::encode(quote.tx.raw_bytes(mainnet::policy()).unwrap())},"ret":[{"contractRet":if scenario=="revert" {"REVERT"} else {"SUCCESS"}}]})
                        }
                    }
                    "/walletsolidity/gettransactioninfobyid" => {
                        json!({"id":txid,"blockNumber":50000,"result":if scenario=="revert" || scenario=="inconsistent" {"FAILED"} else {"SUCCESS"},"receipt":{"result":if scenario=="revert" {"REVERT"} else {"SUCCESS"}},"log":[{
                    "address":hex::encode(&contract.bytes()[1..]),"topics":[hex::encode(chain_evm::keccak256(b"Transfer(address,address,uint256)")),format!("{:0>64}",hex::encode(&quote.tx.owner.bytes()[1..])),format!("{:0>64}",hex::encode(&recipient.bytes()[1..]))],
                    "data":if scenario=="wrong-log" {"00".repeat(32)} else {hex::encode(amount.to_be_bytes())}}]})
                    }
                    _ => panic!("unexpected RPC"),
                })
            };
            let r = status_with(&txid, &quote.summary().unwrap(), &mut call);
            match scenario {
                "success" => assert_eq!(r.unwrap()["state"], "finalized"),
                "missing" => assert_eq!(r.unwrap()["state"], "unknown"),
                "revert" => assert_eq!(r.unwrap()["state"], "reverted"),
                _ => assert!(r.is_err()),
            }
        }
    }
    #[test]
    #[ignore = "mainnet public read-only; no signing or broadcast"]
    fn live_tron_usdt_balance() {
        super::super::configure_proxy("127.0.0.1:1080".into()).unwrap();
        let value = balances("TJRyWwFs9wTFGZg3JbrVriFbNfCug5tDeC").unwrap();
        assert_eq!(value["contract"], USDT);
        assert_eq!(value["usdt_decimals"], 6);
        println!("{value}");
    }
}
