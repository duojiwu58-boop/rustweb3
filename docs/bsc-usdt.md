# BSC USDT implementation and API verification

2026-11-04. Supported asset: Binance-Peg USDT on BSC mainnet, chain 56, contract `0x55d398326f99059fF775485246999027B3197955`, decimals 18. This is an explicit allowlist, not recognition by symbol. BNB remains the gas currency. No new dependencies or wallet format migration.

## Primary references

- [PancakeSwap maintained token list](https://raw.githubusercontent.com/pancakeswap/token-list/main/lists/pancakeswap-default.json): identifies Binance Pegged USDT, address, chain 56 and 18 decimals. Cross-checked with the indexed [BNB Chain asset record](https://explorer.bnbchain.org/asset/USDT-6D8); direct explorer fetch timed out, BscScan returned 403, so neither is claimed as a successful code inspection.
- [BNB Chain RPC endpoints](https://docs.bnbchain.org/bnb-smart-chain/developers/json_rpc/json-rpc-endpoint/): fixed existing endpoint `https://bsc-dataseed.bnbchain.org/`, chain 56. The documentation says these public endpoints disable eth_getLogs; this implementation reads logs from specific transaction receipts, and does not claim full address history.
- [Ethereum JSON-RPC](https://ethereum.org/developers/docs/apis/json-rpc/): eth_call, eth_estimateGas, eth_getBalance, eth_getTransactionCount, eth_gasPrice, eth_sendRawTransaction, eth_getTransactionReceipt, eth_getTransactionByHash, eth_getBlockByNumber.
- [ERC-20 specification](https://eips.ethereum.org/EIPS/eip-20): balanceOf, transfer returning boolean, decimals and Transfer events. False return values are rejected.

## Implementation

`chain-evm/src/bsc.rs` owns the fixed production contract policy. `wallet-services/src/usdt.rs` checks contract code presence and ABI decimals=18, decodes strict 32-byte balanceOf values, simulates the exact transfer and adds a rounded-up 20% gas margin, capped at 200000. Amounts remain uint256/string, never floating point. Token shortage and BNB gas shortage are separate failures. Native value is always zero for token calls.

Public IPC v3 adds opcode 5 (both balances, one account) and 6 (USDT preparation, sender/recipient/amount). Opcode 3 accepts canonical signed native or allowlisted USDT transfers only. Sensitive IPC v2 adds opcode 14 (profile/password/sender/recipient/amount/nonce/gas price/gas limit). Signer builds calldata locally; no arbitrary contract/calldata or approve input exists. Signed USDT summary is nine newline-separated fields: asset, chain, sender, recipient, amount, gas price, gas limit, nonce, contract. The GUI compares this exact summary before releasing bytes. Existing BNB opcode 8 and summary remain compatible.

Journal encryption, durable-before-broadcast ordering and account-wide nonce hold are reused for both assets. Replay opcode 9 returns original bytes, including after restart. Receipt success requires an exact USDT Transfer event matching the transaction's sender, recipient and amount, plus matching receipt transaction/block identity. Canonical-block and finalized checks remain in effect. RPC is still trusted; these are not light-client proofs.

The Delphi page defaults to USDT, shows both balances, offers BNB in the asset selector and displays BNB fee bounds. Changing asset/address/amount discards the quote. The local history is still the last three outgoing intents, not complete chain history.

## Verification

- `scripts/verify-m0a.ps1`: 48 Rust unit tests, 3 separate process failure tests, Clippy, Release builds, Delphi ABI and legacy GUI regression passed. The ignored lock probe is run by its parent test. Evidence: `artifacts/usdt-regression.txt`.
- `node tests/oracle-tronweb/usdt-journal.cjs`: ethers 6.17.0 independent USDT signed bytes/hash, with zero permitted network attempts. Fixture uses public zero entropy, never a user key.
- Focused tests cover strict ABI, wrong chain/decimals, insufficient token/BNB, false simulation, failed estimate, bounded gas, wrong/missing events, exact signature fixture, restart replay and cross-asset duplicate prevention.
- Contained Delphi ABI tests exercise opcode 14, receipt summary, restart replay, BNB blocked by unresolved USDT. No broadcast is invoked.
- Mainnet read-only checks verify contract presence, decimals and both balances through the GUI → broker → contained worker path. WGC/UIA visual inspection shows default USDT, both balances, and disabled send without a quote. A public RPC query transiently failed during regression; the test now retains the actual UI error text for diagnosis.

Not verified: actual funded USDT broadcast/finality, testnet broadcast loop, complete historical indexing, independent security audit, full power-loss/disk-full and multimonitor matrices. No user profile, wallet files or funds were modified by these tests.

## Normal-user startup correction

2026-11-04: user reported startup error 5. Packaged artifacts are owned by Administrators and already grant the container read/execute; ordinary desktop users have Modify but no WRITE_DAC. `Container::grant_execute` previously called SetNamedSecurityInfoW unconditionally. It now checks effective ACL rights first and skips unnecessary writes when the required rights already exist. Missing permissions still require a successful explicit grant; access errors are not ignored and the sandbox is not weakened. Launcher errors identify the failed stage instead of always suggesting another wallet is running.

Confirmed corrected package launches via the Explorer desktop shell with launcher TokenElevation=false, and the public-preview GUI appears. Clippy and the isolation probes passed, including offline access denial, online worker separation and child cleanup. No wallet files or secrets were read for this fix.
