# 第三方来源与归属

本项目独立编写的离线编码实现参考公开协议。chain-evm/src/tests.rs 和 chain-tron/src/tests.rs 使用并改写了 Trust Wallet Core 的公开测试数据；测试改为 Rust，并增加错误输入和恢复校验。未复制其签名实现或链接其二进制。

Trust Wallet Core：Copyright © 2017 Trust Wallet；Apache License 2.0。固定提交 2a8aa02011ec42b43689667682fa99ae905418d5，来源 https://github.com/trustwallet/wallet-core 。原始许可全文保存在 licenses/trust-wallet-core-Apache-2.0.txt。涉及测试路径列于 offline-codecs.md。

EIP-155/EIP-55 和 TRON 官方文档中的公开协议示例仅用于离线对照，具体链接见 offline-codecs.md。proto.rs 是按必要字段定义的最小互操作 schema；编号交叉核对 Trust 的 TronInternal.proto 与其引用的固定官方协议版本，不包含整个 TRON 协议或应用实现。

新增 Rust 依赖名称、固定版本、声明许可证、源码仓库、构建脚本/过程宏及本地许可证文件哈希见 dependency-inventory.json。该清单由 scripts/audit-dependencies.ps1 生成，Cargo.lock 固定下载校验和。清单不是对全部第三方源码的安全审计；实际部署时仍须随产物整理适用依赖许可证全文。
# Test-only TronWeb oracle

tests/oracle-tronweb pins TronWeb 6.5.1 (MIT) and its npm graph in package-lock.json. This offline public-vector tool is not shipped in the wallet EXE/DLL. Installed package notices remain in node_modules; the Rust inventory/audit does not cover this npm graph. The fixture uses the previously attributed public Trust Wallet test key and a fictional contract. Never fund these addresses.


保险库原型使用 bip39 2.2.0（CC0-1.0）、Argon2/XChaCha20Poly1305/getrandom/zeroize 等固定依赖，完整声明及许可证文件哈希见 dependency-inventory.json。零熵 24 词及 TREZOR seed 测试向量来自 BIP39 所引用的 Trezor/python-mnemonic 向量，也可在固定 bip39 源码测试中复核；均为公开测试材料，不能用于资金。

地址派生原型新增 bip32 0.6.0 / ripemd 0.2.0（MIT OR Apache-2.0）；许可证与哈希列入清单。tests/oracle-tronweb/hd-accounts.cjs 使用同一固定 npm lock 中的 ethers 6.17.0（MIT）和 TronWeb，生成仅包含公开测试熵与地址的对照数据，不包含用户密钥。
