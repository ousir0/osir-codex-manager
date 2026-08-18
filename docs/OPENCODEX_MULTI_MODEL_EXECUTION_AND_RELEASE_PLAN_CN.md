# Codex Manager 多模型接入、开发落地与发布总计划

## 1. 总目标

把 Codex Manager 从“单 Provider 配置工具”升级为“可选的多模型接入管理器”：

- 用户安装 Manager 后，不需要预装 Node、npm 或 OpenCodex。
- 用户可以一键连接 OSIRAPI，或粘贴通用供应商连接码。
- Manager 自动检测环境、安装或复用 OpenCodex、配置路由、同步模型并提示重启 Codex。
- Codex 原生模型选择器显示多个供应商和模型。
- Manager 不迁移 OpenCodex 的复杂项目，不修改 Codex 应用包，不重写 OSIR API 的服务端路由。
- 最终产出 macOS arm64、macOS Intel、Windows x64、Windows ARM64 四套可验证安装包，并完成 Manager 自更新和多模型组件分发。

## 2. 产品边界

### 本项目负责

- CODEX 配置管理页面和多模型向导。
- OpenCodex 组件检测、安装、启动、停止、更新、回滚和健康检查。
- 通用供应商连接码解析和供应商适配器入口。
- OSIRAPI 一键连接。
- OpenCodex 路由与模型目录配置。
- 本地配置备份、恢复、脱敏诊断和用户提示。
- Manager、OpenCodex 组件和安装包的发布分发。

### OSIR API 负责

- 用户登录、设备授权和一次性连接票据。
- 供应商路由、API Key、模型权限和套餐范围。
- 连接码签发、签名、过期和单次消费。
- 模型目录和能力范围。
- 用量、限额、计费和服务端审计。

### 首版明确不做

- 不把 OpenCodex 源码迁移进 Manager。
- 不在 Manager 内实现模型请求转发网关。
- 不抓取供应商网页登录 Cookie，不模拟登录，不绕过供应商授权。
- 不首版支持所有协议；首版以 OpenAI Responses / OpenAI Compatible 为主。
- 不自动修改用户订阅、API Key 权限或服务端路由策略。
- 不在用户不知情的情况下自动测试所有付费模型。

## 3. 最终用户流程

用户只需要看到一个入口：CODEX 配置管理 → 启用多模型。

~~~text
检测环境
  -> 选择连接 OSIRAPI / 粘贴供应商连接码 / 手动添加供应商
  -> 安装或复用 OpenCodex
  -> 完成供应商鉴权
  -> 配置并同步模型
  -> 验证服务和目录
  -> 提示完全重启 Codex
~~~

首次页面只显示：

- 连接 OSIRAPI。
- 粘贴配置码。
- 手动添加供应商。
- 当前进度、失败原因和重试按钮。

端口、Provider ID、Adapter、配置文件路径和诊断信息放入高级设置。

## 4. 总体开发阶段

### P0：方案冻结与合同定义

目标：先固定 Manager、OpenCodex、OSIR API 三方边界，避免边开发边改变数据格式。

产出：

- 通用连接码 Schema：codex-manager.connection/v1。
- OSIRAPI 连接票据交换接口定义。
- ProviderConnector 适配器接口。
- OpenCodex 组件版本清单格式。
- 多模型本地配置格式和受管字段规则。
- 用户流程、错误文案和隐私说明。

验收：

- 连接码包含 issuer、audience、过期时间、单次消费、供应商、协议、模型范围和签名。
- 连接码不包含长期 API Key、Refresh Token、Cookie 或密码。
- 直接 Provider 模式和 OpenCodex 模式边界明确。

### P1：OpenCodex 组件管理层

目标：让新用户无需预装 Node、npm 或 OpenCodex。

实现内容：

