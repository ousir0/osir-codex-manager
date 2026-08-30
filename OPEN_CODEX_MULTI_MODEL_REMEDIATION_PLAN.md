# OpenCodex 多模型与 Codex 配置联动整改计划

## 结论

这批问题的共同根因不是单个按钮或单个样式，而是四套状态没有统一：

1. OpenCodex 本地服务是否启动。
2. Codex 当前是否由 OpenCodex 接管。
3. 模型目录中的模型、供应商和参数能力。
4. Codex 旧会话仍保存的供应商/模型上下文。

目标是把启动、切换、同步、会话迁移和重启确认收敛成一条可回滚的配置事务，并让界面只展示后端确认过的真实状态。

## 已确认的问题

### 启动按钮职责错位

- src/app/views/OpenCodexPrototype.tsx 的 connect() 只安装或启动服务；服务就绪时没有调用 openCodexActivateSaved()。
- src/app/views/CodexConfigWorkbench.tsx 的 activateMultiMode() 只有在已有路由时才调用 activate_saved；没有路由时没有统一的启动/授权动作。
- 后端 start() 只执行服务启动；真正写入 Codex config.toml、同步目录、迁移会话的是 activate_saved() 或 save()。

影响：用户看到本地服务已启动，但 Codex 仍使用默认配置，表现为“点击没有效果”。

### 默认配置与 OpenCodex 当前项错位

- effectiveMode 同时依赖 report.provider、report.openCodex.enabled 和 openCodex.enabled，不是单一后端事实。
- activate_default() 在解除 OpenCodex 接管前先做默认网关远程鉴权探测；401/403 会被呈现为切换失败，但界面没有清晰区分“目标配置不可用”和“当前 OpenCodex 仍在生效”。
- 配置文件、Manager 状态、目录、会话迁移和重启标记分散写入，失败回滚边界不完全一致。

### 供应商信息没有进入模型显示名

- model_display_name() 只根据模型 ID 生成 GPT-5.6 Sol 等名称。
- 路由标签同时承担平台标签和供应商名称，无法准确表达实际来源 OSIRAPI。
- provider/model 应保持稳定的内部路由标识，不能为了展示而修改。

### 推理强度被统一硬编码

src-tauri/src/app/opencodex.rs 的 build_opencodex_config() 为所有模型写入同一组推理档位，并统一使用 high。现有目录样本已经包含 supported_reasoning_levels，但同步时没有把它转成每个模型自己的能力信息。

影响：模型不支持某个档位时，选择器仍允许选择，最终请求参数报错。

### 选择器高度仍是固定值

- crates/codex-theme-engine/src/layout.rs 只防止原生模型行被压缩，没有让列表按内容增长。
- src/app/styles.css 中 .multi-model-model-list 固定 max-height: 220px，间距和行高偏大。

### 旧会话仍可能使用旧供应商上下文

src-tauri/src/app/codex_sessions.rs 只迁移 SQLite 中的 threads.model_provider 和 threads.model，没有明确的连接/凭据版本，也没有把凭据变更后的重启和会话重新绑定作为强约束。新对话走新配置而旧会话报密钥错误，符合这一缺口的表现。

当前基线还发现一个 OAuth 时序问题：OpenCodex 的 OAuth 回调监听器设置为非阻塞后，收到连接时可能立刻读取 socket，导致 Resource temporarily unavailable。该问题会直接表现为授权完成但 Manager 没有继续配置，必须与启动入口一起修复。

## 目标方案

### 1. 统一模式状态和切换事务

新增后端权威状态，至少包含：

- activeMode：default / opencodex / unavailable
- 服务、目录、凭据、会话迁移状态
- requiresCodexRestart
- 结构化的 lastTransitionError

把 activate_saved()、activate_default() 和必要的 save() 收敛到同一套流程：

