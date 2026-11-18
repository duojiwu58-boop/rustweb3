# 当前构建依赖与待审查项

2026-11-18 Solana 增量：12 个本地 crate，Windows metadata 清单 102 个 registry package。已刷新 RustSec，`audit-dependencies.ps1 -Refresh` 返回 0 公告。新增精确版本 curve25519-dalek 4.1.3（alloc/precomputed-tables）、ed25519-dalek 2.2.0（alloc/zeroize）、hmac 0.12.1；base64 0.22.1 用于公开 RPC 编码。关闭 Dalek 默认功能，不启用 legacy_compatibility、hazmat、rand_core 或 serde 密钥导出。曲线库构建脚本只检查本地 rustc/目标架构并选择后端；派生过程宏使用已有 syn/quote。审查后更新三个依赖闭包基线；FFI/network 均不链接 wallet-secrets。

SLIP-0010 的密钥、seed 和派生材料使用 Zeroizing/SigningKey 清理；HMAC/摘要内部状态及编译器产生的副本不构成已证明的完整内存擦除。新链库公开路径仅验证签名，私钥生成/派生在离线 signer。没有完成独立密码学或全部传递依赖审计。

`tests/oracle-solana` 使用锁定的 @solana/web3.js 1.98.4、@solana/spl-token 0.4.14、bip39 3.1.0、ed25519-hd-key 1.3.0，只生成公开测试向量，不进入产品依赖；npm 使用 --ignore-scripts 安装，RustSec 检查不覆盖此测试工具闭包。36 组向量覆盖不同公开熵、账户 0/1/2、SOL、USDT 以及创建 ATA 的情形。

2026-11-04 09:12Z：当前 11 个本地 crate，Windows metadata 清单 88 个 registry package；同一 RustSec 库复查 0 公告。闭包基线见 docs/{ffi,signer,network}-dependencies.txt。signer 链接 vault/journal/sandbox，FFI 不链接 wallet-secrets，network 禁止 wallet-secrets。JSON 新增 serde_json 1.0.151、serde_core 1.0.229、itoa 1.0.18、memchr 2.8.3、zmij 1.0.23。windows-sys 0.61.2 / windows-link 0.2.1 提供系统声明，WinHTTP 仅在 services/network 中使用。

日志链接 Windows winsqlite3.dll（系统更新维护，不是 Cargo 包）；本机版本 3.53.4，SHA256 A9148FBBBB59E40E0BE60B6F0F85D286AC55A4558A8A3B1E4CCDCAC41BFACFAB。运行时校验 WAL/FULL/schema。交易载荷密钥从熵进行域分离派生，以 XChaCha20-Poly1305 加密，AAD 绑定 id/hash/summary。没有独立审计全部传递依赖、内存副本或常数时间属性。下文为历史记录，早期“未链接 EXE”状态已更新。

2026-11-04：现有八个本地 crate。Cargo.lock 固定图，rust-toolchain.toml 固定已安装的 1.97.1-x86_64-pc-windows-msvc。新增三个离线库的首次依赖下载通过本机代理完成，后续检查使用 --locked --offline。

| 产物 | Rust 依赖闭包 |
| --- | --- |
| web3rust.dll | wallet-ffi → wallet-ipc / wallet-core / chain-evm / chain-tron |
| wallet-signer.exe | wallet-signer → wallet-ipc → 标准库 |
| wallet-network.exe | wallet-network → wallet-ipc → 标准库 |

FFI 为公开地址/金额预览引入已审查离线库的依赖闭包，固定基线见 ffi-dependencies.txt，审计脚本拒绝未审查的新增/变更版本。两个模拟子进程仍仅依赖 wallet-ipc/标准库，保留独立门槛。没有 HTTP/TLS 或 wallet-secrets；没有 HTTP 依赖不等于禁止联网。Delphi 使用 Windows API 与 VCL，不依赖参考钱包二进制。

测试目录 tests/oracle-tronweb 使用 TronWeb 6.5.1（MIT），package-lock.json 固定 npm 闭包，安装使用 --ignore-scripts。它只运行公开向量；执行脚本封锁网络调用，不进入产品构建/产物。Rust cargo audit 不覆盖 npm 测试工具；测试工具尚未完成传递依赖安全审查。

实测环境：Windows 11 专业工作站版 10.0.26300 x64，PowerShell 7.6.9，rustc 1.97.1（8bab26f4f，LLVM 22.1.6），cargo 1.97.1，Delphi Win64 compiler 36.0（Studio 23.0）。均为现有安装，未安装或升级。MSVC 目标链接成功；MSVC/SDK 完整组件来源及版本清单尚未完成，当前不是可复现供应链证明。

