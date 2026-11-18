//! Public offline preview only; no preparation token, fee estimate or signing capability.
use wallet_core::Amount;

pub const INVALID_ADDRESS: i32 = -10;
pub const INVALID_AMOUNT: i32 = -11;
pub const INVALID_ASSET: i32 = -12;

pub fn model(asset: u32, recipient: &str, amount: &str) -> Result<String, i32> {
    let (network, symbol, decimals) = match asset {
        1 => ("BSC", "BNB", 18),
        2 => ("BSC", "DEMO-USDT", 18),
        3 => ("TRON", "TRX", 6),
        4 => ("TRON", "DEMO-USDT", 6),
        _ => return Err(INVALID_ASSET),
    };
    let recipient = if asset <= 2 {
        chain_evm::Address::parse(recipient)
            .map_err(|_| INVALID_ADDRESS)?
            .checksum()
    } else {
        chain_tron::Address::parse(recipient)
            .map_err(|_| INVALID_ADDRESS)?
            .base58()
    };
    let value = Amount::parse_units(amount, decimals).map_err(|_| INVALID_AMOUNT)?;
    if value == Amount::ZERO || (asset == 3 && value.to_u64().map_or(true, |v| v > i64::MAX as u64))
    {
        return Err(INVALID_AMOUNT);
    }
    let formatted = value.format_units(decimals).map_err(|_| INVALID_AMOUNT)?;
    let units = value.format_units(0).map_err(|_| INVALID_AMOUNT)?;
    // Every inserted string is either constant, canonical hex/base58, or decimal digits.
    Ok(format!(
        r#"{{"schema_version":1,"mode":"demo","network":"{network}","asset":"{symbol}","recipient":"{recipient}","amount":"{formatted}","base_units":"{units}","fee_status":"unavailable","can_sign":false}}"#
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_uses_exact_units_and_cannot_sign() {
        let json = model(
            1,
            "0x3535353535353535353535353535353535353535",
            "0.000000000000000001",
        )
        .unwrap();
        assert!(json.contains(r#""base_units":"1""#));
        assert!(json.contains(r#""can_sign":false"#));
        assert!(json.contains(r#""fee_status":"unavailable""#));
        assert!(
            model(4, "TJRyWwFs9wTFGZg3JbrVriFbNfCug5tDeC", "1.000001")
                .unwrap()
                .contains(r#""base_units":"1000001""#)
        );
    }
    #[test]
    fn preview_rejects_wrong_chain_zero_precision_and_overflow() {
        let address = "TJRyWwFs9wTFGZg3JbrVriFbNfCug5tDeC";
        assert_eq!(model(1, address, "1"), Err(INVALID_ADDRESS));
        for amount in ["0", "-1", "1.0000001", "9223372036854.775808"] {
            assert_eq!(model(3, address, amount), Err(INVALID_AMOUNT));
        }
        assert_eq!(model(0, address, "1"), Err(INVALID_ASSET));
    }
}
