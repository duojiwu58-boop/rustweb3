//! Public Solana mainnet operations. No mnemonic/key access; fixed mint/programs.
use super::{Network, post};
use base64::{Engine, engine::general_purpose::STANDARD};
use chain_solana::{
    Address, Asset, GENESIS, Quote, SYSTEM_PROGRAM, TOKEN_PROGRAM, Transfer, USDT_MINT,
};
use serde_json::{Value, json};
use std::io;
use wallet_core::Amount;
fn invalid(reason: &str) -> io::Error {
    io::Error::other(format!("Solana: {reason}"))
}
fn checked<T>(result: wallet_core::Result<T>) -> io::Result<T> {
    result.map_err(|e| invalid(&e.to_string()))
}
fn rpc_on(network: Network, method: &str, params: Value) -> io::Result<Value> {
    let value = post(
        network,
        "/",
        &json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}),
    )?;
    if value["id"] != 1 || value["jsonrpc"] != "2.0" || value.get("error").is_some() {
        return Err(invalid("RPC rejected request"));
    }
    value
        .get("result")
        .cloned()
        .ok_or_else(|| invalid("RPC result missing"))
}
fn rpc(method: &str, params: Value) -> io::Result<Value> {
    rpc_on(Network::Solana, method, params)
}
fn mainnet(call: &mut impl FnMut(&str, Value) -> io::Result<Value>) -> io::Result<()> {
    if call("getGenesisHash", json!([]))? != GENESIS {
        return Err(invalid("wrong network"));
    }
    Ok(())
}
fn number(value: &Value) -> io::Result<u64> {
    value.as_u64().ok_or_else(|| invalid("invalid integer"))
}
fn format(amount: u64, decimals: u8) -> io::Result<String> {
    checked(Amount::from_u64(amount).format_units(decimals))
}
fn account(
    call: &mut impl FnMut(&str, Value) -> io::Result<Value>,
    address: Address,
) -> io::Result<Value> {
    let result = call(
        "getAccountInfo",
        json!([address.base58(),{"encoding":"jsonParsed","commitment":"confirmed"}]),
    )?;
    result
        .get("value")
        .cloned()
        .ok_or_else(|| invalid("missing account result"))
}
fn mint(call: &mut impl FnMut(&str, Value) -> io::Result<Value>) -> io::Result<()> {
    let value = account(call, checked(Address::parse(USDT_MINT))?)?;
    if value["owner"] != TOKEN_PROGRAM
        || value["executable"] != false
        || value.pointer("/data/parsed/type").and_then(Value::as_str) != Some("mint")
        || value
            .pointer("/data/parsed/info/decimals")
            .and_then(Value::as_u64)
            != Some(6)
        || value
            .pointer("/data/parsed/info/isInitialized")
            .and_then(Value::as_bool)
            != Some(true)
    {
        return Err(invalid("USDT mint identity/decimals mismatch"));
    }
    Ok(())
}
fn token_amount(value: &Value, owner: Address) -> io::Result<u64> {
    if value.is_null() {
        return Ok(0);
    }
    let info = value
        .pointer("/data/parsed/info")
        .ok_or_else(|| invalid("token data missing"))?;
    if value["owner"] != TOKEN_PROGRAM
        || value["executable"] != false
        || value.pointer("/data/parsed/type").and_then(Value::as_str) != Some("account")
        || info["mint"] != USDT_MINT
        || info["owner"] != owner.base58()
        || info["state"] != "initialized"
        || info["tokenAmount"]["decimals"] != 6
    {
        return Err(invalid("USDT account identity/state mismatch"));
    }
    let text = info["tokenAmount"]["amount"]
        .as_str()
        .ok_or_else(|| invalid("token amount missing"))?;
    checked(checked(Amount::parse_units(text, 0))?.to_u64())
}
fn sol_balance(
    call: &mut impl FnMut(&str, Value) -> io::Result<Value>,
    owner: Address,
) -> io::Result<u64> {
    number(
        &call(
            "getBalance",
            json!([owner.base58(),{"commitment":"confirmed"}]),
        )?["value"],
    )
}
pub fn balances(owner: &str) -> io::Result<Value> {
    balances_with(owner, &mut rpc)
}
fn balances_with(
    owner: &str,
    call: &mut impl FnMut(&str, Value) -> io::Result<Value>,
) -> io::Result<Value> {
    let owner = checked(Address::wallet(owner))?;
    mainnet(call)?;
    mint(call)?;
    let sol = sol_balance(call, owner)?;
    let token = account(call, checked(owner.usdt_account())?)?;
    let usdt = token_amount(&token, owner)?;
    Ok(
        json!({"network":"solana-mainnet","address":owner.base58(),"sol":format(sol,9)?,"usdt":format(usdt,6)?,"mint":USDT_MINT,"usdt_decimals":6}),
    )
}
pub fn prepare(sender: &str, recipient: &str, asset_amount: &str) -> io::Result<Value> {
    prepare_with(sender, recipient, asset_amount, &mut rpc)
}
fn prepare_with(
    sender: &str,
    recipient: &str,
    asset_amount: &str,
    call: &mut impl FnMut(&str, Value) -> io::Result<Value>,
) -> io::Result<Value> {
    let (asset, amount) = asset_amount
        .split_once('\n')
        .ok_or_else(|| invalid("asset/amount missing"))?;
    let asset = checked(Asset::parse(asset))?;
    let amount = checked(asset.amount(amount))?;
    let sender = checked(Address::wallet(sender))?;
    let recipient = checked(Address::wallet(recipient))?;
    if sender == recipient {
        return Err(invalid("recipient is current account"));
    }
    mainnet(call)?;
    let sender_info = account(call, sender)?;
    if sender_info.is_null()
        || sender_info["owner"] != SYSTEM_PROGRAM
        || sender_info["executable"] != false
    {
        return Err(invalid("sender requires a funded system wallet"));
    }
    let balance = sol_balance(call, sender)?;
    let recipient_info = account(call, recipient)?;
    if !recipient_info.is_null()
        && (recipient_info["owner"] != SYSTEM_PROGRAM || recipient_info["executable"] != false)
    {
        return Err(invalid("recipient must be a wallet address"));
    }
    let mut rent = 0;
    if asset == Asset::Usdt {
        mint(call)?;
        let source = account(call, checked(sender.usdt_account())?)?;
        if token_amount(&source, sender)? < amount {
            return Err(invalid("insufficient USDT"));
        }
        let destination = account(call, checked(recipient.usdt_account())?)?;
        if destination.is_null() {
            rent = number(&call(
                "getMinimumBalanceForRentExemption",
                json!([165,{"commitment":"confirmed"}]),
            )?)?;
        } else {
            token_amount(&destination, recipient)?;
        }
    }
    let recent = call("getLatestBlockhash", json!([{"commitment":"confirmed"}]))?;
    let blockhash = checked(Address::parse(
        recent["value"]["blockhash"].as_str().unwrap_or(""),
    ))?;
    let last_valid_height = number(&recent["value"]["lastValidBlockHeight"])?;
    let transfer = Transfer {
        sender,
        recipient,
        asset,
        amount,
        blockhash,
        create_recipient_account: rent > 0,
    };
    let message = checked(transfer.message())?;
    let fee = number(
        &call(
            "getFeeForMessage",
            json!([STANDARD.encode(message),{"commitment":"confirmed"}]),
        )?["value"],
    )?;
    let quote = Quote {
        transfer,
        last_valid_height,
        fee,
        rent,
    };
    let summary = checked(quote.summary())?;
    let needed = fee
        .checked_add(rent)
        .and_then(|n| n.checked_add(if asset == Asset::Sol { amount } else { 0 }))
        .ok_or_else(|| invalid("amount overflow"))?;
    if balance < needed {
        return Err(invalid(
            "insufficient SOL for amount, network fee and account rent",
        ));
    }
    let simulation = call(
        "simulateTransaction",
        json!([STANDARD.encode(checked(quote.transfer.unsigned())?),{"encoding":"base64","sigVerify":false,"replaceRecentBlockhash":false,"commitment":"confirmed"}]),
    )?;
    if simulation.pointer("/value/err") != Some(&Value::Null) {
        return Err(invalid("transfer simulation failed"));
    }
    Ok(
        json!({"network":"solana-mainnet","summary":summary,"fee_sol":format(fee,9)?,"rent_sol":format(rent,9)?}),
    )
}
pub fn broadcast(raw: &str, signature: &str, summary: &str) -> io::Result<Value> {
    let quote = checked(Quote::parse(summary))?;
    let signature_bytes = checked(chain_solana::signature(signature))?;
    if raw.len() > chain_solana::MAX_TRANSACTION * 2 {
        return Err(invalid("transaction size"));
    }
    let bytes = hex::decode(raw).map_err(|_| invalid("transaction hex"))?;
    if bytes != checked(quote.transfer.signed(&signature_bytes))? {
        return Err(invalid("signed transfer mismatch"));
    }
    mainnet(&mut rpc)?;
    let height = number(&rpc("getBlockHeight", json!([{"commitment":"confirmed"}]))?)?;
    if height > quote.last_valid_height {
        return Err(invalid(
            "blockhash expired; query original signature before another payment",
        ));
    }
    let result = rpc(
        "sendTransaction",
        json!([STANDARD.encode(bytes),{"encoding":"base64","skipPreflight":false,"preflightCommitment":"confirmed","maxRetries":0}]),
    )?;
    if result != signature {
        return Err(invalid("broadcast signature mismatch"));
    }
    Ok(json!({"signature":signature,"state":"unknown"}))
}
fn state_on(
    signature: &str,
    quote: &Quote,
    call: &mut impl FnMut(&str, Value) -> io::Result<Value>,
) -> io::Result<&'static str> {
    mainnet(call)?;
    let status = call(
        "getSignatureStatuses",
        json!([[signature],{"searchTransactionHistory":true}]),
    )?;
    let values = status["value"]
        .as_array()
        .filter(|a| a.len() == 1)
        .ok_or_else(|| invalid("signature status missing"))?;
    let value = &values[0];
    if !value.is_null() {
        if value["confirmationStatus"] != "finalized" {
            return Ok("unknown");
        }
        let transaction = call(
            "getTransaction",
            json!([signature,{"encoding":"base64","commitment":"finalized","maxSupportedTransactionVersion":0}]),
        )?;
        let encoded = transaction["transaction"][0]
            .as_str()
            .ok_or_else(|| invalid("final transaction missing"))?;
        let bytes = STANDARD
            .decode(encoded)
            .map_err(|_| invalid("final transaction encoding"))?;
        if bytes
            != checked(
                quote
                    .transfer
                    .signed(&checked(chain_solana::signature(signature))?),
            )?
            || transaction["slot"] != value["slot"]
            || transaction.pointer("/meta/err") != value.get("err")
        {
            return Err(invalid("final transaction evidence mismatch"));
        }
        return match value.get("err") {
            Some(Value::Null) => Ok("finalized"),
            Some(_) => Ok("reverted"),
            None => Err(invalid("missing execution result")),
        };
    }
    let height = number(&call(
        "getBlockHeight",
        json!([{"commitment":"finalized"}]),
    )?)?;
    if height <= quote.last_valid_height {
        return Ok("unknown");
    }
    let valid = call(
        "isBlockhashValid",
        json!([quote.transfer.blockhash.base58(),{"commitment":"finalized"}]),
    )?;
    if valid["value"] != false {
        return Ok("unknown");
    }
    let transaction = call(
        "getTransaction",
        json!([signature,{"encoding":"base64","commitment":"finalized","maxSupportedTransactionVersion":0}]),
    )?;
    if !transaction.is_null() {
        return Ok("unknown");
    }
    Ok("expired")
}
pub fn status(signature: &str, summary: &str) -> io::Result<Value> {
    checked(chain_solana::signature(signature))?;
    let quote = checked(Quote::parse(summary))?;
    let mut state = state_on(signature, &quote, &mut rpc)?;
    // A second fixed provider must agree before treating a never-observed payment as expired.
    if state == "expired" {
        let other = state_on(signature, &quote, &mut |method, params| {
            rpc_on(Network::SolanaSecondary, method, params)
        })?;
        if other != "expired" {
            state = "unknown";
        }
    }
    Ok(json!({"network":"solana-mainnet","signature":signature,"state":state}))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<&'static str> {
        include_str!("../../../../tests/fixtures/solana-vectors.txt")
            .lines()
            .find(|l| !l.starts_with('#'))
            .unwrap()
            .split('|')
            .collect()
    }
    #[test]
    fn preparation_validates_network_mint_owner_fee_balance_and_simulation() {
        let f = fixture();
        let sender = Address::parse(f[2]).unwrap();
        let recipient = Address::parse(f[5]).unwrap();
        for asset in ["SOL", "USDT"] {
            for fault in [
                "",
                "network",
                "mint",
                "owner",
                "balance",
                "fee",
                "simulation",
                "frozen",
            ] {
                let mut call = |method: &str, params: Value| -> io::Result<Value> {
                    Ok(match method {
                        "getGenesisHash" => json!(if fault == "network" {
                            "devnet"
                        } else {
                            GENESIS
                        }),
                        "getAccountInfo" => {
                            let address = params[0].as_str().unwrap();
                            if address == USDT_MINT {
                                json!({"value":{"owner":TOKEN_PROGRAM,"executable":false,"data":{"parsed":{"type":"mint","info":{"decimals":if fault=="mint"{9}else{6},"isInitialized":true}}}}})
                            } else if address == sender.usdt_account().unwrap().base58() {
                                json!({"value":{"owner":TOKEN_PROGRAM,"executable":false,"data":{"parsed":{"type":"account","info":{"mint":USDT_MINT,"owner":sender.base58(),"state":if fault=="frozen"{"frozen"}else{"initialized"},"tokenAmount":{"amount":"1000000000","decimals":6}}}}}})
                            } else if address == recipient.usdt_account().unwrap().base58() {
                                json!({"value":null})
                            } else {
                                json!({"value":{"owner":if fault=="owner"{TOKEN_PROGRAM}else{SYSTEM_PROGRAM},"executable":false}})
                            }
                        }
                        "getBalance" => {
                            json!({"value":if fault=="balance"{1}else{10_000_000_000u64}})
                        }
                        "getMinimumBalanceForRentExemption" => json!(2_039_280),
                        "getLatestBlockhash" => {
                            json!({"value":{"blockhash":f[6],"lastValidBlockHeight":1000}})
                        }
                        "getFeeForMessage" => json!({"value":if fault=="fee"{1_000_001}else{5000}}),
                        "simulateTransaction" => {
                            assert_eq!(params[1]["sigVerify"], false);
                            json!({"value":{"err":if fault=="simulation"{json!({"InstructionError":[0,"failure"]})}else{Value::Null}}})
                        }
                        _ => panic!("unexpected RPC {method}"),
                    })
                };
                let result = prepare_with(f[2], f[5], &format!("{asset}\n1"), &mut call);
                let pass =
                    fault.is_empty() || (asset == "SOL" && matches!(fault, "mint" | "frozen"));
                assert_eq!(result.is_ok(), pass, "{asset}/{fault}");
                if pass {
                    let result = result.unwrap();
                    let quote = Quote::parse(result["summary"].as_str().unwrap()).unwrap();
                    assert_eq!(
                        quote.transfer.amount,
                        if asset == "SOL" {
                            1_000_000_000
                        } else {
                            1_000_000
                        }
                    );
                    assert_eq!(quote.rent, if asset == "SOL" { 0 } else { 2_039_280 });
                }
            }
        }
    }
    #[test]
    fn finality_checks_exact_transaction_and_does_not_confuse_expiry_with_success() {
        let f = fixture();
        let bytes = hex::decode(f[10]).unwrap();
        let signature = bs58::encode(&bytes[1..65]).into_string();
        let quote = Quote {
            transfer: Transfer {
                sender: Address::parse(f[2]).unwrap(),
                recipient: Address::parse(f[5]).unwrap(),
                asset: Asset::Sol,
                amount: 123456789,
                blockhash: Address::parse(f[6]).unwrap(),
                create_recipient_account: false,
            },
            last_valid_height: 1000,
            fee: 5000,
            rent: 0,
        };
        for scenario in [
            "pending",
            "success",
            "failed",
            "mismatch",
            "expired",
            "still-valid",
        ] {
            let mut call = |method: &str, _: Value| -> io::Result<Value> {
                Ok(match method {
                    "getGenesisHash" => json!(GENESIS),
                    "getSignatureStatuses" => {
                        json!({"value":[if matches!(scenario,"expired"|"still-valid"){Value::Null}else{json!({"slot":900,"confirmationStatus":if scenario=="pending"{"confirmed"}else{"finalized"},"err":if scenario=="failed"{json!({"failure":true})}else{Value::Null}})}]})
                    }
                    "getTransaction" => {
                        if scenario == "expired" {
                            Value::Null
                        } else {
                            json!({"slot":900,"transaction":[STANDARD.encode(if scenario=="mismatch"{vec![1;65]}else{bytes.clone()}),"base64"],"meta":{"err":if scenario=="failed"{json!({"failure":true})}else{Value::Null}}})
                        }
                    }
                    "getBlockHeight" => json!(1001),
                    "isBlockhashValid" => json!({"value":scenario=="still-valid"}),
                    _ => panic!("unexpected {method}"),
                })
            };
            let result = state_on(&signature, &quote, &mut call);
            if scenario == "mismatch" {
                assert!(result.is_err());
            } else {
                assert_eq!(
                    result.unwrap(),
                    match scenario {
                        "success" => "finalized",
                        "failed" => "reverted",
                        "expired" => "expired",
                        _ => "unknown",
                    }
                );
            }
        }
    }
    #[test]
    #[ignore = "live read-only Solana mainnet public fixture; no broadcast"]
    fn live_public_balances() {
        super::super::configure_proxy("127.0.0.1:8080".into()).unwrap();
        let f: Vec<_> = include_str!("../../../../tests/fixtures/solana-vectors.txt")
            .lines()
            .last()
            .unwrap()
            .split('|')
            .collect();
        let value = balances(f[2]).unwrap();
        assert_eq!(value["network"], "solana-mainnet");
        assert_eq!(value["mint"], USDT_MINT);
        assert_eq!(value["usdt_decimals"], 6);
        println!("PASS: Solana mainnet genesis/mint/SOL+USDT read-only query");
    }
}
