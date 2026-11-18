use super::*;
use k256::ecdsa::SigningKey;

fn example() -> (Transaction, Policy) {
    (
        Transaction {
            owner: Address::parse("TJRyWwFs9wTFGZg3JbrVriFbNfCug5tDeC").unwrap(),
            ref_block_bytes: [0x7b, 0x3b],
            ref_block_hash: hex::decode("b21ace8d6ac20e7e").unwrap().try_into().unwrap(),
            timestamp_ms: 1_539_295_479_000,
            expiration_ms: 1_539_331_479_000,
            fee_limit: 0,
            transfer: Transfer::Native {
                recipient: Address::parse("THTR75o8xXAgCTQqpiot2AFRAjvW1tSbVV").unwrap(),
                amount: Amount::from_u64(2_000_000),
            },
        },
        Policy {
            token_contract: None,
        },
    )
}
#[test]
fn trust_wallet_native_signature_and_txid_vector() {
    // Public data from Trust Wallet Core TronSigner.SignTransfer, Apache-2.0.
    let (tx, policy) = example();
    let raw = tx.raw_bytes(policy).unwrap();
    assert_eq!(
        hex::encode(tx.txid(policy).unwrap()),
        "dc6f6d9325ee44ab3c00528472be16e1572ab076aa161ccd12515029869d0451"
    );
    assert_eq!(Transaction::decode_raw(&raw, policy).unwrap(), tx);
    let key = SigningKey::from_slice(
        &hex::decode("2d8f68944bdbfbc0769542fba8fc2d2a3de67393334471624364c7006da2aa54").unwrap(),
    )
    .unwrap();
    assert_eq!(
        Address::from_public_key(key.verifying_key().to_sec1_point(false).as_bytes()).unwrap(),
        tx.owner
    );
    let (sig, id) = key.sign_prehash_recoverable(&tx.txid(policy).unwrap());
    let mut signature = [0; 65];
    signature[..64].copy_from_slice(&sig.to_bytes());
    signature[64] = id.to_byte();
    assert_eq!(
        hex::encode(signature),
        "ede769f6df28aefe6a846be169958c155e23e7e5c9621d2e8dce1719b4d952b63e8a8bf9f00e41204ac1bf69b1a663dacdf764367e48e4a5afcd6b055a747fb200"
    );
    let signed = tx.signed_bytes(policy, &signature).unwrap();
    let envelope: proto::Signed = strict_decode(&signed).unwrap();
    assert_eq!(envelope.raw.unwrap().encode_to_vec(), raw);
    assert_eq!(envelope.signatures, vec![signature.to_vec()]);
    let changed = Transaction {
        timestamp_ms: tx.timestamp_ms + 1,
        ..tx.clone()
    };
    assert!(changed.signed_bytes(policy, &signature).is_err());
}

#[test]
fn official_public_key_and_base58_vectors() {
    let public = hex::decode("045f81956d5826bad7d30daed2b5c8c98e72046c1ec8323da336445476183fb7ca54ba511b8b782bc5085962412e8b9879496e3b60bebee7c36987d1d5848b9a50").unwrap();
    let address = Address::from_public_key(&public).unwrap();
    assert_eq!(
        hex::encode(address.bytes()),
        "412a260a110bc7b03f19c40a0bd04ff2c5dcb57594"
    );
    assert_eq!(address.base58(), "TDp4x8kCJ6aHz8wnqZgkaVQAm2ajH1og3L");
    assert_eq!(Address::parse(&address.base58()).unwrap(), address);
    assert_eq!(
        hex::encode(
            Address::parse("TNPeeaaFB7K9cmo4uQpcU32zGK8G1NYqeL")
                .unwrap()
                .bytes()
        ),
        "418840e6c55b9ada326d211d818c34a994aeced808"
    );
    for bad in [
        "TDp4x8kCJ6aHz8wnqZgkaVQAm2ajH1og3M",
        "TDp4x8kCJ6aHz8wnqZgkaVQAm2ajH1og30",
        "abc",
        "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed",
    ] {
        assert!(Address::parse(bad).is_err());
    }
    assert!(Address::from_bytes(&[0; 21]).is_err());
}

