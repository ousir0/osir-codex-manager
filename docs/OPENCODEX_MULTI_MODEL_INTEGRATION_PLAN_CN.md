# Codex Manager × OpenCodex 多模型集成方案

## 结论

采用 OpenCodex 的本机代理与模型目录方法，但不迁移 OpenCodex 项目、不复制其管理界面、也不把多供应商路由搬进 Codex Manager。

Codex Manager 新增一个 OpenCodex 集成属性与独立配置面板：用户在这里维护多个上游模型来源；Manager 安全写入 OpenCodex 本机配置，启动并同步服务；Codex 最终只连接一个本机 Provider，因此原生模型选择器会显示多供应商模型。

这条路径的改动集中在现有 CODEX 配置管理，不改 Codex 应用包，也不改 OSIR API 的实时路由和计费逻辑。

## 已确认的事实

- 历史可用方案位于 /Users/ouwei/sub2api-new/Codex 多模型适配教程.md。
- 历史安装器位于 /Users/ouwei/sub2api-new/integrations/codex-setup/index.mjs。
- 该方案通过 OpenCodex 本机服务把 Codex 指向 http://127.0.0.1:10100/v1，并使用 Responses 协议。
- 多模型选择器由 OpenCodex 同步生成的模型目录驱动；模型内部路由格式是 provider/model，显示名称可独立处理。
- 当前 Manager 已能安全读取、备份、原子写入 ~/.codex/config.toml，且已支持多个 Codex Provider、模型获取和配置恢复。

## 推荐架构

~~~text
Codex 原生模型选择器
        │ 读取模型目录
        ▼
Codex config.toml
  model_provider = opencodex
  base_url = http://127.0.0.1:10100/v1
        │
        ▼
OpenCodex 本机服务
  - 多个 Provider
  - 各自 API Key
  - provider/model 路由
  - 模型目录同步
        │
        ├── OSIR API：GPT / Claude / Grok 等分组
        ├── 其他 OpenAI Compatible 服务
        └── 后续可扩展的其他协议
~~~

职责边界：

| 组件 | 负责什么 | 不负责什么 |
| --- | --- | --- |
| Codex Manager | 安装、配置、备份、启动、健康检查、模型同步、恢复 | 不转发模型请求、不保管服务端上游 Key |
| OpenCodex | 本机代理、provider/model 路由、模型目录生成 | 不替代 Manager 的安装与 UI 管理 |
| OSIR API | 用户权限、API Key、模型权限、用量、计费、服务端路由 | 不直接写用户电脑的配置文件 |
| Codex | 展示模型选择器并调用本机代理 | 不感知每个模型实际来自哪个上游 |

## 为什么这是最小改动

当前 Manager 已有 Provider 列表和 config.toml 的原子写入能力，但普通 Provider 模式假设只有一个 Base URL 和一个 auth.json API Key。OpenCodex 模式需要多个 Key 和多个模型来源，直接塞进现有基础表单会导致：

- 一个全局 API Key 覆盖多个 Provider 的 Key；
- 通用保存逻辑会把 requires_openai_auth 强制写回 true，破坏本机代理；
- 手动拼接模型目录容易与 Codex 版本变化不兼容。

因此新增独立的 OpenCodex 集成对象，而不是把多个 Key 扩展到普通 Provider 表单。Codex Manager 复用已有配置页面、备份能力、错误提示和模型卡片，不重做 OpenCodex 的路由内核。

## 目标数据模型

### 1. Codex 配置报告新增属性

~~~text
openCodex: {
  enabled: boolean
  installed: boolean
  version: string | null
  port: number
  serviceState: stopped | starting | ready | unhealthy | unknown
  codexProviderId: string
  catalogPath: string | null
  modelCount: number
  lastSyncAt: string | null
  backupAvailable: boolean
  error: string | null
}
~~~

### 2. OpenCodex 路由配置

~~~text
OpenCodexConfigInput {
  enabled: boolean
  port: number                 // 默认 10100；冲突时显式选择其他端口
  codexProviderId: string      // 默认 opencodex，保持稳定，不随模型变化
  defaultRoute: provider/model
  routes: [
    {
      id: string               // 例如 osir-gpt、osir-claude、custom-team
      label: string            // 用户可读名称
      adapter: openai-responses
      baseUrl: string
      apiKey?: string          // 仅写入，不回显、不进入 config.toml
      apiKeyConfigured: boolean
      models: [string]
      defaultModel: string
      enabled: boolean
    }
  ]
}
~~~