1. 加锁，阻止重复点击、并发同步和重复 OAuth 回调。
2. 快照 config.toml、OpenCodex 配置、模型目录、Manager 状态和会话数据库。
3. 校验目标配置结构、协议、路由和默认模型。
4. 切到 OpenCodex：启动/重启服务、同步目录、确认可用模型、写入代理配置。
5. 切回默认：确认接管备份可恢复，恢复默认配置并解除 OpenCodex 所有权。
6. 迁移会话，写入状态和重启标记。
7. 回读所有关键文件并确认真实生效模式；任一步失败都恢复快照。

默认网关远程鉴权应与模式状态分开：

- 配置结构不合法：阻止切换。
- 401/403：明确返回“默认配置未切换，OpenCodex 仍生效”。
- 临时网络错误：模式和健康状态分离，不再把 OpenCodex 错显示为当前项。

### 2. 修复启动入口

前端只保留一个语义清晰的动作：

- 有保存且可验证的路由：准备服务 → 启用接管 → 同步目录 → 迁移会话 → 提示重启。
- 没有路由：准备服务 → 打开 OSIRAPI 授权 → 保存路由 → 验证 → 启用接管。
- 已经是 OpenCodex：显示重新检测/管理连接，不重复执行启用。

“服务已启动”不能再代替“Codex 已切换”。

### 3. 供应商显示格式采用后置标注

推荐格式：

    GPT-5.6 Sol · OSIRAPI
    Claude Opus 5 · OSIRAPI

模型名仍是第一扫描对象，供应商作为清晰标注；斜杠继续只用于内部 selector，例如 osirapi-openai/gpt-5.6-sol；真实模型 ID 不变。

实现上分离 platformLabel 与 providerName：OSIRAPI 路由统一显示 OSIRAPI，自定义路由使用用户填写的供应商名；同步时同时更新 OpenCodex 的 customModels.displayName 和目录中的 display_name。Manager 列表使用同一格式。

### 4. 按模型能力处理推理强度

为每个模型建立 modelId、supportedReasoningEfforts、defaultReasoningEffort 和 reasoningSupport（supported / unsupported / unknown）。

规则：

- 优先读取目录里的 supported_reasoning_levels。
- 默认值不在支持列表时回退为自动。
- 缺少能力信息时标记 unknown，不注入猜测档位。
- 不支持推理的模型隐藏或禁用控件，并发送不带该参数的请求。
- 切换模型时清理不兼容的当前值。
- model_reasoning_effort 只在当前模型确认支持时写入，否则移除或保持自动。

### 5. 调整选择器布局

原生 Codex 选择器：列表容器按内容自适应，最大高度为视口约 2/3，超出后内部滚动；收紧行高、内边距和间距，但保留键盘和点击区域；打开选择器、模型增删和窗口缩放后重新计算。

Manager 列表：移除 max-height: 220px，使用 max-height: min(66.666vh, 640px)；内容少时自然收缩，内容多时内部滚动；gap 调整到约 3px，行最小高度调整到约 30px。CSS 选择器继续限定在模型选择器作用域，不能影响其他菜单。

### 6. 修复旧会话与凭据生命周期

- 会话只保存对话身份和规范模型，不保存旧 Key。
- 供应商、Base URL、Key 或路由变化时，迁移所有受管理会话到当前有效路由；失效模型回退到路由默认模型。
- 旧 provider、裸模型和历史别名全部规范化。
- 凭据变更统一设置必须重启标记；重启后回读并确认新凭据版本，再清除标记。
- 无法安全映射的旧会话禁止静默发送，明确提示选择新模型。
- 会话迁移继续使用事务和备份，并返回更新、跳过、回退、无法映射数量。

## 建议改动范围

后端：

- src-tauri/src/app/opencodex.rs：统一切换事务、真实模式快照、供应商名、模型能力和回读校验。
- src-tauri/src/app/codex_config.rs：解耦默认网关健康检查；按模型能力校验推理强度；凭据变更统一设置重启/重新绑定。
- src-tauri/src/app/codex_sessions.rs：扩展规范化迁移和迁移报告。
- src-tauri/src/commands.rs：提供统一切换命令或让现有命令共享事务实现。
- crates/codex-theme-engine/src/layout.rs：实现自适应高度、2/3 上限和紧凑间距。

