//! Fixed production asset policy shared by network validation and offline signing.
use crate::{Address, Policy};
pub const USDT: Address = Address::from_bytes([
    0x55, 0xd3, 0x98, 0x32, 0x6f, 0x99, 0x05, 0x9f, 0xf7, 0x75, 0x48, 0x52, 0x46, 0x99, 0x90, 0x27,
    0xb3, 0x19, 0x79, 0x55,
]);
pub const POLICY: Policy = Policy {
    chain_id: 56,
    token_contract: Some(USDT),
};
pub const MAX_TOKEN_GAS: u64 = 200_000;
pub fn valid_token_gas(gas: u64) -> bool {
    (21_000..=MAX_TOKEN_GAS).contains(&gas)
}
