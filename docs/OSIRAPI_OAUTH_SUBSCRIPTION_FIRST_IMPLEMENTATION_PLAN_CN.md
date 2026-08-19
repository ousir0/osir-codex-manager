# OSIRAPI × Codex Manager 订阅用户 OAuth 接入落地方案

## 1. 结论

本期只落地“订阅用户通过浏览器 OAuth 登录并自动接入 GPT、Claude、Grok”。余额用户暂不进入本期实现，保留后续扩展接口和状态，不把余额计费逻辑混入当前 OAuth 主链路。

本期目标不是重做 OpenCodex，而是把现有的订阅 Key 创建、加密配置下发和 OpenCodex 多模型同步流程，从“网页登录后复制连接码”升级为“浏览器授权后自动完成”。

## 2. 本期范围

### 必须完成

- Manager 点击“连接 OSIRAPI”后打开系统浏览器。
- 用户在 OSIRAPI 完成登录和 Codex Manager 授权。
- 使用 OAuth Authorization Code + PKCE + 本机回调完成桌面认证。
- OSIRAPI 自动创建或复用 GPT、Claude、Grok 三把订阅专用 Key。
- Key 绑定到用户有效订阅对应的平台接入分组。
- OSIRAPI 返回加密配置包，Manager 自动安装/检测 OpenCodex、写入配置并同步 Codex。
- 完成 OpenCodex health、ready、模型目录和 Codex Provider 校验。
- 保留“粘贴配置码”作为备用接入方式。

### 明确不做

- 本期不支持余额用户自动创建余额 Key。
- 本期不设计订阅和余额同时存在时的扣费优先级。
- 本期不允许 Manager 自己选择服务端内部分组 ID。
- 本期不把长期上游供应商 Key 或数据库凭据放入客户端。
- 本期不迁移 OpenCodex 项目，也不改造 Codex 原生选择器。

## 3. 当前基础能力

### Manager 已有能力

- 已有 OpenCodex 安装、启动、健康检查和配置同步。
- 已有 OpenCodex 路由和模型目录管理。
- 已有配置备份、原子写入、恢复和错误保护。
- 已有 OSIRAPI 一次性连接码兑换和 RSA + AES-GCM 加密配置包流程。

### OSIRAPI 已有能力

- 已有用户登录、JWT、Refresh Token 和订阅查询。
- 当前订阅接入接口会检查有效订阅、平台分组和模型目录。
- 当前服务端会自动创建或复用三把订阅专用 API Key。
- 当前 Key 创建接口会在未传入自定义 Key 时生成随机 Key。
- 当前兑换接口会把配置包加密到 Manager 生成的 RSA 公钥，不直接明文返回长期配置。

### 当前限制

现有流程要求用户先在 OSIRAPI 网页中登录，再手动复制一次性连接码。普通登录本身不会创建 Codex Key；Key 创建发生在用户发起 Codex 接入并通过订阅校验之后。

## 4. 最终用户流程

~~~text
Manager 点击「连接 OSIRAPI」
        ↓
系统浏览器打开 OSIRAPI 授权页
        ↓
用户登录（已有浏览器登录态时直接进入授权确认）
        ↓
用户确认「允许 Codex Manager 配置订阅模型」
        ↓
OSIRAPI 校验订阅、平台分组和模型目录
        ↓
自动创建或复用 GPT / Claude / Grok 订阅 Key
        ↓
浏览器回调 Manager，本机完成 PKCE 换证
        ↓
Manager 自动安装/启动 OpenCodex
        ↓
获取加密配置包并写入 OpenCodex
        ↓
同步 Codex 模型目录并检查 ready
        ↓
提示「模型已接入，请完全重启 Codex」
~~~

用户不需要看到 Key，不需要复制命令，不需要手动填写分组，也不需要理解 OAuth、PKCE 或 OpenCodex 的内部细节。

## 5. Key 自动创建与订阅分组规则

### 5.1 Key 生命周期

每个平台维护一把 Manager 专用订阅 Key：

- GPT：GPT Pro 订阅套餐密钥
- Claude：Claude Pro 订阅套餐密钥
- Grok：Grok订阅套餐密钥

本期继续复用 OSIRAPI 已有名称，以兼容现有连接码和 Key 复用逻辑；后续可以增加独立的 managed purpose 字段，不依赖名称识别。

服务端按“用户 + Manager 用途 + 平台 + 计费模式”幂等查找：

1. 没有 Key：自动生成随机 Key。
2. 有有效 Key：复用原 Key。
3. Key 停用、过期或分组不匹配：修复绑定或重新生成。
4. 用户重新连接：不重复创建新 Key。
5. 用户主动撤销或轮换：停用旧 Key，再生成新 Key。

Key 内容不允许由用户自定义。用户只能看到脱敏状态、用途、创建时间、最后使用时间和撤销入口。

### 5.2 订阅分组选择