fn token_example() -> (Transaction, Policy) {
    let (mut tx, _) = example();
    // A fictional allowlist entry, NOT a declaration that this address is USDT.
    let contract = Address::parse("THTR75o8xXAgCTQqpiot2AFRAjvW1tSbVV").unwrap();
    tx.transfer = Transfer::Token {
        contract,
        recipient: Address::parse("TW1dU4L3eNm7Lw8WvieLKEHpXWAussRG9Z").unwrap(),
        amount: Amount::from_u64(1000),
    };
    tx.fee_limit = 100_000_000;
    (
        tx,
        Policy {
            token_contract: Some(contract),
        },
    )
}
#[test]
fn tronweb_independent_signed_trc20_vector() {
    let fixture = include_str!("../../../../tests/fixtures/trc20-tronweb-6.5.1.txt");
    let value = |name: &str| {
        fixture
            .lines()
            .filter_map(|line| line.split_once('='))
            .find(|(key, _)| *key == name)
            .unwrap()
            .1
    };
    let (tx, policy) = token_example();
    assert_eq!(
        hex::encode(tx.raw_bytes(policy).unwrap()),
        value("raw_data_hex")
    );
    assert_eq!(hex::encode(tx.txid(policy).unwrap()), value("txid"));
    let key = SigningKey::from_slice(
        &hex::decode("2d8f68944bdbfbc0769542fba8fc2d2a3de67393334471624364c7006da2aa54").unwrap(),
    )
    .unwrap();
    let (sig, id) = key.sign_prehash_recoverable(&tx.txid(policy).unwrap());
    let mut signature = [0; 65];
    signature[..64].copy_from_slice(&sig.to_bytes());
    signature[64] = id.to_byte();
    assert_eq!(hex::encode(signature), value("signature"));
    assert_eq!(
        hex::encode(tx.signed_bytes(policy, &signature).unwrap()),
        value("signed_hex")
    );
    let changed = Transaction {
        fee_limit: tx.fee_limit + 1,
        ..tx
    };
    assert!(changed.signed_bytes(policy, &signature).is_err());
}
#[test]
fn trc20_uses_20_byte_abi_address_and_whitelist() {
    let (tx, policy) = token_example();
    let raw = tx.raw_bytes(policy).unwrap();
    let parsed: proto::Raw = strict_decode(&raw).unwrap();
    let trigger: proto::Trigger =
        strict_decode(&parsed.contracts[0].parameter.as_ref().unwrap().value).unwrap();
    assert_eq!(
        hex::encode(trigger.data),
        "a9059cbb000000000000000000000000dbd7c53729b3310e1843083000fa84abad99696100000000000000000000000000000000000000000000000000000000000003e8"
    );
    assert_eq!(Transaction::decode_raw(&raw, policy).unwrap(), tx);
    assert!(
        Transaction::decode_raw(
            &raw,
            Policy {
                token_contract: None
            }
        )
        .is_err()
    );
    assert!(
        Transaction { fee_limit: 0, ..tx }
            .raw_bytes(policy)
            .is_err()
    );
}

#[test]
fn rejects_unknown_fields_permissions_duplicate_contracts_and_malformed_wire() {
    let (tx, policy) = example();
    let bytes = tx.raw_bytes(policy).unwrap();
    for end in 0..bytes.len() {
        assert!(Transaction::decode_raw(&bytes[..end], policy).is_err());
    }
    for extra in [&[0x50, 0x00][..], &[0x70, 0x01][..], &[0xff][..]] {
        let mut raw = bytes.clone();
        raw.extend(extra);
        assert!(Transaction::decode_raw(&raw, policy).is_err());
    }
    let mut raw = tx.raw(policy).unwrap();
    raw.contracts.push(raw.contracts[0].clone());
    assert!(Transaction::decode_raw(&raw.encode_to_vec(), policy).is_err());
    let mut raw = tx.raw(policy).unwrap();
    raw.contracts[0].kind = 46; // AccountPermissionUpdateContract
    assert!(Transaction::decode_raw(&raw.encode_to_vec(), policy).is_err());
    // Unknown field in native Any.value must not disappear unnoticed.
    let mut raw = tx.raw(policy).unwrap();
    raw.contracts[0]
        .parameter
        .as_mut()
        .unwrap()
        .value
        .extend([0x20, 0x01]);
    assert!(Transaction::decode_raw(&raw.encode_to_vec(), policy).is_err());
    // Explicit permission_id=0 in Contract is forbidden too.
    let mut contract = tx.raw(policy).unwrap().contracts[0].encode_to_vec();
    contract.extend([0x28, 0]);
    assert!(strict_decode::<proto::Contract>(&contract).is_err());
}

#[test]
fn rejects_approval_extra_value_and_noncanonical_tron_abi_prefix() {
    let (tx, policy) = token_example();
    for mode in 0..3 {
        let mut raw = tx.raw(policy).unwrap();
        let any = raw.contracts[0].parameter.as_mut().unwrap();
        let mut trigger: proto::Trigger = strict_decode(&any.value).unwrap();
        if mode == 0 {
            trigger.data[..4].copy_from_slice(&[0x09, 0x5e, 0xa7, 0xb3]);
        }
        if mode == 1 {
            trigger.data[15] = 0x41;
        }
        any.value = trigger.encode_to_vec();
        if mode == 2 {
            any.value.extend([0x18, 1]);
        } // call_value=1
        assert!(Transaction::decode_raw(&raw.encode_to_vec(), policy).is_err());
    }
}

#[test]
fn bounds_amount_expiration_and_native_fee() {
    let (tx, policy) = example();
    for expiration in [
        tx.timestamp_ms,
        tx.timestamp_ms - 1,
        tx.timestamp_ms + 86_400_001,
        i64::MIN,
    ] {
        assert!(
            Transaction {
                expiration_ms: expiration,
                ..tx.clone()
            }
            .raw_bytes(policy)
            .is_err()
        );
    }
    assert!(
        Transaction {
            fee_limit: 1,
            ..tx.clone()
        }
        .raw_bytes(policy)
        .is_err()
    );
    for amount in [
        Amount::ZERO,
        Amount::from_u64(i64::MAX as u64 + 1),
        Amount::from_be_bytes([255; 32]),
    ] {
        assert!(
            Transaction {
                transfer: Transfer::Native {
                    recipient: tx.owner,
                    amount
                },
                ..tx.clone()
            }
            .raw_bytes(policy)
            .is_err()
        );
    }
}
