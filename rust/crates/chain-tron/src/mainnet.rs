//! Fixed mainnet TRC20 USDT policy and canonical, human-readable signing intent.
use crate::{Address, Policy, Transaction, Transfer};
use wallet_core::{Amount, Error, Result};
pub const USDT: &str = "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t";
pub const GENESIS: &str = "00000000000000001ebf88508a03865c71d452e25f4d51194196a1d22b6653dc";
pub fn policy() -> Policy {
    Policy {
        token_contract: Some(Address::parse(USDT).expect("fixed USDT")),
    }
}
pub fn amount(text: &str) -> Result<Amount> {
    let amount = Amount::parse_units(text, 6)?;
    if amount == Amount::ZERO {
        return Err(Error::InvalidAmount);
    }
    Ok(amount)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_usdt_wire_and_strict_intent() {
        let f: Vec<_> = include_str!("../../../../tests/fixtures/tron-usdt.txt")
            .lines()
            .filter(|l| !l.starts_with('#'))
            .collect();
        let bytes = hex::decode(f[4]).unwrap();
        let tx = Transaction::decode_signed(&bytes, policy()).unwrap();
        assert_eq!(tx.owner.base58(), f[0]);
        assert_eq!(hex::encode(tx.raw_bytes(policy()).unwrap()), f[2]);
        assert_eq!(hex::encode(tx.txid(policy()).unwrap()), f[3]);
        assert_eq!(
            tx.transfer,
            Transfer::Token {
                contract: policy().token_contract.unwrap(),
                recipient: Address::parse(f[1]).unwrap(),
                amount: Amount::from_u64(1_234_567)
            }
        );
        let quote = Quote {
            tx,
            bandwidth_fee: 400_000,
        };
        let summary = quote.summary().unwrap();
        let parsed = Quote::parse(&summary).unwrap();
        parsed.verify_signed(&bytes, f[3]).unwrap();
        assert!(parsed.check_fresh(1_800_000_000_000).is_ok());
        assert!(parsed.check_fresh(1_800_000_060_000).is_err());
        assert!(parsed.check_fresh(1_799_999_980_000).is_err());
        for changed in [
            summary.replace("tron-mainnet", "shasta"),
            summary.replace("1.234567", "1.234568"),
            summary.replace("30000000", "100000001"),
        ] {
            assert!(
                Quote::parse(&changed)
                    .and_then(|q| q.verify_signed(&bytes, f[3]))
                    .is_err()
            );
        }
        for bad in ["0", "-1", "1.0000001", "1e6"] {
            assert!(amount(bad).is_err());
        }
        for index in [10, bytes.len() - 1] {
            let mut bad = bytes.clone();
            bad[index] ^= 1;
            assert!(parsed.verify_signed(&bad, f[3]).is_err());
        }
    }
}
#[derive(Clone, Debug)]
pub struct Quote {
    pub tx: Transaction,
    pub bandwidth_fee: u64,
}
impl Quote {
    pub fn summary(&self) -> Result<String> {
        let Transfer::Token {
            contract,
            recipient,
            amount,
        } = self.tx.transfer
        else {
            return Err(Error::UnsupportedTransfer);
        };
        if contract != policy().token_contract.unwrap()
            || recipient == contract
            || recipient == self.tx.owner
            || recipient.bytes()[1..] == [0; 20]
            || amount == Amount::ZERO
            || !(1..=100_000_000).contains(&self.tx.fee_limit)
            || self.bandwidth_fee == 0
            || self.bandwidth_fee > 10_000_000
            || self.tx.expiration_ms.checked_sub(self.tx.timestamp_ms) != Some(60_000)
        {
            return Err(Error::InvalidEncoding);
        }
        self.tx.raw_bytes(policy())?;
        Ok(format!(
            "USDT\ntron-mainnet\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
            self.tx.owner.base58(),
            recipient.base58(),
            amount.format_units(6)?,
            hex::encode(self.tx.ref_block_bytes),
            hex::encode(self.tx.ref_block_hash),
            self.tx.timestamp_ms,
            self.tx.expiration_ms,
            self.tx.fee_limit,
            self.bandwidth_fee
        ))
    }
    pub fn parse(text: &str) -> Result<Self> {
        let f: Vec<_> = text.split('\n').collect();
        if f.len() != 11 || f[0] != "USDT" || f[1] != "tron-mainnet" {
            return Err(Error::InvalidEncoding);
        }
        let quote = Self {
            tx: Transaction {
                owner: Address::parse(f[2])?,
                transfer: Transfer::Token {
                    contract: policy().token_contract.unwrap(),
                    recipient: Address::parse(f[3])?,
                    amount: amount(f[4])?,
                },
                ref_block_bytes: hex::decode(f[5])
                    .map_err(|_| Error::InvalidEncoding)?
                    .try_into()
                    .map_err(|_| Error::InvalidEncoding)?,
                ref_block_hash: hex::decode(f[6])
                    .map_err(|_| Error::InvalidEncoding)?
                    .try_into()
                    .map_err(|_| Error::InvalidEncoding)?,
                timestamp_ms: f[7].parse().map_err(|_| Error::InvalidEncoding)?,
                expiration_ms: f[8].parse().map_err(|_| Error::InvalidEncoding)?,
                fee_limit: f[9].parse().map_err(|_| Error::InvalidEncoding)?,
            },
            bandwidth_fee: f[10].parse().map_err(|_| Error::InvalidEncoding)?,
        };
        if quote.summary()? != text {
            return Err(Error::InvalidEncoding);
        }
        Ok(quote)
    }
    pub fn verify_signed(&self, bytes: &[u8], txid: &str) -> Result<()> {
        if Transaction::decode_signed(bytes, policy())? != self.tx
            || hex::encode(self.tx.txid(policy())?) != txid
        {
            return Err(Error::InvalidSignature);
        }
        Ok(())
    }
    pub fn check_fresh(&self, now_ms: i64) -> Result<()> {
        if self.tx.timestamp_ms > now_ms + 15_000 || self.tx.expiration_ms <= now_ms + 5_000 {
            return Err(Error::InvalidEncoding);
        }
        Ok(())
    }
}
