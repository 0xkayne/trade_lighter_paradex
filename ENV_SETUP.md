# Paradex 交易客户端环境配置

## 快速开始

1. **复制示例配置文件**
```bash
cp .env.example .env
```

2. **编辑 `.env` 文件，填入您的账户信息**
```bash
nano .env
```

3. **运行程序**
```bash
# 测试网（默认）
cargo run

# 生产环境
cargo run -- --production
```

## 环境变量说明

| 变量名 | 说明 | 示例 |
|--------|------|------|
| `paradex_account_private_key_hex` | Paradex 账户私钥（十六进制） | `0x0706e8111...` |
| `eth_account_address` | 以太坊账户地址（用于 onboarding） | `0x36Fb7eFD...` |
| `paradex_account_address` | Paradex StarkNet 账户地址 | `0x445afd19...` |

## 账户体系说明

Paradex 使用双层账户体系：

### Root 账户（主账户）
- **Ethereum 地址**：L1 身份验证
- **StarkNet Root 账户**：L2 签名账户
- **用途**：Onboarding（注册）

### Subkey 账户（交易账户）
- **Subkey StarkNet 地址**：交易专用地址
- **Subkey 私钥**：交易签名
- **用途**：JWT 认证、执行交易

## 重要提示

⚠️ **私钥和地址必须匹配**：
- Onboarding 时使用 Root StarkNet 账户
- JWT 获取时使用 Subkey 账户
- 如果私钥和地址不匹配，会出现 `STARKNET_SIGNATURE_VERIFICATION_FAILED` 错误

## 功能说明

程序会自动执行：
1. 从 `.env` 加载账户信息
2. 执行 Onboarding（如果需要）
3. 获取 JWT token
4. 订阅市场数据（公开 + 私有频道）
5. 执行订单操作（创建、修改、取消）
6. 2分钟后清理并退出

## 故障排除

**签名验证失败**：
- 检查私钥是否与 StarkNet 地址匹配
- 确认使用的是正确的账户类型（root vs subkey）

**NOT_ONBOARDED 错误**：
- 需要先成功完成 onboarding
- 检查 Ethereum 地址和 Root StarkNet 地址是否正确