- 新增 OpenCodex runtime/component manager。
- 检测操作系统、架构、Node、OpenCodex、端口和已有配置。
- 使用 Manager 自有版本清单下载固定版本组件。
- 下载后做 SHA-256、签名、版本和平台校验。
- 安装到用户目录，不使用全局 npm，不默认申请管理员权限。
- macOS 使用当前用户 launchd，Windows 使用当前用户 Task Scheduler。
- 支持 start、stop、health、ready、sync、update、rollback。

推荐目录：

~~~text
~/.codex-manager/runtime/opencodex/<version>/
~/.codex-manager/state/opencodex.json
~/.codex-manager/backups/opencodex/<timestamp>/
~~~

分发 OpenCodex 运行时前必须确认许可证和再分发条件。如果不能随 Manager 分发，则改为由 Manager 下载官方固定版本，并提供同等哈希和签名校验；用户仍不需要手动执行 npm。

### P2：本地配置引擎

目标：安全地把多模型路由接入 Codex。

实现内容：

- 新增 Rust 模块 opencodex_config。
- 扩展 CodexConfigReport，增加 OpenCodex 状态、版本、端口、服务状态、目录路径和模型数量。
- 新增 OpenCodex 独立输入结构，不复用只支持单 API Key 的基础保存结构。
- 备份 config.toml、models_cache.json、opencodex-catalog.json 和 OpenCodex 配置。
- 只修改 Manager 自己标记的 Provider、路由和模型，不删除用户其他配置。
- 候选文件先校验，再原子替换；同步后重新读取并验证。
- OpenCodex 模式写入代理 Provider：

~~~toml
model_provider = "opencodex"
model_catalog_json = "~/.codex/opencodex-catalog.json"

[model_providers.opencodex]
name = "OpenCodex 多模型路由"
base_url = "http://127.0.0.1:<port>/v1"
wire_api = "responses"
requires_openai_auth = false
~~~

### P3：通用供应商连接与 OSIR 一键接入

目标：把“连接供应商”统一成一个入口，OSIR 只是第一个适配器。

适配器顺序：

1. OSIRAPI：一次性连接票据。
2. OpenAI Compatible：Base URL + API Key。
3. 官方 OAuth / 设备码供应商：仅在有正式授权合同后接入。
4. 签名配置包：团队管理员分发。

实现内容：

- 连接码签名、来源、受众、过期时间、单次消费校验。
- OSIRAPI 登录、设备授权或 Deep Link。
- 加密配置包交换。
- Provider、模型范围、能力和默认模型转换为 OpenCodex 路由。
- 连接成功后清空连接码输入框，不回显长期 Key。
- 失败时不写入本地配置，保留原状态。

### P4：极简配置 UI

目标：普通用户不需要理解技术配置。

页面结构：

- 状态卡：未启用、配置中、已就绪、需要修复。
- 主按钮：连接 OSIRAPI。
- 次按钮：粘贴配置码。
- 第三入口：手动添加供应商。
- 进度条：检测环境、安装组件、连接供应商、同步模型、完成。
- 完成卡：供应商类别、模型数量、上次同步时间、重启提示。
- 高级设置：路由、端口、模型筛选、组件版本、诊断、恢复。

默认行为：

- OSIRAPI 默认提供 GPT、Claude、Grok 三类路由。
- 模型默认全部同步，用户可以在高级设置中取消某类模型。
- 默认只做健康检查；实际请求测试按供应商执行一次，并明确可能产生费用。
- 普通 API Key 页面在 OpenCodex 模式下显示“不使用 auth.json”，避免用户误以为需要重复填写。

### P5：测试与本地联调

目标：在打包前证明“配置可写、服务可用、模型可见、请求可走通”。

自动化测试：

- Rust：连接码校验、配置合并、原子写入、备份恢复、端口状态、服务状态映射。
- TypeScript：向导状态机、连接模式切换、模型分组、错误文案、浏览器 fallback。
- OpenCodex contract：固定版本的 config validate、sync、health、ready 输出。
- 安全测试：Key 脱敏、日志脱敏、连接码过期、重复消费、签名错误、错误来源 URL。
- 回归测试：直接 Provider 模式、MCP、图片配置、旧 Provider 和旧配置迁移。

