# 本地保险库

当前已接入隔离 signer、敏感 ABI 和 VCL 创建/恢复窗口，新增 BNB 类型化签名和加密交易载荷。一次性随机测试钱包创建、确认、恢复及地址一致已通过实际 Delphi→DLL→signer 测试。没有生成用户收款钱包。详见 [隔离实现](isolation-prototype.md)；以下保留独立库阶段的实现说明和历史状态。

2026-11-04：用户授权创建新钱包，并表示准备自行转入真钱。本轮仅完成独立 `wallet-secrets` 库的代码与公开向量测试，没有生成用户钱包、助记词或可充值地址。现有 EXE/DLL/signer/network 都不依赖该库，界面仍是演示。实际使用前还缺密钥路径隔离、认证 IPC、创建/备份界面与完整异常恢复验收；两链派生、地址恢复比对和同目录独占访问已经通过本轮测试。

## 实现与边界

- `Session::create_new` 从系统随机源取得 32 字节熵，仅支持英文 24 词、空 BIP39 passphrase。没有公开传入固定熵的创建接口；测试直接使用私有函数注入公开的零熵向量。此轮未调用成功的随机钱包创建路径。
- 密码按原始字节处理，长度限定 12–1024 字节，不自动 trim 或 Unicode 转换；长度检查不代表密码强度保证，调用者负责清除输入缓冲区。
- Argon2id v19：64 MiB、3 次、1 lane，输出 32 字节；16 字节独立随机 salt。参数由格式版本固定，不从未认证文件读取任意资源预算。
- XChaCha20-Poly1305 使用 24 字节系统随机 nonce，头部全体作为 AAD。加密明文只有 32 字节熵；文件不含密码、助记词或种子。
- 内存对象没有 Debug/Display/Clone/序列化实现。熵、密钥、解密临时数据、备份字符串使用 Zeroizing，BIP39 和加密库启用 zeroize。特别检查到 Argon2 0.6.0 默认分配的工作区释放时不擦除，因此使用自有 Zeroizing 工作区及 `hash_password_into_with_memory`。这不证明系统分页文件、运行库/寄存器临时副本或崩溃转储完全不留痕。
- 会话持有秘密最多供调用检查的 5 分钟；到期后的下一次使用会清除并拒绝。`lock` 立即清除。没有后台定时擦除、IPC 身份绑定或自动监听锁屏。已经返回给调用者的备份字符串须由调用者及时销毁，锁定不能撤回外部副本。

## 格式与保存

固定 97 字节：8 字节 `W3VAULT1`；1 字节阶段（1 pending_backup / 2 active）；16 字节 salt；24 字节 nonce；32 字节密文和16 字节认证 tag。长度、版本、阶段在 KDF 前检查；头部和密文均纳入认证。密码错误与认证失败不写入文件。读取只分配至多 98 字节。

创建要求全新绝对路径目录。`create_dir` 和 `create_new` 拒绝已有目录/文件，不覆盖甚至空目录。先写 `pending.vault` 并 `sync_all` 成功，才返回可以显示备份的会话。确认完整备份词匹配后，以新的 nonce 写 `activation.next` 并 sync_all，再用不带 REPLACE_EXISTING 的 MoveFileExW 同目录重命名为 `active.vault`，返回成功才更新内存阶段。pending 加密文件保留，便于后续显式恢复工具使用，不是普通缓存。

重开时只要 active 存在就选择 active；损坏/截断时拒绝，不静默退回 pending。写入中断可能留下不完整文件或空目录；程序不会删除或覆盖。首版原型因此可能需要显式人工恢复，已有不覆盖目标的暂存提交；损坏文件仍需显式恢复工具，不自动删除或替换。sync_all 验证不是断电持久化证明，尚未验证文件系统/存储设备断电行为。

