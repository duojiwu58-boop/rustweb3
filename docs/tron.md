# Tron 主网 USDT（TRC20）

入口：双击 `artifacts/bnb-live/wallet-launcher.exe`，解锁钱包后在顶部网络下拉框选择 **Tron**。

支持当前所选账户的独立 Tron 收款地址、TRX / USDT 余额、USDT 转账核对与发送、最近两笔本机交易（待处理优先）、原交易查询及有效期内的原样重播。多账户使用现有 `m/44'/195'/0'/0/index` 派生路径，旧钱包无须迁移或重新导入。TRX 在本页用于收款和支付手续费，暂不提供 TRX 转出。

仅支持主网官方 USDT 合约 `TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t`，6 位小数。来源：[Tether 支持协议](https://tether.to/en/supported-protocols/)。不接受自定义代币、测试网、授权调用或多签／修改过 owner 权限的发送账户。

## 使用

1. 点击“复制 Tron 地址”，核对充值平台选择的是 **Tron / TRC20**。不要使用 BSC 或 Solana 地址。
2. 新地址应先接收 TRX 激活账户，并保留 TRX 支付费用。USDT 不能直接支付手续费。
3. 输入收款地址和 USDT 数量，点击“核对转账”。核对完整地址、数量、能量费用上限和带宽费用预留，在 20 秒内确认发送。
4. 在“本机转账”页查询结果。“已提交”或“待确认”均不表示到账；只有固化交易、成功回执和匹配的 USDT Transfer 日志才显示最终确认。

能量费用上限按模拟能量 × 当前链上能量单价 × 2 计算，限制为最多 100 TRX。带宽按编码长度加协议开销预留，最多 10 TRX。为保守检查余额，不预先抵扣质押、委托或免费资源；实际费用可能更低。能量上限不是总手续费上限，带宽预留也不是协议强制的费用上限。

## 失败与恢复

签名载荷先加密写入现有 SQLite 交易日志，再释放给独立联网进程。一个 Tron 账户有未决交易时，禁止创建第二笔 Tron 付款；重播只使用原交易的相同字节和 txID。

交易有效期为 60 秒。超过有效期不能重播。仅凭“过期／查无交易”无法证明没有付款，因此本版会继续保留待确认锁，不自动解锁以创建新付款。查询最终结果前，请勿删除钱包或清理日志来绕过待确认状态。外部转入当前通过余额查看，本页不是完整链上历史浏览器。

TronGrid 公共节点可能限流。只读请求遇到 HTTP 429 最多重试两次；广播不自动重试，失败后通过原 txID 查询。沿用 `network.ini` 的代理设置，无静默直连回退。

## 验证与边界

- TronWeb 6.5.1 离线独立向量：USDT ABI、protobuf、txID 和签名；`tests/oracle-tronweb/tron-usdt.cjs`。
- Rust 测试：精度、错误网络、费用／余额不足、模拟失败、账户绑定、锁定、重启重播、防重复付款及固化回执／事件匹配。
- Delphi 隔离测试：新增 Tron 签名、加密载荷重播、记录与锁定检查；界面在 96 / 144 DPI 下检查控件边界。
- 主网只读验证：公开测试地址的 TRX／USDT 余额与合约精度；不使用用户钱包，不广播真实资金交易。

实现参考：[模拟调用](https://developers.tron.network/reference/triggerconstantcontract)、[签名广播流程](https://developers.tron.network/docs/api-signature-and-broadcast-flow)、[固化确认语义](https://developers.tron.network/docs/confirmation-semantics)。这轮验证不等于真实资金端到端转账或独立安全审计。
