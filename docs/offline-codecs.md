# M0a 离线金额、地址与交易编码

2026-11-04：新增 wallet-core、chain-evm、chain-tron，均不执行网络、文件或进程操作。FFI 已通过这些库校验公开地址与金额；signer/network 仍只处理公开 ping。库 API 没有私钥输入或签名生成，只有编码、解析、哈希及签名恢复校验；私钥到公钥和签名生成仅在 cfg(test) 中使用公开向量。

## 已实现

wallet-core 的 Amount 保存 32 字节无符号最小单位整数。parse_units/format_units 使用十进制字符串，不经浮点数。拒绝负数、指数、千位分隔、空白、前导零、精度超限、uint256 溢出；允许零余额，转账编码额外拒绝零金额。显示保留指定小数位，decimals 范围 0–77。

chain-evm 支持 SEC1 公钥校验、20 字节地址、EIP-55 输出和混合大小写校验。全小写/全大写地址按 EIP-55 兼容行为接收，不能声称这些输入经过校验和保护。交易实现 EIP-155 legacy 格式（不含 EIP-1559），金额/gas price 使用 Amount，nonce/gas limit/chain ID 使用 u64。只接受原生转账或可信 Policy 中指定合约的 transfer(address,uint256)。解析后必须精确重编码一致，拒绝非最短整数、额外字段、尾随数据及签名字段伪装成准备记录。

chain-tron 支持 0x41 前缀的 21 字节地址与 Base58Check、TransferContract 和白名单 TriggerSmartContract transfer。Protobuf 只实现必要字段，在外层和 Any.value 内层均执行精确重编码校验。拒绝未知字段、显式默认字段、乱序/重复字段、额外合约、权限 ID、memo、TRC10、任意调用、call_value、token_id 和附带转移。这是本项目严格子集，不是通用 TRON 解码器；某些链上合法但不同编码形式也会被拒绝。

TRC-20 合约参数使用去掉 0x41 后的 20 字节地址；普通 protobuf owner/to 地址仍保留 0x41。TRX 金额必须为正 int64；代币金额为 uint256。时间字段须为正、expiration 在 timestamp 后且间隔不超过 24 小时，但不声称据此证明当前链上有效。TRX 不带 fee_limit，TRC-20 要求正 fee_limit；该字段仅为能量限制，不是总费用保证。

两链 signed_bytes 只接收已生成的 65 字节签名，通过 low-s、标量范围、恢复 ID 和发送方校验后组装载荷；不生成签名。确认内容变更会导致旧签名不能通过恢复校验。TRON txID 为 raw_data 字节的 SHA-256，EVM 签名摘要为 EIP-155 编码字节的 Keccak-256。

## 验证依据与边界

| 范围 | 独立依据 | 本轮结果 |
| --- | --- | --- |
| EVM 地址 | EIP-55 用例、Trust Ethereum AddressTests 公钥派生向量 | 通过 |
| EVM legacy 编码/签名 | EIP-155 nonce=9 的公开例子 | 签名前字节、摘要、完整已签字节一致 |
| TRON 地址 | 官方 Accounts 公钥与 hex/Base58 对照、Trust 向量 | 通过 |
| TRX 编码/签名 | Trust TronSigner.SignTransfer 公开例子 | txID 与完整 65 字节签名一致，恢复 owner 一致 |
| ERC20/TRC20 transfer | 固定函数选择器、20 字节地址/uint256 ABI 对照 | 通过字节与解析测试 |
| 非法请求 | 错链/错合约、approve、错误 padding、非规范字段、截断、签名更改 | 拒绝 |

EIP-155 的独立完整向量使用 Ethereum chain ID 1，仅证明公共 EVM 编码规则；BSC chain ID 56/97 的配置区分经过本地测试，尚未完成 BSC 独立链上或提供方交叉验证。TRC-20 已与固定 TronWeb 6.5.1 独立实现比较 raw_data、txID、签名和完整已签字节，详见下方补充。TRON 原始交易本身不编码 chain ID，后续必须用可信网络配置和参考块验证绑定网络。任何 Policy 都由可信调用方提供，当前库不能证明策略来源或对端身份。

未实现 BIP39/BIP32、保险库、随机创建、真实签名服务、确认令牌、会话认证、当前参考块/资源/余额/nonce/最终性查询、交易日志或广播。重复编码不等于防止重复付款。当前结果不授权使用真实密钥。

## 来源

第三段补充：tests/oracle-tronweb/vector.cjs 使用 TronWeb 6.5.1 的独立 JavaScript ABI、google-protobuf、ethereum-cryptography 实现生成 TRC-20 向量。运行 `node tests/oracle-tronweb/vector.cjs` 复核 fixture，`--write` 显式再生成。脚本封锁联网并断言零网络调用。SDK 输出 recovery+27，fixture 保留 sdk_signature 与减 27 后的 signature；signed_hex 是规范化签名交给 SDK protobuf 序列化所得。Rust 比较 raw_data、txID、r/s、恢复位和完整已签载荷，并拒绝修改费用后的旧签名。这是明确适配恢复位之后的一致性。使用公开 Trust 测试密钥和虚构代币合约，不是 USDT。

- [EIP-155](https://eips.ethereum.org/EIPS/eip-155)：签名编码及公开向量。
- [EIP-55](https://eips.ethereum.org/EIPS/eip-55)：地址 checksum 与用例。
- [TRON Accounts](https://developers.tron.network/docs/account)：公钥派生和地址对照。
- [TRON Encoding](https://developers.tron.network/docs/encoding)：ABI 地址去掉 0x41；历史 21 字节兼容形式不作为本项目编码输出。
- [TRON Transactions](https://developers.tron.network/docs/tron-protocol-transaction)：raw_data、txID、时间及参考块语义。
- [固定协议 schema](https://github.com/tronprotocol/protocol/blob/2a678934da3992b1a67f975769bbb2d31989451f/core/Tron.proto)：字段编号对照。
- 本地 Trust Wallet Core 2a8aa02011ec42b43689667682fa99ae905418d5 的 tests/chains/Ethereum/AddressTests.cpp、tests/chains/Tron/AddressTests.cpp、SignerTests.cpp 和 src/Tron/Protobuf/TronInternal.proto。公开测试数据的归属与许可证见 THIRD-PARTY-NOTICES.md。
