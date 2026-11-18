# 参考源码清单

本目录为用户授权下载的只读研究快照。主开发项目不应将这些嵌套仓库作为产品源码整体提交或打包。克隆内容未运行、未安装依赖、未构建、未初始化子模块，也未完成安全审计。精确提交、来源和许可证文件 SHA256 见 [manifest.json](manifest.json)。

| 本地目录 | 官方来源 | 固定提交 | 本轮用途 |
| --- | --- | --- | --- |
| gem-wallet | https://github.com/gemwalletcom/wallet | 9026429adaea5f02590073f0f157193674aa5288 | Rust 链适配、签名、原生界面与核心分层 |
| trust-wallet-core | https://github.com/trustwallet/wallet-core | 2a8aa02011ec42b43689667682fa99ae905418d5 | Ethereum/TRON 地址、交易与测试向量，C ABI |

Gem Wallet 顶层 LICENSE 是 GPL-3.0，但 core/LICENSE 是 MIT，core/Cargo.toml 的 workspace license 同样为 MIT。实际复用仍需逐模块、文件及依赖核对；不因核心采用 MIT 而把移动端界面也当 MIT 使用。Trust Wallet Core 的 LICENSE 为 Apache-2.0，另有 LICENSE-3RD-PARTY.txt。

优先阅读：

- gem-wallet/core/crates/gem_evm/src：provider、rpc、signer、fee_calculator.rs。
- gem-wallet/core/crates/gem_tron/src：provider、rpc、signer、transaction、trc20.rs。
- gem-wallet/core/crates/chain_traits 与 gem-wallet/core/crates/signer。
- trust-wallet-core/src/Ethereum、src/Tron、tests/chains/Ethereum、tests/chains/Tron。
- trust-wallet-core/include/TrustWalletCore。

下载方式为 git clone --depth 1，命令级 HTTP/HTTPS 代理 http://127.0.0.1:1080，未修改全局 Git 代理。两仓库下载后均核对 origin、HEAD 和干净工作区。后续更新应保留旧提交证据并重新审查差异，不自动追踪上游最新版本。

补充在线参考（本轮未下载）：[OKX SDK](https://github.com/okx/go-wallet-sdk)、[Rabby](https://github.com/RabbyHub/Rabby)、[TronLink Core](https://github.com/TronLink/tronlink-extension-core)。