//! Indexed addresses from one mnemonic (English, empty passphrase).
use crate::{Error, Result};
use bip32::{DerivationPath, XPrv};
use bip39::Mnemonic;
use zeroize::Zeroizing;

pub const BSC_PATH: &str = "m/44'/60'/0'/0/0";
pub const TRON_PATH: &str = "m/44'/195'/0'/0/0";

/// Public data only. Network selection does not change these address strings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountAddresses {
    pub bsc: String,
    pub tron: String,
    pub bsc_path: String,
    pub tron_path: String,
}

pub(crate) fn from_entropy(entropy: &[u8]) -> Result<AccountAddresses> {
    from_entropy_at(entropy, 0)
}
pub(crate) fn from_entropy_at(entropy: &[u8], index: u32) -> Result<AccountAddresses> {
    if index >= crate::MAX_ACCOUNTS { return Err(Error::InvalidFormat); }
    let mnemonic = Mnemonic::from_entropy(entropy).map_err(|_| Error::Crypto)?;
    let seed = Zeroizing::new(mnemonic.to_seed_normalized(""));
    let bsc_path = format!("m/44'/60'/0'/0/{index}");
    let tron_path = format!("m/44'/195'/0'/0/{index}");
    let bsc = derive(&seed, &bsc_path)?;
    let tron = derive(&seed, &tron_path)?;
    let bsc_public = bsc.private_key().verifying_key().to_sec1_point(false);
    let tron_public = tron.private_key().verifying_key().to_sec1_point(false);
    Ok(AccountAddresses {
        bsc: chain_evm::Address::from_public_key(bsc_public.as_bytes())
            .map_err(|_| Error::Crypto)?
            .checksum(),
        tron: chain_tron::Address::from_public_key(tron_public.as_bytes())
            .map_err(|_| Error::Crypto)?
            .base58(),
        bsc_path,
        tron_path,
    })
}

pub(crate) fn derive(seed: &[u8; 64], path: &str) -> Result<XPrv> {
    let path: DerivationPath = path.parse().map_err(|_| Error::Crypto)?;
    XPrv::derive_from_path(seed, &path).map_err(|_| Error::Crypto)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn indexed_accounts_match_offline_ethers_and_tronweb() {
        for line in include_str!("../../../../tests/fixtures/hd-multi-accounts.txt").lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
            let fields: Vec<_> = line.split('|').collect();
            let account = from_entropy_at(&hex::decode(fields[0]).unwrap(), fields[1].parse().unwrap()).unwrap();
            assert_eq!(account.bsc, fields[2]); assert_eq!(account.tron, fields[3]);
        }
        assert!(from_entropy_at(&[0;32], crate::MAX_ACCOUNTS).is_err());
    }
    #[test]
    fn independent_ethers_tronweb_public_vectors() {
        let fixture = include_str!("../../../../tests/fixtures/hd-accounts.txt");
        for line in fixture
            .lines()
            .filter(|line| !line.starts_with('#') && !line.is_empty())
        {
            let fields: Vec<_> = line.split('|').collect();
            assert_eq!(fields.len(), 3);
            let entropy: [u8; 32] = hex::decode(fields[0]).unwrap().try_into().unwrap();
            let addresses = from_entropy(&entropy).unwrap();
            assert_eq!(addresses.bsc, fields[1]);
            assert_eq!(addresses.tron, fields[2]);
        }
    }

    #[test]
    fn derived_keys_sign_native_transfers_for_the_expected_accounts() {
        use wallet_core::Amount;
        let mnemonic = Mnemonic::from_entropy(&[0; 32]).unwrap();
        let seed = Zeroizing::new(mnemonic.to_seed_normalized(""));
        let accounts = from_entropy(&[0; 32]).unwrap();
        let bsc_key = derive(&seed, BSC_PATH).unwrap();
        let sender = chain_evm::Address::parse(&accounts.bsc).unwrap();
        let policy = chain_evm::Policy {
            chain_id: 56,
            token_contract: None,
        };
        let tx = chain_evm::LegacyTransfer {
            chain_id: 56,
            nonce: 0,
            gas_price: Amount::from_u64(1_000_000_000),
            gas_limit: 21000,
            transfer: chain_evm::Transfer::Native {
                recipient: sender,
                amount: Amount::from_u64(1),
            },
        };
        let hash = tx.signing_hash(policy).unwrap();
        let (sig, id) = bsc_key.private_key().sign_prehash_recoverable(&hash);
        let mut signature = [0; 65];
        signature[..64].copy_from_slice(&sig.to_bytes());
        signature[64] = id.to_byte();
        assert_eq!(
            chain_evm::recover_address(&hash, &signature).unwrap(),
            sender
        );
        assert!(tx.signed_bytes(policy, sender, &signature).is_ok());
        let tron_key = derive(&seed, TRON_PATH).unwrap();
        let owner = chain_tron::Address::parse(&accounts.tron).unwrap();
        let policy = chain_tron::Policy {
            token_contract: None,
        };
        let tx = chain_tron::Transaction {
            owner,
            ref_block_bytes: [0x7b, 0x3b],
            ref_block_hash: [1; 8],
            timestamp_ms: 1_539_295_479_000,
            expiration_ms: 1_539_295_539_000,
            fee_limit: 0,
            transfer: chain_tron::Transfer::Native {
                recipient: owner,
                amount: Amount::from_u64(1),
            },
        };
        let (sig, id) = tron_key
            .private_key()
            .sign_prehash_recoverable(&tx.txid(policy).unwrap());
        signature[..64].copy_from_slice(&sig.to_bytes());
        signature[64] = id.to_byte();
        // signed_bytes itself verifies the recovered signer matches owner.
        assert!(tx.signed_bytes(policy, &signature).is_ok());
        assert!(
            chain_tron::Transaction {
                timestamp_ms: tx.timestamp_ms + 1,
                ..tx
            }
            .signed_bytes(policy, &signature)
            .is_err()
        );
    }
}