人工验收矩阵：

| 环境 | 必测内容 |
| --- | --- |
| macOS Apple Silicon | 全新用户、已有 Codex、已有 OpenCodex、端口冲突、恢复 |
| macOS Intel | 同上，验证架构和用户级服务 |
| Windows x64 | 全新用户、已有 Codex、任务计划服务、恢复 |
| Windows ARM64 | ARM64 Manager、ARM64 Codex 或兼容环境、配置和目录 |
| 网络异常 | 下载中断、连接码过期、模型同步失败、重试后恢复 |

### P6：发布前冻结

目标：代码、组件版本、协议和文档冻结，避免构建后反复返工。

冻结内容：

- AppManager 版本号。
- OpenCodex 组件版本和哈希。
- OSIRAPI 连接码 Schema 版本。
- 默认模型、Provider 显示名和模型目录规则。
- Windows/macOS 安装器名称和更新 URL。
- 隐私、许可证和故障恢复文案。

冻结后只允许修复发布阻塞问题；任何功能改动都必须回到 P5 重新验证。

## 5. 打包与构建总计划

### 5.1 构建输入

最终构建必须来自：

- main 已合并且可追溯的提交 SHA。
- 已冻结的 AppManager 版本。
- 已冻结的 OpenCodex 组件版本清单。
- 经过测试的 OSIRAPI 连接码公钥和接口地址。
- 发布环境中的 updater 私钥；私钥不进入仓库、不进入安装包。

### 5.2 CI 阶段

每次代码合并：

- TypeScript 类型检查、lint、单元测试。
- Rust 单元测试和配置模块测试。
- 连接码、OpenCodex contract 和发布清单测试。
- 不触发正式安装包发布。

候选版本：

- 在默认分支冻结后手动触发跨平台打包。
- macOS arm64 和 Intel 分别构建。
- Windows x64 和 ARM64 由 Windows Runner 构建。
- Windows ARM64 必须检查 PE machine 为 0xAA64。
- 生成 updater 签名、SHA-256 和组件清单。

### 5.3 产物清单

Manager：

- CodexManager_aarch64.dmg
- CodexManager_x86_64.dmg
- CodexManager_<version>_x64-setup.exe
- CodexManager_<version>_arm64-setup.exe
- 两种 macOS updater tarball 及 .sig
- Windows x64/ARM64 安装器及 .sig

多模型组件：

- 组件版本清单 JSON。
- macOS arm64 / Intel 组件包。
- Windows x64 / ARM64 组件包。
- 每个包的 SHA-256、大小、版本、许可证和下载地址。

更新清单：

- latest.json 必须包含四个平台。
- partial 必须为 false。
- 每个 URL、签名和实际字节必须一致。
- 组件清单必须能被 Manager 下载并校验。

### 5.4 发布顺序

1. 发布候选构建到 GitHub Actions artifact，不直接覆盖线上 latest。
2. 下载全部产物并在本地验证签名、哈希、名称和架构。
3. 生成完整 latest.json 和 OpenCodex 组件清单。
4. 在独立服务器目录发布版本化文件。
5. 线上验证 200、Range 206、CORS、SHA-256、签名和模型组件下载。
6. 原子切换服务器 current。
7. 发布官网和下载页。
8. 再发布 GitHub Release 或其他镜像。
9. 更新交接文档和发布记录。

### 5.5 不同平台的发布门槛

| 平台 | 必须通过的门槛 |
| --- | --- |
| macOS arm64 | DMG 可安装、Bundle ID 正确、updater 签名正确；正式发布需 Developer ID 和公证 |
| macOS Intel | Intel 二进制、DMG、updater tarball 和签名完整 |
| Windows x64 | 安装、启动、升级、卸载 smoke test；updater 签名正确 |
| Windows ARM64 | ARM64 PE 检查、安装器签名、updater 签名；真实 ARM64 运行验证可作为后续增强 |
| OpenCodex 组件 | 平台架构、版本、签名、哈希和 health/ready 验证完整 |