active 会话不提供备份显示；旧 pending 会话显示前也检查 active 文件存在。现通过 profile.lock 的 Windows share_mode(0) 持有独占文件句柄，阻止同一目录的第二个合作进程进入；lock 清除秘密但独占句柄保留到 Session 销毁。此锁不是针对恶意进程的权限边界，也不是跨不同 profile 的同一地址互斥，不宣称防御用户回滚旧备份或恶意替换父目录。目录 ACL、reparse point/路径替换防护必须在 signer 接入前落实。

`restore_new` 仅支持本原型的英文 24 词、空 passphrase，恢复到全新 profile 并重新要求备份确认。现已验证原钱包、重启与备份恢复后的 BSC/TRON 地址一致，并通过派生密钥的离线签名校验。只有独立库的密码认证地址查询 API；没有产品收款入口、真实签名服务或广播能力。

## 验证

`cargo test -p wallet-secrets --release --locked --offline`：14 个测试通过（另有 1 个由父测试自动调用的子进程探针，普通枚举标为 ignored），覆盖公开 BIP39 零熵/助记词/TREZOR seed 向量、重启保留同一备份、active 拒绝显示、跨进程独占、错误密码、头部/密文/tag 篡改、截断/超长、部分 active 文件、拒绝覆盖、恢复和锁定/超时清除。临时文件仅含公开测试熵的加密形式，测试按确切文件名清理，无递归删除。生产参数参与测试，没有降低 KDF 成本。

资料：[BIP39](https://bips.dev/39/)、[Argon2 API](https://docs.rs/argon2/0.6.0/argon2/)、[XChaCha20Poly1305](https://docs.rs/chacha20poly1305/0.11.0/chacha20poly1305/)、[getrandom Windows 后端](https://docs.rs/getrandom/0.4.3/getrandom/)。库版本、feature、构建脚本与许可清单见 dependencies.md 和 dependency-inventory.json。

当前原型没有签名用途，激活成功立即清除内存熵和派生密钥；重新打开 active 文件只验证密码/认证并返回阶段，不保留解密材料。

## 地址派生与新增故障验证

固定 BIP32/bip32 0.6.0 与 k256 0.14.0，BSC 路径 m/44'/60'/0'/0/0，TRON 路径 m/44'/195'/0'/0/0。W3VAULT1 只支持账户 0 和空 BIP39 passphrase；不会自动猜测其他钱包的路径。地址本身不包含主网/测试网身份，路径也不能替代可信链配置。

`Session::account_addresses` 获取同目录独占锁，要求 active 文件并重新验证密码，返回两链地址和路径；不返回私钥、种子或扩展密钥。pending、错误密码、目录已被占用均拒绝。种子使用 Zeroizing；派生密钥仅在函数内借用，返回后销毁。已有派生库内部临时副本/chain code 的擦除不能完整保证，仍须纳入密钥路径审查。

独立 oracle：`node tests/oracle-tronweb/hd-accounts.cjs`，复核 ethers 6.17.0 与 TronWeb 6.5.1 的三组公开熵向量，整个脚本禁止联网。Rust 同时验证从原 profile 和恢复 profile 得到相同地址、以对应派生密钥签署 BSC/TRX 原生转账后可恢复预期 owner。交易使用历史/虚构参数、从未广播；不等于真实链可用性验收。

完整暂存文件可在重启后继续：用户重新确认备份，程序验证暂存记录的认证标签、salt、阶段及熵与当前 pending 一致，重新 flush 后再提交。部分/不匹配暂存文件拒绝提交并保留 pending；已有 active 永不覆盖。新增测试覆盖这些路径和实际 Windows 跨进程锁。尚未做强杀/断电矩阵、恶意路径/ACL 测试，不能据此宣称系统隔离已完成。

实现依据：[BIP32 API](https://docs.rs/bip32/0.6.0/bip32/type.XPrv.html)、[派生路径表](https://docs.trezor.io/trezor-firmware/misc/coins-bip44-paths.html)、[Windows 文件共享模式](https://doc.rust-lang.org/std/os/windows/fs/trait.OpenOptionsExt.html)、[MoveFileExW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw)。