第一版只开放 OpenAI Responses 适配器。这样可以先覆盖 OSIR 的 GPT、Claude、Grok 分组及大部分 OpenAI Compatible 服务；Anthropic/Gemini 等协议适配留到第二版，避免一次引入大量协议差异。

## 文件写入规则

### Codex 的 config.toml

只维护带 Manager 标记的单个 Provider 块，示例：

~~~toml
model_provider = "opencodex"
model = "osir-gpt/gpt-5.6-sol"
model_catalog_json = "/Users/name/.codex/opencodex-catalog.json"

[model_providers.opencodex]
name = "OpenCodex 多模型路由"
base_url = "http://127.0.0.1:10100/v1"
wire_api = "responses"
requires_openai_auth = false
~~~

普通 Provider、MCP、用户自定义 TOML 和 auth.json 保持不动。启用 OpenCodex 后，普通 API Key 区域显示为“该模式不使用 auth.json”；各路由 Key 只存放在 OpenCodex 私有配置中。

### OpenCodex 的配置与目录

- 写入 ~/.opencodex/config.json，文件权限按用户目录私密文件处理。
- 只增删由 Manager 自己记录的 Provider 和 customModels，不动用户原有项目。
- 每次写入前备份 config.toml、models_cache.json、opencodex-catalog.json 和 OpenCodex 配置。
- 先生成候选 JSON，调用 OpenCodex 配置校验，通过后原子替换。
- 配置后执行 service、health、ready、sync；sync 后回读 config.toml 和目录，确认没有被覆盖。

## 模型选择器实现方式

不要由 Manager 自己伪造 Codex 的模型目录格式。

正确流程：

1. Manager 保存路由配置。
2. Manager 调用 OpenCodex 的同步命令。
3. OpenCodex 生成合法目录文件到 ~/.codex/opencodex-catalog.json。
4. Manager 将 model_catalog_json 指向这个目录。
5. Codex 完全退出后重新打开，选择器按目录显示模型。

内部路由始终保留 provider/model，例如 osir-claude/claude-opus-5；显示名默认使用 模型名 · Provider 名，避免 GPT、Claude、Grok 出现重名且不破坏真实路由。

## 产品界面建议

在 CODEX 配置管理的基础页顶部增加“连接模式”：

- 直接 Provider：保留现有行为，使用一个 Base URL 和 auth.json API Key。
- OpenCodex 多模型：显示状态卡、路由列表、同步按钮和恢复按钮。

OpenCodex 模式的最小交互：

1. 安装或检测 OpenCodex。
2. 选择端口，并展示占用与健康状态。
3. 添加路由：名称、Base URL、API Key、获取模型、勾选模型、默认模型。
4. 保存并同步。
5. 显示“已写入 N 个模型；请完全重启 Codex”。
6. 支持测试每条路由的一次最小文本请求；测试全部模型前必须单独确认可能产生费用。
7. 支持“停用并恢复上次配置”，但卸载 OpenCodex 必须单独二次确认。

## 面向新用户的安装与适配流程

用户不应先安装 Node、npm 或 OpenCodex，再学习一套命令。Manager 应把 OpenCodex 作为可选的“多模型组件”，通过向导完成安装与接入。

### 推荐用户路径

1. 用户安装并打开 Codex Manager。
2. 在 CODEX 配置管理中选择 OpenCodex 多模型，点击启用多模型。
3. Manager 做环境检查并清楚展示结果：Codex、OpenCodex、端口、已有配置、网络和运行状态。
4. 用户确认安装后，Manager 只在当前用户目录安装受控版本的 OpenCodex 运行组件，不要求管理员权限。
5. Manager 启动当前用户级后台服务，等待 health 和 ready 成功。
6. 用户在 Manager 内添加 OSIR 预设或自定义路由，填写各路由 Key 并同步模型。
7. Manager 备份配置、写入代理 Provider、校验模型目录，并提示用户完全退出后重新打开 Codex。

### 不同环境的处理规则

