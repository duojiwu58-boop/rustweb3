# M0a 分段验证

2026-11-04 06:40 UTC：模拟 ABI/进程生命周期和离线编码切片通过；M0a 整体仍在进行，未进入 M0b/M1。第一段的原始通过时间为 06:17 UTC。

项目根目录 PowerShell 7 执行：

```powershell
& .\scripts\verify-m0a.ps1
```

脚本使用现有 Cargo 与 Delphi 23.0，可通过 -Cargo / -DelphiRoot 指定位置。进入 rust 应用工具链锁，运行 fmt、单元测试、Clippy、三个运行产物的独立 Release 构建与 feature 树、三个离线库 Release 构建，再编译故障注入器、执行进程测试、编译运行 Delphi smoke。默认 cargo test 跳过需要故障 EXE 的测试，完整脚本显式运行，最终没有未执行的测试。

| 验证 | 实际结果 |
| --- | --- |
| fmt / Clippy -D warnings | 退出码 0 |
| Rust 单元测试 | 19 passed：原 5 个 IPC/FFI 测试，加金额 3、EVM 5、TRON 6 |
| 故障进程测试 | 3 passed：异常退出、错误 ID、超时、超量输出、取消、任务上限、失效句柄、EXE 缺失 |
| 独立 Release 构建 | 三运行产物成功，仅本地 wallet-ipc 依赖；三个离线库亦构建成功 |
| Delphi DLL 实调 | ABI SMOKE PASSED，退出码 0 |
| Delphi 细项 | 16 并发任务、两角色 UTF-8、消费后查询、取消、锁定、错误长度/立即重复释放、null、带任务销毁、10 次 DLL 重载 |
| 退出后进程 | 本项目 bin 下无遗留 signer/network |
| 已知漏洞扫描 | cargo audit 0.22.2，最终 0 漏洞、无 warning；证据见 dependency-audit.json |

金额/EVM/TRON 新增测试覆盖 uint256 边界、最小单位、错误格式、EIP-55 与 Base58Check、EIP-155 独立签名向量、TRX txID/签名独立向量、白名单 transfer、错误链/合约/签名、high-s、未知/重复 protobuf 字段、截断与越界等。详细向量来源及交叉验证范围见 offline-codecs.md。

依赖审计复跑：`& .\scripts\audit-dependencies.ps1` 使用本地公告库；`-Refresh` 通过本机代理刷新。首次候选 k256 0.13.4 的传递依赖命中 RUSTSEC-2026-0387，已升级到 k256 0.14.0 / crypto-bigint 0.7.10 后复验通过，未忽略公告。

Rust 链接器本地化“正在创建库…”被 rustc 记为 linker_messages warning，构建成功；Clippy 无警告。首次 Delphi 编译发现加载常量未声明及过程变量语法不兼容，已修正，最终编译和实调通过。

产物在 artifacts/m0a/bin；故障注入器在上级，仅作测试。重复运行更新本项目产物，不操作钱包或参考仓库。Delphi EXE 构建时间可使哈希变化；本次具体哈希在 artifacts/m0a/hashes.json。

第三段（2026-11-04）：新增 Web3Rust.exe 四个 VCL 页面、公开预览 ABI、TronWeb 6.5.1 完整 TRC-20 向量。22 单元测试、3 故障进程测试、fmt/clippy、Release、Delphi ABI 和 GUI 自测均通过；审计仍为 0 公告。GUI 在 96/120/144/192 PPI 下通过按钮 Click 验证非法精度拒绝、最小单位与完整地址预览、真实模拟 IPC、锁定清除和恢复。记录位于 artifacts/m0a/verification.txt、gui/self-test.txt。独立 oracle 再生成与复核通过，零网络尝试。

实际窗口已通过 UIA 观察资产行、页面切换及输入，WGC 截图检查资产与输入页。PaintTo 截图不能完整绘制原生控件，已移除，不能作为 DPI 视觉验收证据。UIA Invoke 的“下一步”未观察到页面变化；应用内 Button.Click 自测通过，实际点击链路仍需补充核实。实际多屏 DPI、最小窗口、系统锁屏通知尚未完整验收。WTS 锁定处理已接入，未主动锁定用户桌面。

未完成：GUI 完整验收、BIP39/BIP32、真实密钥依赖/生命周期审查、敏感 ABI、对端认证、重放/重连、强杀退出、系统隔离/防篡改、交易状态与持久恢复、实际钱包/链查询。进程拆分不是安全边界，此结果不是可用钱包。

2026-11-04 07:12 UTC 增量：新增 wallet-secrets 的 6 项测试，总计 28 单元测试 + 3 进程故障测试；fmt、Clippy、独立 Release、Delphi ABI 与 GUI 自测通过。库未链接到运行程序；保险库验证范围与未完成项见 vault-prototype.md。没有真实钱包创建或充值地址。

2026-11-04 07:24 UTC 增量：完整回归 36 单元测试 + 3 故障进程测试通过；保险库占其中 14 项，另有 1 个 ignored 探针由父测试在独立 Windows 进程实际调用。新增两链派生/恢复地址、原生转账离线签名、跨进程目录锁、暂存激活重启/篡改/不覆盖测试；ethers/TronWeb 三组独立 HD 向量复核通过且零网络尝试。仅证明离线与本机文件行为，没有真实钱包创建、主网收发或系统隔离验收。
