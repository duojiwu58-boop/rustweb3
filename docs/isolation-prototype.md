# Windows 隔离与敏感会话

## 当前 BNB 实测入口（2026-11-04 09:12Z）

`wallet-launcher.exe` 启动 `SecureWallet.exe`；GUI/signer 使用零 capability 的 Web3Rust.Wallet.v1，network 使用独立 Web3Rust.Network.v1（仅 internetClient）。可信普通用户 broker 仅中继版本 3 限长公开请求，不代理秘密。每个网络请求启动一个 worker，45 秒上限、回复至多 2052 字节。FFI 校验公开字段、完整签名 BNB 交易及其哈希，拒绝秘密字符串伪装广播载荷。

敏感版本 2 增加操作 8 BNB 签名、9 原载荷释放、10 最近记录、11 回执观察、12 取消未签名意图、13 钱包名称列表。BNB 路径仍仅允许 chain 56 原生币、21000 gas、无 data。2026-11-04 USDT 增量增加敏感操作 14（typed USDT 签名）、公开操作 5（双余额）和 6（USDT 准备）；固定合约、18 位精度与有界 gas，详见 bsc-usdt.md。signer 独占写 SQLite：签名前保存 prepared，签名后保存加密载荷/哈希，释放前提交 broadcasting。每个地址仅一笔未解决交易，重播始终相同字节。已有钱包的日志缺失、截断或版本不符拒绝发送；回执终态与区块证据单事务保存。RPC 数据受信任，不是轻客户端证明。

`w3_secure_epoch` 及 exchange 的 epoch 参数拒绝锁定前的后台请求。切换钱包清空备份会话和转账上下文。BNB 查询、准备、签名/发送使用后台线程并核对界面代次；创建、恢复和地址验证 KDF 仍同步运行，可能短暂等待。每次签名重新输入密码，预览 30 秒有效。

已实测父启动器强杀后子进程退出、worker 嵌套启动拒绝（1816）、读取/写入/复制父进程句柄拒绝（5）、身份间文件访问拒绝；全局单实例限制同用户并发启动。IPv6 外网仍因无路由未验收。未新增防火墙规则或回环豁免。

测试身份升级为 VaultPublicTest.v2 / DesktopPublicTest.v2，与用户 Wallet.v1 分离。Delphi 实调在容器内部创建一次性随机测试钱包并验证恢复地址一致，不输出助记词；另用公开 fixture 验证签名及合成回执，未广播。VCL 锁定/缩放与异步主网只读请求通过，UIA/WGC 只检查不可签名的公开预览。

运行数据位于 `%LOCALAPPDATA%\Packages\web3rust.wallet.v1\AC`，网络身份无此目录权限。管理员、受控宿主、被篡改可信程序不在隔离保证内。Delphi/系统缓冲区不能证明完全清零。尚缺磁盘耗尽、真实断电、链接替换等完整矩阵及真钱端到端交易。操作见 [BNB 实测说明](real-bnb-test.md)。

以下为早期实现记录，旧“尚未实现”段落不代表当前入口。

2026-11-04：本轮由 Codex 直接实现 `wallet-sandbox`、signer 的专用保险库会话、敏感 ABI 和独立 VCL 创建/恢复窗口。仍未达到真实充值条件：余额/RPC、交易日志、签名/广播、完整恢复与隔离验收尚待实现。原 `Web3Rust.exe` 继续是模拟原型，新 `SecureWallet.exe` 也只作为开发验证入口。

## 已实现

`Container::open` 使用 Windows CreateAppContainerProfile 创建本项目的每用户身份，已有身份则派生 SID；没有添加防火墙规则或回环豁免。`spawn_offline` 使用零 capabilities；`spawn_internet` 只使用 internetClient。启动前读取回环豁免清单，匹配本身份则拒绝启动。仅在本项目产物目录及明确文件上添加对应身份的读取/执行权限。当前测试身份为 `Web3Rust.OfflineProbe.v1`、`Web3Rust.NetworkProbe.v1`、`Web3Rust.VaultPublicTest.v1`、`Web3Rust.DesktopPublicTest.v1`，测试资料全部公开。

子进程仅继承显式列出的标准输入/输出管道，使用 STARTUPINFOEX 的 security capabilities、handle list、job list。Job Object 的 kill-on-close 和进程数量限制在创建进程时原子绑定：单 worker 限制 1 个进程，GUI 与 signer 共享离线身份的测试限制 4 个。父端 Drop 终止并等待子进程。父进程被强杀、并发启动与恶意继承句柄仍需进一步故障验收，不能仅凭配置声称全部通过。