| 用户环境 | Manager 动作 | 禁止动作 |
| --- | --- | --- |
| 全新电脑，没有 Codex | 先引导完成 Manager 现有的 Codex 安装流程，再进入多模型向导 | 不假设 Codex Home 已存在 |
| 有 Codex，没有 OpenCodex | 安装受控版本，创建首次备份，再写入多模型配置 | 不要求用户打开终端运行 npm |
| 已有健康 OpenCodex | 读取版本、端口和配置，展示“复用”或“纳入 Manager 管理” | 不覆盖已有 Provider 和模型 |
| 端口 10100 被其他程序占用 | 若确认是健康 OpenCodex 则复用；否则让用户选择新端口 | 不终止未知进程 |
| Node 或 npm 缺失 | 使用 Manager 受控运行时或受控安装器补齐依赖 | 不依赖系统全局 npm，也不要求 sudo |
| 网络不可用或下载失败 | 保持原配置不变，提供重试和离线诊断 | 不写入半套配置或切换 Codex Provider |
| OpenCodex 更新后不兼容 | 保留已验证版本，失败自动回退到上次可用组件和配置 | 不自动升级到未验证版本 |

### 组件分发建议

正式面向用户时，不建议依赖用户的全局 npm 安装。更稳妥的方式是：

- Manager 从自己的版本清单下载经过哈希校验的 OpenCodex 组件与必需运行时。
- 安装位置放在 Manager 管理的用户目录，例如 ~/.codex-manager/runtime/opencodex/版本号。
- 后台服务只以当前用户身份运行：macOS 使用 launchd，Windows 使用 Task Scheduler；不申请管理员权限。
- 组件更新由 Manager 显式检查、用户确认、备份、健康验证和回退组成，不跟随 OpenCodex 自动更新。
- 分发前必须确认 OpenCodex 的许可证、再分发条件和依赖运行时许可；未满足时才采用“检测 Node 后受控 npm 安装”的兼容降级路径。

### 新用户向导的完成标准

- 用户不需要提前了解 Node、npm、端口、config.toml 或模型目录。
- 用户只需完成两件事：选择多模型模式，以及在 Manager 内填写自己的 API Key。
- 任一安装、服务或同步步骤失败时，Manager 都能说明失败位置、保留原状并给出重试入口。
- 用户可在任何时刻停用多模型模式并恢复到启用前的 Codex 配置。

## 极简使用体验与 OSIR 一键接入

首版不应让用户先理解 Provider、端口、Adapter、模型目录或 OpenCodex。默认界面只提供两条入口：

1. 连接 OSIRAPI：适合 OSIR 用户，一键完成多模型接入。
2. 导入配置码：适合从 OSIR 控制台、团队管理员或其他可信来源拿到配置的人。

“自定义路由”放入高级设置，不出现在首次配置的主流程。

### 一键连接 OSIRAPI

用户点击连接 OSIRAPI 后，Manager 只展示一个简短进度页：

~~~text
检测环境 → 安装多模型组件 → 连接 OSIRAPI → 同步模型 → 完成
~~~

推荐接入合同：

1. Manager 打开 OSIR 登录页、Deep Link 或设备授权页。
2. 用户完成 OSIR 登录与授权。
3. OSIR API 签发一次性、短时、仅可消费一次的多模型连接票据。
4. Manager 用票据换取加密配置包，包内包含用户有权使用的路由、模型和专用 Key。
5. Manager 安装或复用 OpenCodex，写入本机私密配置并同步模型目录。
6. Manager 只显示“已接入 GPT、Claude、Grok 等 N 个模型”，不回显 Key。

OSIR 侧默认提供 GPT、Claude、Grok 三个预设路由。用户只需要选择“全部启用”或取消某一个类别；高级模型筛选以后再配置。

### 导入配置码，而不是执行自由文本指令

用户可能需要复制内容到 Manager 完成配置，但这个内容应是“连接码”或“已签名配置包”，不能是任意自然语言指令。

推荐支持三种等价输入：

- 从 OSIR 控制台复制的一次性连接码。
- OSIR Deep Link。
- OSIR 控制台展示的二维码。

Manager 对连接码执行格式、签名、来源、过期时间、单次消费和目标域名校验，再向 OSIR API 获取配置。这样用户可以极简粘贴完成接入，同时避免“粘贴一段未知文字就改写本机配置或执行命令”的安全风险。

连接码不应包含明文 API Key；剪贴板中的值过期后自动失效。Manager 消费成功后应清空输入框，并提示用户不要转发该码。

### 首次配置页面

首次页只保留以下内容：