## 6. 灰度发布计划

### 内部验证

- 先在当前 Mac 环境完成 OSIR 一键连接和多模型选择器验收。
- 再在一台干净 macOS 和一台干净 Windows 环境验证首次安装。
- 使用独立测试 Key，不使用生产主 Key。

### 小范围试用

- 选择 3 至 5 名用户，覆盖 macOS、Windows x64 和不同网络环境。
- 只开放 OSIRAPI 一键连接，不先开放自由供应商配置。
- 记录安装成功率、连接码失败率、模型同步失败率、恢复成功率和用户完成时间。

### 正式发布

- 首次正式版本保留 OpenCodex 多模型为可选功能，不强制所有用户启用。
- 新用户默认展示入口，旧用户不自动改变原有 Provider。
- 组件升级和路由配置均由用户确认后执行。
- 发现严重问题时，可以停用多模型入口，但不影响 Manager 安装、更新和直接 Provider 模式。

## 7. 风险与回退

| 风险 | 影响 | 回退方案 |
| --- | --- | --- |
| OpenCodex 组件不兼容 | 模型目录或请求失败 | 停留在上一个已验证版本，恢复 Codex 配置 |
| 连接码服务异常 | 用户无法一键接入 | 提供手动 API Key / 配置码入口，不改原配置 |
| 供应商鉴权不支持 | 无法生成 OpenCodex 可用凭据 | 不做网页登录逆向，保留手动 API Key 方案 |
| 用户已有 OpenCodex | 配置覆盖或端口冲突 | 只读检测，复用或显式纳管，禁止静默覆盖 |
| 打包后发现功能缺陷 | 需要重新构建并发布 | 发布冻结门槛前完成 P5；正式包不做现场修改 |
| macOS 公证缺失 | Gatekeeper 警告或阻止安装 | 先作为测试包，不宣称正式无警告发布 |
| Windows Authenticode 缺失 | SmartScreen 未知发布者 | 保留 updater 签名，待证书配置后重新构建 |

## 8. 最终完成标准

开发完成：

- 用户可以从 Manager 启用 OpenCodex 多模型，不需要手动安装依赖。
- OSIRAPI 一键连接和通用连接码至少各有一条完整链路。
- GPT、Claude、Grok 模型可在 Codex 原生选择器中选择。
- 旧的直接 Provider、MCP、图片配置和用户原有配置不受影响。
- 所有失败路径都有备份、恢复和用户可理解的提示。

发布完成：

- 四个平台 Manager 安装包构建成功并通过平台门槛。
- OpenCodex 组件四平台清单可下载、校验和启动。
- latest.json 不再是 partial，所有签名和哈希与线上字节一致。
- 新用户在干净环境可以完成“安装 Manager → 连接 OSIRAPI → 看到多模型”的完整闭环。
- 灰度用户反馈的问题已分类，严重问题有停用和回滚方案。

## 9. 当前下一步

当前只完成方案和文档沉淀，不启动构建。下一步按顺序推进：

1. 确认 OpenCodex 组件的再分发许可和组件获取方式。
2. 固定通用连接码 Schema 与 OSIRAPI 交换接口。
3. 实现 P1/P2：组件管理和本地配置引擎。
4. 实现 P3/P4：OSIR 一键连接、配置码导入和极简向导。
5. 完成 P5 测试后冻结版本。
6. 再执行 P6 打包、线上发布和灰度。

## 10. 反思

最大的返工风险不是打包本身，而是组件分发方式、连接码协议和本地配置边界没有先冻结。先把这三件事确定，再开发和构建，后续才能避免“功能做完却无法在新用户环境安装”的问题。