Manager 不接收也不提交内部 Group ID。OSIRAPI 根据用户选择的有效订阅自动解析：

- 订阅套餐 → 订阅配额池。
- 套餐包含的 GPT/Claude/Grok 平台能力 → 对应平台订阅接入分组。
- API Key 绑定到平台订阅接入分组，不绑定到 quota pool 本身。
- 分组必须 active，且有至少一个可用模型。

当前接口已经具备这一逻辑：先读取用户有效订阅，再读取套餐包含的访问分组，然后为每个平台创建或复用 Key。

### 5.3 三个平台的开通条件

本期默认要求 GPT、Claude、Grok 三个平台都满足条件：

- 有有效的非系统订阅；
- 套餐包含对应平台的 active 接入分组；
- 对应分组至少有一个可用模型。

如果某个平台缺失，不应生成半套配置后继续启用 OpenCodex。应明确提示缺少哪个平台，并保持原有 Codex 配置不变。

后续如产品需要“只开通已购买的平台”，再增加按平台启用，不在本期扩大范围。

## 6. OAuth 技术流程

### 6.1 Manager 端

1. 生成随机 state。
2. 生成 PKCE code_verifier 和 code_challenge。
3. 在本机监听随机端口，仅绑定 127.0.0.1。
4. 打开系统浏览器到 OSIRAPI 授权地址。
5. 接收回调并校验 state。
6. 使用 code_verifier 换取一次性授权结果。
7. 发送 Manager 公钥和订阅接入请求，获取加密配置包。
8. 解密后写入 OpenCodex，明文 Key 不写日志。
9. 完成健康检查、模型目录同步和 Codex 配置校验。

### 6.2 OSIRAPI 端

实际落地采用“网页登录态 + PKCE 一次性授权码”，避免新增完整 OAuth Provider 和长期 Token 系统：

- 浏览器授权页：GET /codex-manager/connect
- 已登录用户签发授权码：POST /api/v1/codex-install/desktop/authorize
- Manager 兑换加密配置：POST /api/v1/codex-install/desktop/exchange

未登录用户由授权页自动跳转到现有登录页，登录成功后回到原授权页。浏览器 JWT 不会传给 Manager。

授权范围建议：

- codex.install
- codex.models.read

不授予账户管理、余额管理、API Key 管理等其他权限。

OAuth 换证成功后，OSIRAPI 应直接进入现有 Codex 接入服务：

1. 根据 OAuth 用户身份读取有效订阅。
2. 选择或确认订阅套餐。
3. 调用现有 Key 自动创建/复用逻辑。
4. 生成短时、一次性接入会话。
5. 使用 Manager 公钥返回加密配置包。

第一阶段不在 Manager 持久化 Access Token 或 Refresh Token。浏览器授权只负责身份确认和一次性授权，模型 Key 继续通过现有加密配置包下发。

## 7. 配置包与本地联动

配置包中的每个平台路由至少包含：

~~~json
{
  "platform": "openai",
  "provider": "osirapi-openai",
  "key_id": 123,
  "api_key": "server-generated-key",
  "adapter": "openai-responses",
  "base_url": "https://api.osirclaw.com/v1",
  "models": ["gpt-5.6-sol"],
  "recommended_model": "gpt-5.6-sol"
}
~~~

Manager 处理规则：

- Key 只写入 OpenCodex 私有配置，不写入 Codex auth.json。
- Base URL、Adapter 和模型目录写入 OpenCodex 路由。
- 生成的模型目录指向 Codex 配置中的 model_catalog_json。
- OpenCodex ready 且模型目录非空后，才启用 model_provider = opencodex。
- 任何一步失败，都恢复本次操作前的 Codex/OpenCodex 配置。

## 8. 失败与边界处理

| 场景 | 用户看到的结果 | 系统动作 |
| --- | --- | --- |
| 用户取消授权 | 已取消连接 | 不改本地配置 |
| OAuth 回调超时 | 授权超时，请重试 | 清理临时状态，不保留 Token |
| 没有有效订阅 | 当前账号没有可用订阅 | 不创建 Key，不启用 OpenCodex |
| 缺少 GPT/Claude/Grok 分组 | 套餐缺少某个平台能力 | 不写入半套路由 |
| 分组没有模型 | 该平台暂不可用 | 不启用对应接入 |
| Key 创建失败 | OSIRAPI 暂时无法准备模型 | 本地配置保持不变 |
| OpenCodex 启动失败 | 多模型组件未就绪 | 不修改 Codex 生效 Provider |
| Codex 模型目录为空 | 模型目录同步失败 | 恢复上次可用配置 |
| 用户已有配置 | 保留原配置并创建备份 | 只管理 Manager 自己标记的内容 |

## 9. 安全要求