清理后的环境只含 Windows 用户目录、系统目录与容器临时目录；不继承代理变量、凭据或调试钩子。Windows 创建 AppContainer 子进程需要用户目录等环境变量，仅传 SystemRoot 会失败（203）；这不是降低隔离的理由。

signer 的 `--vault-session` 在读取任何请求前检查实际 token 为 AppContainer 且 capability 数量为零，保险库目录从 token 的 AppContainer SID 经系统 API 获取，不接受任意路径或环境变量作为根目录。普通进程中启动立即静默退出 2。数据实际位于该容器系统目录下 `profiles/wallet-<name>`，不是早期规划的普通 LocalAppData 目录；正式启动身份和迁移文档尚待固定。

专用协议版本 2：小端 u32 帧长（10–2048），版本 u8、操作 u8、单连接严格递增 u64 编号，字段以 u16 字节数开头；回复含同一版本/操作/编号、u16 状态和限长结果。操作 1 创建、2 重开、3 待备份词、4 确认、5 锁定、6 已激活地址、7 新目录恢复。无私钥导出、任意路径、任意哈希/消息签名。重复编号或畸形帧关闭连接；错误只返回固定状态。请求与回复 Rust 缓冲区清零；后台每 250ms 检查保险库五分钟期限。

此通道通过可信启动器创建的继承句柄限定对端，不开放可被任意客户端连接的服务端名称。尚未实现通用命名管道认证。网络容器没有获得敏感管道句柄。公开测试驱动作为可信父端会接触公开助记词；正式启动器不能代理秘密消息。

敏感 ABI 是 `w3_secure_open/close/exchange`，与公共任务和公共返回缓冲区注册表分开。调用方提供 2048 字节输出缓冲区并负责清除；Rust 接收、回复副本使用 Zeroizing。`SecureWalletApi.pas` 清除自有缓冲区，VCL 清除密码/助记词控件；Delphi 管理字符串、控件与 OS 管道仍可能存在无法证明擦除的副本，不能声称绝对内存清零。当前调用同步执行；发布前需消除 KDF 等待期间的界面阻塞。

## 已观察到的行为

`scripts/verify-isolation.ps1` 构建 Release 并运行公开探针。离线 token 的外部文件读取/写入及父进程内存访问返回错误 5；IPv4 外网返回 WSAEACCES 10013。本机 IPv4/IPv6 回环监听器在普通父进程的连接对照成功，隔离子进程连接超时且监听器未收到连接；本机代理 1080 也不可连接。IPv6 外网返回无路由 10051，不能据此声称 IPv6 外网隔离已实测通过。

离线 DNS 查询 example.com 返回 11001；联网容器同域名可解析并建立 IPv4 TCP 连接。早先使用 1.1.1.1:443 的联网正对照超时，换用同域名解析出的地址后成功。联网容器读取/写入离线容器文件及访问父进程内存返回错误 5。DNS 缓存/多协议与系统策略异常仍需扩展验收。

`vault-session-probe` 在隔离 signer 内执行 12 个有序请求：公开助记词恢复、备份读回、错误确认拒绝、正确确认、active 词导出拒绝、两目录地址一致、错误密码拒绝、锁定；地址与 ethers/TronWeb 独立 fixture 一致。只清理此次创建的两个公开测试目录的已知文件。`SecureAbiSmoke.dpr` 在独立离线容器中跑 Delphi→DLL→signer→保险库，再锁定重开核对地址；保留的 profile 仅含公开助记词。`SecureWallet.dpr --self-test` 在同隔离身份下验证 VCL 窗口、锁定清屏及缩放。

## 剩余工作

还需正式 launcher 与受限 network 的配对、两身份间的完整文件/进程/IPC 攻击矩阵、强杀/超时/重放故障验证、真实 VCL 交互/锁屏、RPC 服务和交易状态日志，以及最终转账确认、签名和广播。此文不是完整安全审计报告；禁止将公开测试向量地址当作用户收款地址。

官方依据：[启动 AppContainer](https://learn.microsoft.com/en-us/windows/win32/secauthz/implementing-an-appcontainer)、[隔离边界](https://learn.microsoft.com/en-us/windows/win32/secauthz/appcontainer-isolation)、[回环豁免查询及释放](https://learn.microsoft.com/en-us/windows/win32/api/netfw/nf-netfw-networkisolationgetappcontainerconfig)、[系统容器目录](https://learn.microsoft.com/en-us/windows/win32/api/userenv/nf-userenv-getappcontainerfolderpath)。