| 区域 | 用户看到的内容 | 用户动作 |
| --- | --- | --- |
| 状态 | 未启用多模型，或已就绪 N 个模型 | 查看即可 |
| 主按钮 | 连接 OSIRAPI | 点击后登录或授权 |
| 次按钮 | 粘贴配置码 | 粘贴一次性连接码 |
| 进度 | 5 个可读步骤和当前状态 | 等待或重试 |
| 完成页 | 已接入的模型类别、模型数量、重启提示 | 打开或重启 Codex |
| 高级 | 自定义路由、端口、手动模型选择、诊断、恢复 | 仅需要时展开 |

每一步的文案只回答三件事：正在做什么、是否需要用户操作、失败后下一步怎么做。例如“正在安装多模型组件，通常需要 1 到 3 分钟；请不要关闭 Manager”。

### 完成后的日常界面

日常状态卡建议显示：

~~~text
多模型：已就绪
来源：OSIRAPI
模型：18 个，覆盖 GPT / Claude / Grok
上次同步：刚刚
操作：同步模型｜管理模型｜停用并恢复
~~~

默认不显示端口、OpenCodex 版本、Provider ID 或密钥状态；这些只在“诊断与高级设置”中提供。用户日常只需同步模型或重新打开 Codex。

## 实施分期

### P0：本机配置闭环

- 新增 Rust 模块 opencodex_config，负责检测、备份、配置、同步、恢复和状态读取。
- 新增 Tauri 命令：status、install、save、sync、test-route、restore。
- 扩展 shared types、managerApi 与 CodexConfig 页面。
- 只支持 OpenAI Responses 路由。
- 覆盖 OSIR 三个预设：GPT、Claude、Grok；同时允许添加自定义路由。
- 通过 OpenCodex 生成目录并验证 Codex 冷启动后可读。

### P1：可运营性

- 保存 Manager 自己的受管清单、配置版本和备份索引。
- 检测端口冲突、服务异常、目录缺失和外部同步覆盖。
- 路由健康状态、单路由测试、脱敏诊断导出。
- 支持模型搜索、排序和按 Provider 分组。

### P2：协议扩展

- 在确认 OpenCodex 对应版本稳定后，增加 Anthropic、Gemini 等 Adapter 预设。
- 每个新 Adapter 独立增加配置校验、请求测试和回退测试。

## 风险与护栏

| 风险 | 影响 | 处理方式 |
| --- | --- | --- |
| OpenCodex 更新改变配置格式 | 同步后选择器丢模型或路由错位 | 固定已验证版本；更新前备份；同步后回读校验；失败自动恢复 |
| 10100 被其他进程占用 | Codex 指向错误本机服务 | 仅复用健康 OpenCodex；否则让用户显式选择端口，不杀进程 |
| 多 Provider Key 泄露 | 直接造成账户风险 | Key 只写入本机私密配置；UI 只显示配置状态；日志与诊断脱敏 |
| 通用基础配置保存覆盖代理字段 | 多模型模式失效 | OpenCodex 走独立命令，不复用会强制 requires_openai_auth=true 的普通保存逻辑 |
| 模型可见但实际无权限 | 用户选择后请求失败 | 将“目录同步成功”和“调用成功”分开显示；每条路由提供最小请求验证 |
| 外部路由别名接管模型 | 模型选择与实际 Provider 不一致 | 保存前检测 combo、routing profile、subagent fallback 冲突，发现冲突不写入 |

## 验收标准

1. 用户可在 Manager 中新增至少三个路由：GPT、Claude、Grok。
2. 每条路由可独立配置 Base URL、API Key、模型列表和默认模型。
3. Manager 不显示、不日志记录完整 API Key。
4. 保存后 Codex 使用一个稳定的 OpenCodex Provider，且 requires_openai_auth=false。
5. OpenCodex 同步后，Codex 模型选择器展示各路由模型，显示名与内部 provider/model 路由一致。
6. 删除或更新某一条 Manager 受管路由，不影响用户原有 OpenCodex Provider 和自定义模型。
7. 端口、服务、同步、目录或最小请求验证失败时，保留可恢复备份，不留下半配置。
8. 停用 OpenCodex 可恢复启用前的 Codex 配置，不删除用户其他 Provider 或 auth.json。

## 当前决策

首版默认采用“OpenCodex 本机代理 + OpenAI Responses 路由 + OSIR 三路预设 + 自定义路由”的方案。它覆盖多供应商模型选择的核心体验，改动集中且可回退；不在首版引入服务器迁移、服务端路由重写或多协议适配。

## 反思

