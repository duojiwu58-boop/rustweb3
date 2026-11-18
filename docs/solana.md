# Solana SOL / USDT

运行 `artifacts/bnb-live/wallet-launcher.exe`，解锁已有钱包后在顶部网络下拉框选择“Solana”。通过同一下拉框切回 BSC 或 Tron，不另开窗口。账户编号沿用钱包当前选择；各链的地址、余额与交易记录各自独立。

首页点击“接收”显示 Solana 收款地址，SOL 与 Solana USDT 共用这个钱包地址。提现时必须选择 Solana 网络。USDT 固定为 Tether 的主网 mint `Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB`，6 位小数；SOL 为 9 位。当前仅管理该钱包的标准关联代币账户（ATA），不扫描其他 SPL 账户、不支持任意代币、Token-2022、跨链或额外 memo。

发送页选择资产、输入接收方钱包地址及数量，点击“核对转账”。程序查询余额、费用、近期区块哈希并模拟交易，核对页列出金额、完整地址、SOL 手续费，以及必要时为接收方创建 USDT 账户的 SOL 费用。核对信息 20 秒后失效；发送需要再次明确确认。USDT 转账也需要余额足够的 SOL。首版不增加优先费。

“本机转账”显示最近两笔，未解决的交易优先。发送超时不代表失败；先刷新记录并查询原交易。“重播原交易”只能发送已持久保存的相同签名字节，不生成新的付款。在原交易得到最终确认、执行失败或经两个固定 RPC 共同核查过期前，同一 Solana 地址不能签下一笔付款。外部转入只体现在余额中，首版没有完整链上流水。

恢复使用现有 BIP39 助记词与空 passphrase；Solana 使用 SLIP-0010 Ed25519 路径 `m/44'/501'/账户索引'/0'`。账户 01 对应索引 0，最多 32 个账户。不同钱包软件可能使用其他路径；恢复后应核对原地址，不以相同助记词推断地址一定一致。现有 BSC 路径和加密钱包格式不变。

已验证：36 组独立公开 SDK 向量（地址、ATA、消息与签名）、余额/费用/模拟失败及最终确认的模拟测试、持久化与重启相同字节重播、锁定及账户绑定、实际隔离 ABI、100%/150% 界面布局、主网只读查询和 BSC 回归。没有发送真实资金，也没有用用户钱包进行测试。

主网数据来自固定 TLS RPC，校验完整 genesis、mint、程序及账户所有者。最终确认核对原交易字节；过期需两个提供者均返回无记录、finalized 高度超过有效高度且 blockhash 无效。仍依赖 RPC 的真实性和历史完整性，不是轻客户端证明；查询错误或证据不一致保持待确认。

参考：[Solana 交易结构](https://solana.com/docs/core/transactions)、[关联代币账户](https://solana.com/docs/tokens/basics/create-token-account)、[Tether 支持的协议](https://tether.to/en/supported-protocols/)、[SLIP-0010](https://github.com/satoshilabs/slips/blob/master/slip-0010.md)。
