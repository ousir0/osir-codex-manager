归档说明：此文件是基于 19ce08b 的历史审查方案，不代表当前实现状态。工作区已同步正式发布 v0.5.51（5f4b8d8），包含 PR #41、#42 的原生生图配置和凭据修复。下文状态和测试计划均保留原始记录；本次归档未执行真实授权或生图测试。

# osirAPI 授权后原生生图：方案与测试

状态：仅方案，未修改业务代码，未执行授权、生图或配置迁移。
审查基线：管理器提交 `19ce08b`；以下路径相对管理器仓库，行号对应该基线。
适用规则：`/Users/ouwei/AGENTS.md`；管理器及 docs 下未发现更深层 AGENTS。交叉阅读 OpenCodex 时已读取其根目录及 src/AGENTS.md。

## 结论

当前兼容模式默认关闭，不是默认开启；但授权保存和升级迁移均未显式开启原生 image_generation，也未补齐 OpenCodex images.provider。旧配置会保留关闭原生工具的状态，因此重新授权同一个 Key 不足以完成迁移。

目标：用户更新管理器并重新授权原 osirAPI Key，完成必要重启后，在新对话自然语言生图，无需安装或调用技能。不能把“兼容模式关闭”当成“原生工具已可用”的证明。

## 已确认事实与代码证据

| 位置 | 当前行为 | 影响 |
| --- | --- | --- |
| `src/app/views/CodexConfigWorkbench.tsx:40` | 前端初始 imageGenerationCompatibility=false | 新表单默认关闭兼容模式 |
| `src/services/managerApi.ts:746` | 浏览器回退报告同样为 false | 仅回退默认，不代表真实磁盘配置 |
| `src/app/views/CodexConfigWorkbench.tsx:143` | 读取报告后用真实配置覆盖表单 | 老用户不会因前端默认值而自动恢复原生工具 |
| `src-tauri/src/app/codex_config.rs:379` | 显式 features.image_generation 优先，兼容模式取反 | 原生 false 对应兼容 true；原生 true 对应兼容 false |
| `src-tauri/src/app/codex_config.rs:389` | 无显式开关时，requires_openai_auth=false 且存在旧 actor 授权头则判兼容 true；其他情况 false | 旧标记也会影响界面；本审查没有读取真实用户配置 |
| `src-tauri/src/app/codex_config.rs:1481` | 保存行为设置写入 image_generation=!compatibility | 开启兼容模式实际关闭原生工具；关闭时显式开启 |
| `src-tauri/src/app/opencodex.rs:3218`、`:3248`、`:3086` | OAuth 交换、解密、生成路由、调用 save | 同 Key 重授权走同一路径，没有生图专项迁移 |
| `src-tauri/src/app/opencodex.rs:4034`、`:4125` | 合并供应商并更新 Key，保留其他顶层配置 | 不生成 images.provider；已有错误指向也会保留 |
| `src-tauri/src/app/opencodex.rs:4194` | 写入本机 Responses provider、模型和目录绑定 | 不处理 image_generation，旧 false 保留 |
| `src-tauri/src/app/opencodex.rs:525` | 升级整理模型、重启、sync、修复会话 | 没有补齐图片供应商与原生开关 |
| `src-tauri/src/app/opencodex.rs:130`、`:533` | 版本变化或待迁移标记触发完整迁移；同版本已完成时走会话修复 | 仅增加保存逻辑不能修复只升级的存量用户；同版本重试也需覆盖 |
| `src-tauri/src/app/opencodex.rs:495` | Codex 运行时延后更新迁移 | 应明确重启后验收，不承诺当前对话即时生效 |
| `src-tauri/src/app/codex_config.rs:779`、`:1189`、`:608` | 独立生图配置安装技能，读取配置时还会补装 | 老技能可能继续参与工具选择；原生路径不应调用这套安装逻辑 |

目标文件定位见 `src-tauri/src/app/opencodex.rs:885`：Codex home 下 config.toml，用户目录下 .opencodex/config.json。

## 最小实现方案（尚未实施）

1. 在 opencodex.rs 增加共用、幂等的生图配置修复逻辑，授权保存和升级迁移都调用。先识别可信的管理器托管 osirAPI 路由，不靠默认聊天路由或单独的名字包含 openai 来选择。候选必须启用、adapter=openai-responses、使用 Key 且凭据可用。多候选需稳定选择，优先保留有效绑定。
2. 设置 images.provider 为该已有供应商 ID，直接复用供应商 Key，避免复制到独立图片配置。保留 images 的其他字段；用户已有有效自定义图片供应商默认保留。失效的自定义指向应报告，不能静默切换计费渠道。
3. 对符合条件的托管 osirAPI 路径显式设置 features.image_generation=true，迁移旧 false。通用 write_codex_proxy_config 不应无条件开启所有供应商。旧 false 无来源标记时无法区分主动禁用与历史兼容设置，产品规则需限定为本次明确选择原生生图的托管路径。
4. 授权保存沿用 `opencodex.rs:4792` 的验证、快照和失败恢复；升级也要保证两份配置作为同一变更处理，成功后再写完成标记。为迁移设置独立修订或幂等检查，避免同版本早退漏修；无合格候选不得声称迁移成功。
5. 无技能验收先用干净配置证明原生链路。旧用户若仍有管理器安装的 relay 技能，后续只处理可识别且未被用户修改的管理器资产，并避免 `codex_config.rs:608` 自动补装；不删除用户自定义技能或凭据。此项是存量用户体验补充，不能用技能回退冒充原生验收成功。

