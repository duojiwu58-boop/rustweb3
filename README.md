# rustweb3 (Web3Rust)

Windows 多链桌面钱包，使用 Delphi VCL 界面与 Rust 密钥、签名、联网和交易日志组件。源代码以 MIT 许可证开源。

## 功能

- 单窗口切换 BNB Smart Chain、Solana 和 Tron，共用资产、发送、接收和记录导航。
- BSC：BNB / USDT 余额、发送、USDT 链上收发记录和本机交易恢复。
- Solana：SOL / USDT 余额、发送、交易查询和原交易重播。
- Tron：TRC20 USDT 余额、发送、交易查询和原交易重播；TRX 余额用于激活账户和支付手续费，不提供 TRX 发送入口。
- 三条链的 USDT 资产详情布局一致。Solana / Tron 当前仅展示最近两笔本机转账中的 USDT 记录，不含外部转入，尚不支持更早链上记录分页。
- 创建、备份与恢复钱包，同一助记词下支持最多 32 个账户。
- GUI 和 signer 使用离线 AppContainer，独立联网 worker 处理公开查询及已签名交易；加密日志支持待处理交易查询和原样重播。

项目仍在开发中，尚未完成独立安全审计。公开测试向量中的助记词、私钥和地址不是可用钱包，不要向其充值。安全边界见 [security-boundary.md](docs/security-boundary.md) 和 [隔离实现](docs/isolation-prototype.md)。

## 构建与验证

需要 Windows x64、PowerShell 7、Rust MSVC 工具链及 Visual Studio C++ Build Tools / Windows SDK。Rust 版本固定在 [rust-toolchain.toml](rust/rust-toolchain.toml)。GUI 构建另需自行安装并获得许可的 Delphi Win64 编译器；脚本默认使用 RAD Studio 23.0，可通过 `-DelphiRoot` 指定路径。仓库不包含 Delphi 工具链或构建二进制。

在项目根目录运行以下 PowerShell 命令。第一次构建需联网下载锁定的 Rust 依赖；验证脚本随后使用离线模式：

```powershell
cargo fetch --manifest-path rust/Cargo.toml --locked
cargo test --manifest-path rust/Cargo.toml --workspace --locked --offline
./scripts/verify-isolation.ps1
./scripts/verify-secure.ps1
```

每一步成功后再执行下一步。`verify-secure.ps1` 包含隔离的 ABI/VCL 自测及公开测试钱包，不广播真实交易。

需要打包本机可运行目录时，先完成主网只读验证，再打包：

```powershell
./scripts/verify-secure.ps1 -Live
./scripts/package-bnb.ps1
```

启动 `artifacts/bnb-live/wallet-launcher.exe`。请通过 launcher 启动，以建立隔离和联网进程；旧的 `Web3Rust.exe` 是早期模拟演示。

网络代理配置来自 `config/network.ini`，复制到运行目录后可按实际环境调整；当前示例为 `http://127.0.0.1:8080`，设为 `proxy=` 可直连。代理失败不会自动绕过。打包脚本保留已有运行目录的配置。

独立对照向量生成器位于 `tests/oracle-tronweb` 和 `tests/oracle-solana`，各自包含 npm 锁文件。它们仅用于公开测试数据，不是钱包运行依赖。依赖审查脚本见 `scripts/audit-dependencies.ps1`。

## 文档与目录

- 使用说明：[BSC](docs/real-bnb-test.md)、[Solana](docs/solana.md)、[Tron](docs/tron.md)。
- `desktop/`：Delphi VCL GUI、ABI 封装与自测。
- `rust/`：Rust workspace、链编码、密钥存储、签名、网络和隔离组件。
- `scripts/`：构建、验证、审查与本机打包脚本。
- `tests/`：公开向量与独立实现对照工具。
- `references/`：参考项目来源和版本清单，不包含第三方仓库快照。

部分文档保留早期原型阶段记录，应结合日期与当前代码阅读。当前功能范围以本 README 和各链使用说明为准。

## 许可证

本项目原创代码采用 [MIT](LICENSE)。第三方依赖、公开测试向量和改写测试保留各自许可证与归属，见 [THIRD-PARTY-NOTICES.md](docs/THIRD-PARTY-NOTICES.md) 和 [参考来源](references/README.md)。MIT 许可不替代这些第三方许可。

不要提交真实助记词、私钥、密码、API 凭据、钱包文件、运行配置中的凭据或个人交易数据。构建产物、运行数据、参考源码快照和安装后的依赖目录已从版本控制中排除。
