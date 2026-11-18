# 参考源码初查（M0a）

2026-11-04 再次核对两个参考仓库 HEAD 与 references/manifest.json 一致，工作区干净。本轮没有修改、编译、安装或复制参考源码到产品。

| 参考范围 | 观察 | 取舍 |
| --- | --- | --- |
| Gem gem_evm/Cargo.toml 与 src/signer/transaction.rs | RPC/indexer/signer feature 分开；原生和 ERC20 transfer 分别构造 to/value/input，金额用 U256 | 借鉴显式模型，须独立白名单校验与解析 |
| Gem gem_tron/Cargo.toml 与 src/signer/transaction.rs | Transfer 和 TriggerSmart 分开，还有 approval/swap 入口 | 仅允许两种转账，其他路径不复用 |
| Gem signer/Cargo.toml | k256、ed25519-dalek、zeroize 及 primitives | 不整体引入多链 signer；底层实现待审查 |
| Gem chain_traits/Cargo.toml | async-trait、futures 与共享 primitives | 参考查询职责，不把 RPC 引入密钥进程 |
| Trust tests/chains/Ethereum/AddressTests.cpp | EIP55、无效地址、公钥派生用例 | 独立对照候选 |
| Trust tests/chains/Tron/AddressTests.cpp | Base58、公钥派生、错误公钥类型用例 | 独立对照候选 |

Gem 提交 9026429adaea5f02590073f0f157193674aa5288：顶层 GPL-3.0，core MIT。Trust 提交 2a8aa02011ec42b43689667682fa99ae905418d5：上述测试带 Apache-2.0 SPDX。许可证哈希沿用来源清单，尚未完成所有候选模块、传递依赖与资源审查。

第二段已阅读 Trust Tron SignerTests、SerializationTests 和 TronInternal.proto，完成公开地址、EIP-155 和 TRX txID/签名对照，详细结果见 offline-codecs.md。签名生成仅存在测试模块，运行 signer 仍只回复 ping。

发现 Trust 的 SignTransferTrc20Contract 示例把 0x41 放入 ABI 地址 word。TRON 官方 Encoding 文档说明存在这种历史兼容形式，但标准工具应使用去掉前缀后的 20 字节。本项目采用标准形式，并将 21 字节形式作为拒绝测试；不直接照搬该 TRC-20 完整签名向量。TRC-20 完整已签载荷的独立交叉验证仍未完成。

provider/rpc 和 chain_traits 具体实现、BIP39/BIP32 与保险库依赖仍待审查。本页不是 M0a 完整验收。