- 使用 Authorization Code + PKCE，禁止在 Manager 中嵌入账号密码登录。
- 回调必须校验 state，授权码只能使用一次。
- 本机回调只监听 127.0.0.1 随机端口。
- OSIRAPI 连接地址只允许 HTTPS。
- Manager 日志、诊断包、更新清单和 UI 不显示完整 API Key、Token 或连接码。
- 配置包使用当前 RSA 公钥加密和 AES-GCM 完整性校验。
- 服务端记录 Key 用途、设备标签、创建时间、最后使用时间和撤销状态，但不记录明文 Key。
- 订阅过期时，服务端停止该 Key 的有效调用；本地可以保留脱敏路由状态，待重新授权后恢复。

## 10. Manager 开发任务

### P4：OAuth 接入开发

- 增加桌面 OAuth 状态模型。
- 增加随机端口本机回调服务。
- 增加系统浏览器唤起。
- 增加 OAuth 成功、取消、超时和失败 UI。
- 将现有“连接 OSIRAPI”弹窗改为浏览器授权入口。
- 保留配置码输入入口。

### P5：联调与可用性验收

- 联调未登录用户和已有登录态用户。
- 验证首次创建 Key 和再次连接复用 Key。
- 验证订阅变更后的 Key 分组修复。
- 验证 GPT、Claude、Grok 三个平台的模型目录。
- 验证 OpenCodex ready 后才启用 Codex Provider。
- 验证 OAuth 失败不破坏旧配置。
- 验证完全重启 Codex 后模型选择器显示多模型。
- 验证撤销 Key 后本地状态和远端调用结果一致。

## 11. OSIRAPI 开发任务

- 注册 Codex Manager OAuth Client ID。
- 增加桌面 OAuth 授权、回调和一次性换证接口。
- 增加 codex.install 和 codex.models.read 权限校验。
- 抽取现有 ensureCodexInstallKeys 为可复用的订阅接入服务。
- 将“选择订阅、解析平台分组、创建/复用 Key、生成加密配置包”串入 OAuth 会话。
- 增加 OAuth 审计事件和撤销/轮换能力。
- 增加无订阅、缺分组、无模型、重复连接和并发连接测试。

## 12. P5 验收标准

### 用户流程

- 用户只需点击“连接 OSIRAPI”，在浏览器登录并确认一次。
- Manager 能自动回到前台并显示连接进度。
- 用户无需复制连接码、填写 API Key 或选择分组 ID。

### 服务端 Key

- 首次连接自动创建三把订阅 Key。
- 第二次连接复用原有三把 Key，不产生重复 Key。
- Key 分别绑定 GPT、Claude、Grok 的订阅接入分组。
- Key 的创建、复用、修复和撤销都有审计记录。

### 本地模型

- OpenCodex ready 后模型目录包含真实可用模型。
- Codex 配置存在 opencodex Provider。
- Codex 选择器能看到 GPT、Claude、Grok 模型。
- 每个平台至少完成一次最小请求验证。
- 路由失败时不自动切换到其他平台。

### 安全与恢复

- 日志和诊断信息不包含完整 Key、Token 或连接码。
- 取消授权、网络失败和配置失败均不破坏旧配置。
- Manager 重启后可以读取本地状态，但不会无提示自动重新授权。
- 用户可通过 OSIRAPI 撤销 Manager 专用 Key。

## 13. 余额用户后续扩展边界

余额用户不属于本期交付。后续如要支持，必须单独设计：

- standard 余额分组；
- 用户余额预检查和扣费；
- 余额 Key 的创建、复用和轮换；
- 订阅与余额同时存在时的优先级；
- 余额不足时的模型状态和恢复策略。

本期代码和接口只保留扩展字段，例如 billing_mode、entitlement_status，不实现余额 Key 创建，避免两套计费逻辑相互污染。

## 14. 最终判断

本期优先做订阅用户是正确的：当前 OSIRAPI 已有完整的订阅分组、Key 自动创建和订阅额度扣费基础，改造集中在 OAuth 授权和接入链路；余额模式则需要新增标准分组、余额权益和扣费策略，适合单独立项。

本期完成后，用户体验应达到：

> 登录 OSIRAPI → 授权 → 自动创建/复用 Key → 自动接入 OpenCodex → 重启 Codex 后看到多模型。

这条链路稳定后，再扩展余额用户，不会推翻本期 OAuth、OpenCodex 和 Manager 的主体结构。

## 15. 当前落地状态

- OSIRAPI 功能提交：6f7dcd141。
- Codex Manager 功能提交：e42cb7d。
- 两个提交均位于隔离功能分支，没有合并 main、没有打标签、没有触发正式发布。
- OSIRAPI 专项测试覆盖首次自动创建三把 Key、加密配置下发和幂等重试。
- Manager 专项测试覆盖 PKCE、127.0.0.1 回调和现有配置包解密。
- Manager 前端 335 项测试通过；OSIRAPI 前端生产构建通过。
- 正式环境真实账号联调必须在受控发布后执行；发布前不得把“代码测试通过”误报为“生产已上线”。