这里解决的是 Codex 原生选择器如何稳定看到多模型，而不是再造一个模型网关。最大的前提是 OpenCodex 继续提供稳定的本机服务、配置校验和目录同步能力；因此首版必须把版本固定、备份和同步后校验作为产品功能，而不是安装细节。

## 通用供应商连接协议

连接码不应设计成 OSIR 专用 Key 包，而应设计成供应商无关的连接描述。建议使用带版本和签名的 JSON 信封，再通过 Base64URL、Deep Link 或二维码承载：

~~~json
{
  "schema": "codex-manager.connection/v1",
  "connection_id": "conn_01...",
  "issuer": "https://api.osirclaw.com",
  "audience": "codex-manager",
  "provider": {
    "id": "osirapi",
    "label": "OSIR API",
    "protocol": "openai-responses"
  },
  "auth": {
    "method": "one_time_ticket",
    "exchange_url": "https://api.osirclaw.com/v1/manager/connections/exchange",
    "expires_at": "2026-08-18T12:00:00Z",
    "single_use": true
  },
  "scope": {
    "models": ["gpt-5.6-sol", "claude-opus-5"],
    "capabilities": ["responses"]
  },
  "signature": {
    "algorithm": "Ed25519",
    "key_id": "osir-manager-2026-01",
    "value": "..."
  }
}
~~~

核心字段保持通用：供应商标识、协议类型、鉴权方式、交换地址、有效期、单次消费、模型范围、能力范围和签名。OSIR 的 GPT、Claude、Grok 只是其中一个供应商实现，其他渠道可以复用同一格式。

连接码只携带短时票据和公开元数据，不携带长期 API Key、Refresh Token、浏览器 Cookie 或密码。Manager 必须校验签名、issuer、audience、过期时间、单次消费和 HTTPS 地址；校验失败时不写入任何本地配置。

### 支持的鉴权方式

| 鉴权方式 | 适用场景 | Manager 动作 |
| --- | --- | --- |
| one_time_ticket | OSIR API、团队控制台、服务端签发配置 | 交换短时票据，获得专用路由配置和 Key |
| oauth_device | 有官方 OAuth 或设备码登录的供应商 | 打开登录页或展示设备码，完成授权后获得官方令牌/配置 |
| browser_oauth | 只提供标准浏览器授权流程的供应商 | 使用系统浏览器完成授权，不读取登录密码和 Cookie |
| api_key | OpenAI Compatible 或用户自建网关 | 用户在 Manager 内输入 Key，Key 只写入受控私密存储 |
| imported_bundle | 团队管理员分发的签名配置包 | 校验签名、版本、来源和权限后导入，禁止任意脚本执行 |

如果供应商没有官方 OAuth/API，也没有明确允许的配置交换接口，Manager 不应抓取网页、提取 Cookie 或模拟网页登录。此时只支持用户手动填写 API Key，或者由供应商提供正式的签名配置包。

### 供应商适配器接口

Manager 核心只处理统一连接协议，供应商差异放到适配器：

~~~text
ProviderConnector {
  providerId()
  supportedProtocols()
  supportedAuthMethods()
  beginAuthorization()
  completeAuthorization()
  exchangeConnection()
  discoverModels()
  buildOpenCodexRoute()
  healthCheck()
}
~~~

首版内置三个适配器：

- OSIR API：one_time_ticket，默认生成 GPT / Claude / Grok 路由。
- OpenAI Compatible：api_key，用户填写 Base URL、Key 和模型。
- OAuth-capable provider：只在确认官方 OAuth 合同后接入，不做网页逆向。

OpenCodex 当前首版仍统一使用 OpenAI Responses 路由。供应商登录成功后，适配器必须把授权结果转换成 OpenCodex 可使用的安全凭据和协议配置；不能因为“登录成功”就假设该供应商能被 OpenCodex 调用。

### 通用连接流程

~~~text
识别连接码 schema
  -> 校验签名、来源、受众、过期时间
  -> 选择 provider connector
  -> 执行 one-time ticket / OAuth / API key / bundle 鉴权
  -> 获取协议、路由、模型和能力
  -> 生成 OpenCodex 受管配置
  -> 启动并同步 OpenCodex
  -> 用每个供应商的最小请求验证真实可调用性
~~~

界面始终使用统一文案“连接供应商”，只在鉴权步骤显示“打开 OSIR 登录”“输入 API Key”或“输入设备码”等具体动作。用户不需要理解连接码内部格式，也不需要手动编辑 JSON。