## 离线库的固定依赖

2026-11-04 07:24 UTC 增量：wallet-secrets 新增 bip32 0.6.0（alloc/secp256k1，使用现有 k256 0.14.0）与本地 chain-evm/chain-tron；新增激活的传递库 ripemd 0.2.0。metadata 清单现为 81 项，包含解析器列出的未激活可选包。正常依赖树见 artifacts/m0a/vault-dependencies.txt：未启用 secp256k1-ffi/原生 C 构建。audit 同库复查仍 0 公告。地址派生仅在独立库，FFI/signer/network 的原依赖门槛保持通过。

局部源码审查发现 bip32 的 ConstantTimeEq 使用 self 构造 key_b；本项目不使用该比较路径，不对外暴露扩展私钥类型。固定派生流程通过独立向量，但不能因此认定其全部内部 API/内存副本已审计。生产接入前须继续审查依赖和秘密流向。

独立 wallet-secrets 原型固定 argon2 0.6.0（alloc/zeroize）、chacha20poly1305 0.11.0（alloc/zeroize）、bip39 2.2.0（alloc/zeroize，仅英文词表）、getrandom 0.4.3、zeroize 1.9.1。关闭前三者默认 feature；bip39 不启用 serde/rand。新增依赖清单纳入 metadata/audit；没有进入 EXE/DLL 的依赖闭包。新增 zeroize_derive 过程宏与 getrandom 构建入口，尚未逐行审计全部依赖；不是安全认证。getrandom 在本 Windows 目标走 ProcessPrng，无备用伪随机路径。关键内存清理审查及局限详见 vault-prototype.md。

| 依赖 | 版本 / 功能 | 用途 |
| --- | --- | --- |
| k256 | 0.14.0，关闭 defaults，仅 ecdsa | SEC1 公钥、签名恢复；签名生成只在测试代码 |
| sha3 | 0.10.8，std | Keccak-256 |
| sha2 | 0.10.9，std | TRON txID；bs58 校验和使用相同版本 |
| bs58 | 0.5.1，std/check | Base58Check |
| rlp | 0.5.2，std | EIP-155 编解码 |
| prost | 0.14.4，std/derive | 最小 TRON protobuf schema，无 protoc/build.rs 生成步骤 |
| hex | 0.4.3 | 公开地址与测试数据编码 |

直接版本精确锁定，传递版本固定于 Cargo.lock。k256 的 std 默认会启用 getrandom，因此本次仅保留 ecdsa，离线编码库不生成随机密钥。k256 使用的 SHA-256 0.11.1 与 TRON 的 0.10.9 同时存在；摘要跨边界使用固定字节数组，无混用 Digest trait。独立 wallet-secrets 原型新增 BIP39/KDF/AEAD，未接入运行进程；BIP32、SQLite 和网络客户端仍待实现与审查。

最初尝试的 k256 0.13.4 引入 crypto-bigint 0.5.5，cargo-audit 0.22.2 检出 [RUSTSEC-2026-0387](https://rustsec.org/advisories/RUSTSEC-2026-0387.html)。已替换为 k256 0.14.0 / crypto-bigint 0.7.10，并重新通过固定向量。没有忽略此公告，也没有为绕过它修改审计规则。

2026-11-04 07:12 UTC 复查（沿用同一公告库）：RustSec 数据库 commit 965bf9ad083b58e6340d65829d85217b18be9290（2026-11-03 更新，1403 条公告），cargo audit 退出 0、0 漏洞、无 informational warning。记录在 artifacts/m0a/dependency-audit.json。默认脚本使用本地数据库；加 -Refresh 才通过命令级代理刷新。没有已知公告不代表独立安全审计。

Windows metadata 清单含 74 个 registry package（lockfile 还含其他目标包），详见 dependency-inventory.json，包括声明许可证和许可证文件哈希。已检查直接依赖 manifest、feature 图和构建入口：anyhow/proc-macro2 使用本地 rustc 探测，generic-array/quote 检查编译器版本，num-traits 使用 autocfg；原有 prost-derive 为过程宏，负责本地 Rust schema 展开，不调用 protoc 或下载工具。尚未逐行审计所有传递依赖或证明常数时间属性；工具链与这些依赖属于未来密钥路径的可信计算基础。
