use super::*;
use chain_evm::{Address, bsc, encode_transfer_call};

fn word(value: &Value) -> io::Result<Amount> {
    let text = value
        .as_str()
        .and_then(|v| v.strip_prefix("0x"))
        .filter(|v| v.len() == 64)
        .ok_or_else(|| io::Error::other("invalid ABI uint256"))?;
    let mut bytes = [0; 32];
    hex::decode_to_slice(text, &mut bytes).map_err(|_| io::Error::other("invalid ABI hex"))?;
    Ok(Amount::from_be_bytes(bytes))
}
type Rpc = dyn FnMut(&str, Value) -> io::Result<Value>;
fn token_identity(rpc: &mut Rpc) -> io::Result<()> {
    let code = rpc("eth_getCode", json!([bsc::USDT.checksum(), "latest"]))?;
    if code.as_str().is_none_or(|v| v.len() <= 2) {
        return Err(io::Error::other("USDT contract code missing"));
    }
    if word(&rpc(
        "eth_call",
        json!([{"to":bsc::USDT.checksum(),"data":"0x313ce567"},"latest"]),
    )?)? != Amount::from_u64(18)
    {
        return Err(io::Error::other("USDT decimals mismatch"));
    }
    Ok(())
}
fn token_balance(account: &str, rpc: &mut Rpc) -> io::Result<Amount> {
    let account = Address::parse(account).map_err(|_| io::Error::other("invalid account"))?;
    let data = format!("0x70a08231{:0>64}", hex::encode(account.bytes()));
    word(&rpc(
        "eth_call",
        json!([{"to":bsc::USDT.checksum(),"data":data},"latest"]),
    )?)
}
pub fn balances(account: &str) -> io::Result<Value> {
    let mut value = bnb_balance(account)?;
    token_identity(&mut bsc_read)?;
    let usdt = token_balance(account, &mut bsc_read)?;
    value["usdt_balance"] = json!(amount_text(usdt, 18)?);
    value["usdt_contract"] = json!(bsc::USDT.checksum());
    value["usdt_decimals"] = json!(18);
    Ok(value)
}
fn gas_limit(estimate: u64) -> io::Result<u64> {
    let gas = estimate
        .checked_add(estimate.div_ceil(5))
        .ok_or_else(|| io::Error::other("gas overflow"))?;
    if !bsc::valid_token_gas(estimate) || !bsc::valid_token_gas(gas) {
        return Err(io::Error::other(
            "USDT gas estimate exceeds supported limit",
        ));
    }
    Ok(gas)
}
pub fn prepare(sender: &str, recipient: &str, amount: &str) -> io::Result<Value> {
    prepare_with(sender, recipient, amount, &mut bsc_read)
}
fn prepare_with(sender: &str, recipient: &str, amount: &str, rpc: &mut Rpc) -> io::Result<Value> {
    let sender = address(sender)?;
    let recipient = Address::parse(recipient).map_err(|_| io::Error::other("invalid recipient"))?;
    if recipient.bytes() == [0; 20] || recipient == bsc::USDT {
        return Err(io::Error::other("invalid USDT recipient"));
    }
    let amount =
        Amount::parse_units(amount, 18).map_err(|_| io::Error::other("invalid USDT amount"))?;
    let call = encode_transfer_call(recipient, amount)
        .map_err(|_| io::Error::other("invalid USDT amount"))?;
    if rpc("eth_chainId", json!([]))?.as_str() != Some("0x38") {
        return Err(io::Error::other("wrong BSC chain"));
    }
    token_identity(rpc)?;
    if token_balance(&sender, rpc)? < amount {
        return Err(io::Error::other("insufficient USDT balance"));
    }
    let gas_price = quantity_u64(&rpc("eth_gasPrice", json!([]))?)?;
    if gas_price == 0 || gas_price > 1_000_000_000_000 {
        return Err(io::Error::other("gas price out of range"));
    }
    let tx = json!({"from":sender,"to":bsc::USDT.checksum(),"value":"0x0","data":format!("0x{}",hex::encode(call))});
    if word(&rpc("eth_call", json!([tx, "pending"]))?)? != Amount::from_u64(1) {
        return Err(io::Error::other("USDT transfer simulation returned false"));
    }
    let gas = gas_limit(quantity_u64(&rpc("eth_estimateGas", json!([tx]))?)?)?;
    let fee = Amount::from_u64(gas_price)
        .checked_mul_u64(gas)
        .map_err(|_| io::Error::other("fee overflow"))?;
    let balance = quantity(&rpc("eth_getBalance", json!([sender, "pending"]))?)?;
    if balance < fee {
        return Err(io::Error::other("insufficient BNB for USDT gas fee"));
    }
    let nonce = quantity_u64(&rpc("eth_getTransactionCount", json!([sender, "pending"]))?)?;
    Ok(
        json!({"chain_id":56,"asset":"USDT","contract":bsc::USDT.checksum(),"decimals":18,
        "sender":sender,"recipient":recipient.checksum(),"amount":amount_text(amount,18)?,
        "nonce":nonce.to_string(),"gas_price_wei":gas_price.to_string(),"gas_limit":gas.to_string(),"maximum_fee":amount_text(fee,18)?}),
    )
}

