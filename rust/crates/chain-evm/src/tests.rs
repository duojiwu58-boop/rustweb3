use super::*;
use k256::ecdsa::SigningKey;

fn example() -> (LegacyTransfer, Policy) {
    (
        LegacyTransfer {
            chain_id: 1,
            nonce: 9,
            gas_price: Amount::from_u64(20_000_000_000),
            gas_limit: 21000,
            transfer: Transfer::Native {
                recipient: Address::parse("0x3535353535353535353535353535353535353535").unwrap(),
                amount: Amount::from_u64(1_000_000_000_000_000_000),
            },
        },
        Policy {
            chain_id: 1,
            token_contract: None,
        },
    )
}

#[test]
fn eip155_independent_signing_vector() {
    // Public EIP-155 example; never usable as a funded wallet.
    let (tx, policy) = example();
    let preimage = tx.signing_bytes(policy).unwrap();
    assert_eq!(
        hex::encode(&preimage),
        "ec098504a817c800825208943535353535353535353535353535353535353535880de0b6b3a764000080018080"
    );
    assert_eq!(
        hex::encode(tx.signing_hash(policy).unwrap()),
        "daf5a779ae972f972197303d7b574746c7ef83eadac0f2791ad23db92e4c8e53"
    );
    assert_eq!(
        LegacyTransfer::decode_signing_bytes(&preimage, policy).unwrap(),
        tx
    );
    let key = SigningKey::from_slice(&[0x46; 32]).unwrap();
    let (sig, id) = key.sign_prehash_recoverable(&tx.signing_hash(policy).unwrap());
    let mut signature = [0; 65];
    signature[..64].copy_from_slice(&sig.to_bytes());
    signature[64] = id.to_byte();
    let sender =
        Address::from_public_key(key.verifying_key().to_sec1_point(false).as_bytes()).unwrap();
    let signed = tx.signed_bytes(policy, sender, &signature).unwrap();
    assert_eq!(
        hex::encode(signed),
        "f86c098504a817c800825208943535353535353535353535353535353535353535880de0b6b3a76400008025a028ef61340bd939bc2195fe537567866003e1a15d3c71ff63e1590620aa636276a067cbe9d8997f761aecb703304b3800ccf555c9f3dc64214b297fb1966a3b6d83"
    );
    assert!(
        tx.signed_bytes(policy, Address::from_bytes([0; 20]), &signature)
            .is_err()
    );
    let changed = LegacyTransfer {
        nonce: 10,
        ..tx.clone()
    };
    assert!(changed.signed_bytes(policy, sender, &signature).is_err());
    signature[64] = 27;
    assert!(tx.signed_bytes(policy, sender, &signature).is_err());
}

#[test]
fn eip55_and_public_key_vectors() {
    for text in [
        "0x52908400098527886E0F7030069857D2E4169EE7",
        "0x8617E340B3D01FA5F11F306F4090FD50E238070D",
        "0xde709f2102306220921060314715629080e2fb77",
        "0x27b1fdb04752bbc536007a920d24acb045561c26",
        "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed",
        "0xfB6916095ca1df60bB79Ce92cE3Ea74c37c5d359",
    ] {
        assert_eq!(Address::parse(text).unwrap().checksum(), text);
    }
    assert!(Address::parse("0x5AAeb6053F3E94C9b9A09f33669435E7Ef1BeAed").is_err());
    assert!(Address::parse("0xZZ908400098527886E0F7030069857D2E4169EE7").is_err());
    assert!(Address::from_public_key(&[0; 65]).is_err());
    // Trust Wallet Core Ethereum AddressTests.cpp, Apache-2.0, public test vector.
    let key = SigningKey::from_slice(
        &hex::decode("afeefca74d9a325cf1d6b6911d61a65c32afa8e02bd5e78e2e4ac2910bab45f5").unwrap(),
    )
    .unwrap();
    assert_eq!(
        Address::from_public_key(key.verifying_key().to_sec1_point(true).as_bytes())
            .unwrap()
            .checksum(),
        "0xAc1ec44E4f0ca7D172B7803f6836De87Fb72b309"
    );
}

#[test]
fn token_transfer_is_exact_and_allowlisted() {
    let recipient = Address::from_bytes([0x35; 20]);
    let contract = Address::from_bytes([0x12; 20]);
    let amount = Amount::from_u64(1000);
    let call = encode_transfer_call(recipient, amount).unwrap();
    assert_eq!(
        hex::encode(call),
        "a9059cbb000000000000000000000000353535353535353535353535353535353535353500000000000000000000000000000000000000000000000000000000000003e8"
    );
    let (mut tx, _) = example();
    tx.chain_id = 56;
    tx.gas_limit = 65000;
    tx.transfer = Transfer::Token {
        contract,
        recipient,
        amount,
    };
    let policy = Policy {
        chain_id: 56,
        token_contract: Some(contract),
    };
    let bytes = tx.signing_bytes(policy).unwrap();
    assert_eq!(
        LegacyTransfer::decode_signing_bytes(&bytes, policy).unwrap(),
        tx
    );
    assert!(
        LegacyTransfer::decode_signing_bytes(
            &bytes,
            Policy {
                chain_id: 97,
                ..policy
            }
        )
        .is_err()
    );
    assert!(
        LegacyTransfer::decode_signing_bytes(
            &bytes,
            Policy {
                token_contract: None,
                ..policy
            }
        )
        .is_err()
    );
    for index in [0, 4, 15] {
        let mut bad = call;
        bad[index] ^= 1;
        assert!(decode_transfer_call(&bad).is_err());
    }
    let mut approval = call;
    approval[..4].copy_from_slice(&[0x09, 0x5e, 0xa7, 0xb3]);
    assert!(decode_transfer_call(&approval).is_err());
    assert!(decode_transfer_call(&[0; 69]).is_err());
    assert!(encode_transfer_call(recipient, Amount::ZERO).is_err());
}

#[test]
fn rejects_trailing_truncated_noncanonical_and_changed_fields() {
    let (tx, policy) = example();
    let bytes = tx.signing_bytes(policy).unwrap();
    for end in 0..bytes.len() {
        assert!(LegacyTransfer::decode_signing_bytes(&bytes[..end], policy).is_err());
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(LegacyTransfer::decode_signing_bytes(&trailing, policy).is_err());
    let mut signature_field = bytes.clone();
    *signature_field.last_mut().unwrap() = 1;
    assert!(LegacyTransfer::decode_signing_bytes(&signature_field, policy).is_err());
    let mut nonminimal = bytes.clone();
    nonminimal[0] += 1;
    nonminimal.insert(1, 0x81);
    assert!(LegacyTransfer::decode_signing_bytes(&nonminimal, policy).is_err());
}

#[test]
fn rejects_high_s_zero_and_invalid_recovery_signatures() {
    let mut signature = [0; 65];
    assert_eq!(
        recover_address(&[0; 32], &signature),
        Err(Error::InvalidSignature)
    );
    signature[31] = 1;
    signature[32..64].copy_from_slice(
        &hex::decode("fffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364140").unwrap(),
    );
    assert_eq!(
        recover_address(&[0; 32], &signature),
        Err(Error::InvalidSignature)
    );
    signature[64] = 2;
    assert_eq!(
        recover_address(&[0; 32], &signature),
        Err(Error::InvalidSignature)
    );
}