前端：

- src/app/views/CodexConfigWorkbench.tsx：只消费后端真实模式；显示明确的未切换/仍由 OpenCodex 生效状态；推理档位按模型过滤。
- src/app/views/OpenCodexPrototype.tsx：合并启动、授权、启用、同步入口；连接完成后重新读取父级配置报告。
- src/shared/types.ts、src/services/managerApi.ts：增加模式快照、模型能力、供应商显示名和迁移报告。
- src/app/styles.css：收紧间距，移除固定高度，增加视口上限。

## 执行顺序

### 阶段一：先修切换正确性

统一启动/启用入口、OAuth 回调读取、当前模式判定、默认/多模型事务切换、回滚和默认网关错误文案。

验收：默认配置、OpenCodex 已启用、服务未启动、无路由、默认鉴权失败五种状态互相切换时，界面、磁盘配置和调用链路一致。

### 阶段二：修模型目录和推理参数

引入供应商显示名，读取每个模型的推理能力，删除统一硬编码，完成旧目录归一化。

验收：同名模型能区分来源；不支持推理的模型不显示可选档位；支持模型只显示自己的档位。

### 阶段三：修旧会话和凭据生命周期

扩展迁移报告，Key/Base URL/路由变化统一触发重启，重启后回读确认。

验收：切换供应商并重启后，旧、新会话都走新渠道；失效模型明确提示，不再静默使用旧 Key。

### 阶段四：修布局和回归

完成原生选择器和 Manager 列表布局调整，验证不同窗口高度、模型数量和平台。

验收：1、4、18、50 个模型分别验证；少量模型紧凑，超出约 2/3 后内部滚动，其他菜单不受影响。

## 必须新增的测试

- 默认配置下点击 OpenCodex：有保存路由时实际调用启用接口；无路由时进入安装/授权，不出现无响应。
- OAuth 回调在非阻塞监听下可稳定读取，不能因一次暂时不可读而中断授权流程。
- 启用失败时当前模式不改变；默认网关 401/403 时明确显示 OpenCodex 仍生效。
- OAuth 完成后父级报告、当前模式和重启标记同步。
- 模型展示为“模型名 · 供应商名”，但 slug 和真实模型 ID 不变。
- 模型切换时推理档位随能力更新，不能保存不支持的值。
- 切换事务任一步失败时恢复 config、目录、状态和备份。
- 旧 provider、裸模型、失效模型的会话迁移有备份和报告。
- Key 变化后必须重启，重启确认前不能清除标记。
- 原生布局注入只作用于模型选择器，且高度最多约占界面 2/3。

## 风险与控制

- 配置、目录和 SQLite 同时变化可能造成 Codex 暂时不可用。控制：统一快照、原子写入、事务迁移、回读确认。
- OpenCodex 目录字段可能随版本变化。控制：先用当前运行时的 config validate 和目录样本确认字段；未知能力按 unknown 处理。
- 原生选择器属于 Codex renderer 实现细节。控制：窄选择器、版本探测、失败可回退，不修改 Codex 安装包。
- 旧会话无法安全映射时，强行回退可能比提示用户更危险。控制：禁止静默使用旧供应商上下文。

## 官方配置核对

本计划以官方 OpenAI 文档中的 Codex 自定义供应商配置和推理参数原则为约束：

- https://developers.openai.com/codex/config-advanced
- https://platform.openai.com/docs/guides/reasoning

落地原则：供应商名称属于展示元数据；真实路由保持稳定的 provider/model 标识；推理强度必须以当前模型实际能力为准。

## 反思

真正需要修的是配置切换的状态机，而不是只给按钮补一个调用。否则下一次换供应商、刷新目录或重启 Codex，状态错位仍会回来。

## 下一步

先实施阶段一的统一切换事务和启动入口，再进入模型能力、会话兼容和布局改造。