预期配置只包含 provider 引用与原生开关，不需要新增生图 Key。候选模型是否有图片权限由端到端测试确认，不把图片模型强行塞进聊天模型目录。

## 发布阻断风险

- 实际组件能力未核验：管理器 `components/opencodex/index.json:4` 为 2.22.0，`scripts/build-opencodex-component.mjs:119` 从 npm 打包。交叉参考的 `/Users/ouwei/opencodex/src/providers/openai-sidecar.ts:132` 支持 images.provider，`:156` 要求 API-key openai-responses，`src/server/images.ts:413` 使用其 Key 请求图片端点；但本地源码不能证明分发包相同。必须测试实际组件；不支持则组件更新属于最小交付必要项。
- 权限与端点：授权流程不验证真实图片调用（opencodex.rs:3144），同 Key 可聊天不代表可生图。失败应给出权限/端点错误，不静默借用其他账号。
- 生效时机：旧会话与运行中进程可能保留旧配置，完整重启并新建对话才作为首轮验收条件。
- 非幂等请求：真实图片请求可能收费，超时不自动重发；日志不得包含 Key、授权头或请求敏感内容。

## 测试计划（未执行）

所有自动化测试用隔离临时目录、假 Key、本地假上游；不调用会改真实配置的管理器 status/save 接口。

| 场景 | 必须断言 |
| --- | --- |
| 空配置首次授权 | 绑定正确图片供应商、原生 true、不创建 relay 凭据或技能 |
| 同 Key 再授权两次 | 生图绑定稳定、不重复配置、原聊天路由保留 |
| Key 轮换 | 图片请求使用供应商新 Key，无旧 Key 副本；只输出匹配布尔值 |
| 默认兼容状态 | 无开关无旧标记为 false；原生 false 为 true；原生 true 为 false |
| 旧标记兼容 | 无显式开关且满足旧 actor 标记时为 true；显式开关覆盖旧标记 |
| 旧原生 false 升级 | 合格托管 osirAPI 路径迁移为 true，与图片绑定一起生效 |
| 只升级、不重授权 | 使用已保存的供应商凭据完成迁移，不要求再次输入 Key |
| 同版本完成标记存在 | 生图修订未完成仍能修复；已完成重复检查无额外变更 |
| 自定义图片供应商 | 有效绑定及其他 images 字段保留，不静默更改计费渠道 |
| 候选缺失/禁用/错误 adapter | 不绑定到任意聊天供应商，明确失败或待处理状态 |
| 中途失败 | JSON/TOML 恢复一致，完成标记未推进，可安全重试 |
| Codex 正在运行 | 不迁移配置或重启服务，退出后执行并清除待处理状态 |
| 实际分发组件 | 接受 images.provider，图片请求到假上游正确路径，携带匹配凭据 |
| 旧 relay 技能存在 | 原生验收不得依赖技能；单独检查补装逻辑，保留用户修改资产 |

管理器现有测试扩展位置：opencodex.rs:5695（生成）、:5730（保留 Key）、:5795（更新）、:5826（回滚）、:5491（运行中延后）、:6167（Codex 配置）；codex_config.rs:1816（兼容模式开关）。

实施后建议命令（本次不执行）：

```sh
cargo test --manifest-path /Users/ouwei/codex-app-manager/src-tauri/Cargo.toml --lib app::opencodex::tests
cargo test --manifest-path /Users/ouwei/codex-app-manager/src-tauri/Cargo.toml --lib app::codex_config::tests
```

组件端参考 `/Users/ouwei/opencodex/tests/server-images.test.ts:293` 的假上游测试，但最终必须对实际打包组件执行同等验证，不能只跑另一份源码。

最终用户验收：干净隔离配置 → 授权同一个测试 osirAPI Key → 完整重启 Codex → 新对话输入“生成一张蓝色杯子的图片” → 原生工具返回可显示图片；没有技能安装、脚本执行或第二把图片 Key。另以旧 false 配置重复流程，并验证聊天仍正常。真实生图可能计费，应使用获授权测试账号，禁自动重试。

## 反思与下一步

根因是配置链路未闭合，不是缺少技能。下一步优先核验实际 2.22.0 分发组件能力，再确定实现是否可以只落在管理器；本文件不代表已验证自然语言生图成功。