/// A successful receipt must contain exactly the intended allowlisted Transfer event.
pub(crate) fn verify_transfer(receipt: &Value, tx: &Value) -> io::Result<()> {
    let parse_address = |v: &Value| {
        Address::parse(v.as_str().unwrap_or("")).map_err(|_| io::Error::other("receipt address"))
    };
    if parse_address(&tx["to"])? != bsc::USDT || quantity(&tx["value"])? != Amount::ZERO {
        return Err(io::Error::other("USDT transaction identity mismatch"));
    }
    let sender = parse_address(&tx["from"])?;
    let input = tx["input"]
        .as_str()
        .and_then(|v| v.strip_prefix("0x"))
        .ok_or_else(|| io::Error::other("missing token input"))?;
    let input = hex::decode(input).map_err(|_| io::Error::other("token input hex"))?;
    let (recipient, amount) = chain_evm::decode_transfer_call(&input)
        .map_err(|_| io::Error::other("unsupported token call"))?;
    let topic = format!(
        "0x{}",
        hex::encode(chain_evm::keccak256(b"Transfer(address,address,uint256)"))
    );
    let from = format!("0x{:0>64}", hex::encode(sender.bytes()));
    let to = format!("0x{:0>64}", hex::encode(recipient.bytes()));
    let logs = receipt["logs"]
        .as_array()
        .ok_or_else(|| io::Error::other("missing token logs"))?;
    let count = logs
        .iter()
        .filter(|log| {
            parse_address(&log["address"]).ok() == Some(bsc::USDT)
                && log["removed"].as_bool() != Some(true)
                && log["topics"] == json!([topic, from, to])
                && word(&log["data"]).ok() == Some(amount)
                && log["transactionHash"] == receipt["transactionHash"]
                && log["blockHash"] == receipt["blockHash"]
        })
        .count();
    if count != 1 {
        return Err(io::Error::other(
            "USDT Transfer event mismatch; payment remains unresolved",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preparation_checks_balances_simulation_and_exact_rpc_transaction() {
        let sender = Address::from_bytes([1; 20]).checksum();
        let recipient = Address::from_bytes([2; 20]).checksum();
        for mode in [
            "ok",
            "token-short",
            "gas-short",
            "false",
            "wrong-chain",
            "wrong-decimals",
            "estimate-error",
        ] {
            let mut rpc = move |method: &str, params: Value| -> io::Result<Value> {
                Ok(match method {
                    "eth_chainId" => json!(if mode == "wrong-chain" { "0x1" } else { "0x38" }),
                    "eth_getCode" => json!("0x6000"),
                    "eth_call" => {
                        assert_eq!(params[0]["to"], bsc::USDT.checksum());
                        let data = params[0]["data"].as_str().unwrap();
                        let n = if data == "0x313ce567" {
                            if mode == "wrong-decimals" { 6 } else { 18 }
                        } else if data.starts_with("0x70a08231") {
                            if mode == "token-short" {
                                0
                            } else {
                                2_000_000_000_000_000_000u64
                            }
                        } else {
                            assert_eq!(params[0]["value"], "0x0");
                            let decoded =
                                chain_evm::decode_transfer_call(&hex::decode(&data[2..]).unwrap())
                                    .unwrap();
                            assert_eq!(decoded.0, Address::from_bytes([2; 20]));
                            assert_eq!(decoded.1, Amount::from_u64(1_000_000_000_000_000_000));
                            if mode == "false" { 0 } else { 1 }
                        };
                        json!(format!("0x{n:064x}"))
                    }
                    "eth_gasPrice" => json!("0x3b9aca00"),
                    "eth_estimateGas" => {
                        if mode == "estimate-error" {
                            return Err(io::Error::other("simulation unavailable"));
                        }
                        assert_eq!(params[0]["to"], bsc::USDT.checksum());
                        json!("0xc350")
                    }
                    "eth_getBalance" => json!(if mode == "gas-short" {
                        "0x0"
                    } else {
                        "0xde0b6b3a7640000"
                    }),
                    "eth_getTransactionCount" => json!("0x7"),
                    _ => panic!("unexpected method {method}"),
                })
            };
            let result = prepare_with(&sender, &recipient, "1", &mut rpc);
            if mode == "ok" {
                let quote = result.unwrap();
                assert_eq!(quote["gas_limit"], "60000");
                assert_eq!(quote["maximum_fee"], "0.000060000000000000");
                assert_eq!(quote["asset"], "USDT");
            } else {
                assert!(result.is_err(), "{mode}");
            }
        }
    }
    #[test]
    fn strict_abi_and_bounded_gas() {
        assert_eq!(
            word(&json!(format!("0x{:064x}", 18))).unwrap(),
            Amount::from_u64(18)
        );
        for v in [
            json!("0x12"),
            json!("0x"),
            json!(null),
            json!(format!("0x{}", "g".repeat(64))),
        ] {
            assert!(word(&v).is_err());
        }
        assert_eq!(gas_limit(50_000).unwrap(), 60_000);
        for v in [0, 20_999, 200_000, u64::MAX] {
            assert!(gas_limit(v).is_err());
        }
    }
    #[test]
    fn receipt_requires_correct_contract_sender_recipient_amount_and_event() {
        let sender = Address::from_bytes([1; 20]);
        let recipient = Address::from_bytes([2; 20]);
        let amount = Amount::from_u64(123);
        let tx = json!({"to":bsc::USDT.checksum(),"from":sender.checksum(),"value":"0x0","input":format!("0x{}",hex::encode(encode_transfer_call(recipient,amount).unwrap()))});
        let event = json!({"address":bsc::USDT.checksum(),"transactionHash":"tx","blockHash":"block","topics":[format!("0x{}",hex::encode(chain_evm::keccak256(b"Transfer(address,address,uint256)"))),format!("0x{:0>64}",hex::encode(sender.bytes())),format!("0x{:0>64}",hex::encode(recipient.bytes()))],"data":format!("0x{}",hex::encode(amount.to_be_bytes()))});
        let receipt = json!({"transactionHash":"tx","blockHash":"block","logs":[event]});
        verify_transfer(&receipt, &tx).unwrap();
        for field in ["address", "data", "transactionHash", "blockHash"] {
            let mut bad = receipt.clone();
            bad["logs"][0][field] = json!("wrong");
            assert!(verify_transfer(&bad, &tx).is_err());
        }
        let mut bad = receipt.clone();
        bad["logs"][0]["topics"][2] = json!("wrong");
        assert!(verify_transfer(&bad, &tx).is_err());
        let mut bad = receipt.clone();
        bad["logs"] = json!([]);
        assert!(verify_transfer(&bad, &tx).is_err());
        let mut bad = tx.clone();
        bad["value"] = json!("0x1");
        assert!(verify_transfer(&receipt, &bad).is_err());
    }
}
