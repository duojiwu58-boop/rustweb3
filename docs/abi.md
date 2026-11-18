# Win64 模拟 ABI v1

此页主体保留旧模拟 ABI。当前真实入口另用 `w3_secure_open/close/epoch/exchange` 和 `w3_network_open/close/exchange`，实现见 wallet-ffi/src/{secure,network}.rs 及 desktop/units/SecureWalletApi.pas。exchange 参数是 u64 epoch、u32 opcode、输入指针/usize 长度、调用方输出指针/usize 容量、usize written 输出槽；输出容量固定 2048。秘密结果不使用公共缓冲注册表，双方清除自有缓冲。secure epoch 必须等于当前会话；network 同位置参数保留且传 0。敏感错误 -31..-34，陈旧会话 -4；network 业务错误 -40 可能附有公开 JSON。新入口的隔离与协议见 isolation-prototype.md。

2026-11-04：M0a 已实现公开 ping 和无状态离线预览，不接受钱包秘密。调用端为 desktop/units/WalletApi.pas，回归入口为 desktop/AbiSmoke.dpr 和 Web3Rust.exe --self-test。业务 ABI 尚未冻结。

全部导出使用 C ABI（Delphi cdecl）。context/task 为不透明 uint64 ID，不是指针，0 无效。同一 DLL 加载周期内 ID 不复用。卸载 DLL 前必须停止其他调用、销毁全部 context、释放全部结果缓冲区。

```c
uint32_t w3_version(void); /* 1 */
int32_t w3_context_create(const uint8_t *path, uint32_t len, uint64_t *context);
int32_t w3_context_destroy(uint64_t context);
int32_t w3_context_lock(uint64_t context);
int32_t w3_task_submit(uint64_t context, const uint8_t *command, uint32_t len, uint64_t *task);
int32_t w3_task_poll(uint64_t context, uint64_t task, const uint8_t **data, uint32_t *len);
int32_t w3_task_cancel(uint64_t context, uint64_t task);
int32_t w3_buffer_free(const uint8_t *data, uint32_t len);
int32_t w3_preview_transfer(uint32_t asset, const uint8_t *recipient, uint32_t recipient_len,
  const uint8_t *amount, uint32_t amount_len, const uint8_t **data, uint32_t *len);
```

preview 是 v1 新增导出，输入为 UTF-8，地址 1–64 字节、金额 1–157 字节。asset 1/2/3/4 分别为 BSC BNB/演示 USDT、TRON TRX/演示 USDT；不是主网合约配置。返回 JSON schema_version=1、mode=demo、can_sign=false，包含规范地址和字符串金额/base_units，fee_status=unavailable。无 context、无准备令牌或秘密，不调用子进程。-10/-11/-12 分别是地址/金额/资产错误；其余错误及输出清零、释放规则同 poll，使用同一个 1024 缓冲区上限。此公开计算接口可在演示锁定时由外部调用，锁定仅清理 GUI 演示状态，不代表钱包认证。

path 是包含两个子进程 EXE 的绝对目录，UTF-8、无 NUL、1–32768 字节。create 核实文件存在并保存规范路径，但不验证签名、ACL、身份或后续替换。Delphi 从绝对路径加载 DLL，依赖查找限制到 DLL 目录和 System32；这不构成目录防篡改。

command 只允许 signer.ping / network.ping（严格 UTF-8，1–32 字节），不能附加参数。没有创建、导入、解锁、签名、广播或通用 JSON 接口。指针需有效、对齐，输出内存可独占写入，输入/输出不重叠；无法检测任意无效非空地址。

| 返回码 | 含义 |
| --- | --- |
| 0 | 成功；poll 返回非 NUL 结尾的 UTF-8 字节 |
| 1 | 尚未完成；继续轮询 |
| -1 | 参数、命令、UTF-8 或缓冲区所有权/长度错误 |
| -2 | context 不存在或已销毁 |
| -3 | task 不存在或结果已消费 |
| -4 | 任务已取消 |
| -5 | 子进程启动/管道/路径 I/O 错误 |
| -6 | 协议错误、响应不匹配或子进程异常退出 |
| -7 | 子进程响应超过两秒 |
| -8 | 资源上限；先消费/释放资源 |
| -9 | 内部错误或可捕获 panic |

create/submit 在合法输出槽中先写 0；poll 先写 null/0。poll 不等待后台任务，终态只消费一次；结果由 Rust 分配，Delphi 拷贝后调用 free，不能用 Delphi 内存管理器释放。缓冲区不归 context 所有，context 销毁后仍须释放。free(null,0) 可重复；长度不符不释放，立即重复释放返回错误。已释放地址可能被复用，调用者仍须保证每次分配只释放一次，不能检测任意陈旧指针。

每 DLL 最多 32 个 context、每 context 最多 32 个未消费任务、1024 个未释放缓冲区。达到缓冲区上限时 poll 保留任务。取消在结果消费前生效，即使工作线程已经完成；lock 取消现有模拟任务，后续公开 ping 仍可提交，不表示存在可解锁的钱包。

销毁从注册表移除 context，取消并 join 所有工作线程；每个线程在退出路径 kill/wait 子进程。入口捕获 Rust unwind；无效指针、系统进程终止、OOM/abort 不在保证内。显式退出/销毁已验证，GUI 被强杀时的 Job Object 约束尚未实现。
