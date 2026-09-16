# OSIR 国产多模型本机配置验收

日期：2026-09-17（Asia/Shanghai）

- 已通过已安装的 Codex Manager 添加 OSIR 国产多模型（osir-domestic）。
- Base URL：https://api.osirclaw.com/v1；密钥仅保存在本机配置，报告不记录密钥。
- 23 个新增模型已全部进入 OpenCodex 服务列表、Codex 目录及缓存。
- 全量同步也刷新了原供应商，总数为 145；原默认模型保持不变。
- 管理器内置验证通过；已安装 Codex 自带 CLI 使用 deepseek-v4.1-flash 返回 OK，退出码 0。
- 桌面端当前窗口的模型选择器尚需完全退出并重新打开后验收。

| 模型 | 最小文本调用 |
|---|---|
| MiniMax-M2.7 | 成功 |
| MiniMax-M2.7-highspeed | 成功 |
| MiniMax-M3 | 成功 |
| deepseek-v4-flash | 成功 |
| deepseek-v4-flash-0731 | 成功 |
| deepseek-v4-flash-vision-exp | 成功 |
| deepseek-v4-pro | 成功 |
| deepseek-v4-pro-0813 | 成功 |
| deepseek-v4.1-flash | 成功 |
| deepseek-v4.1-flash-0910 | 上游暂不可用；按 15/30/60 秒间隔重试 3 轮仍失败 |
| deepseek-v4.1-flash-expires-on-0910 | 成功 |
| glm-5.2 | 成功 |
| glm-5.3 | 成功 |
| glm-5.3-flash | 成功 |
| k3 | 成功 |
| kimi-k2.6 | 成功 |
| kimi-k2.7-code | 成功 |
| kimi-k3 | 成功 |
| mimo-v2.5 | 成功 |
| mimo-v2.5-pro | 成功 |
| qwen3.7-max | 成功 |
| qwen3.8-flash | 上游权限拒绝；直接请求 Responses 和 Chat Completions 均复现 |
| qwen3.8-max | 成功 |

流式工具调用通过：deepseek-v4.1-flash（auto）、glm-5.3、MiniMax-M3、kimi-k3、mimo-v2.5、qwen3.8-max。

测试证明当前最小对话和代表模型工具协议可用，不代表全部复杂编码或图片能力已验证。

下一步：完全退出并重新打开 Codex，在选择器搜索 Deepseek。后续新增模型可在管理器点击同步。两个失败模型保留在目录中，使用时可能报错，需供应商恢复。


## 选择器缺失修复（2026-09-17）

- 根因：已安装 Codex 前端 Bwa 默认 limit=100；Uwa 只请求 cursor=null 并消费 data，未跟随 nextCursor。修复前自定义 23 个模型位于 137 项目录的第 115～137 位，第一页完全没有。
- 本机兼容修复：管理器 normalize_synced_catalog 将 GPT 推理强度和速度变体后置，保留全部模型与默认路由；自定义模型现在位于第 36～58 位。未修改官方 Codex 安装包。
- 已修复本地目录和缓存；已构建并替换 /Applications/Codex Manager.app 内的管理器程序，重新签名验证通过，重启管理器成功。此本机构建采用 dev profile + tauri/custom-protocol，包含打包前端，未发布版本。
- 备份：/Users/ouwei/.codex/backups/picker-first-page-20260917-025816；管理器完整备份：/Users/ouwei/Library/Application Support/com.osir.codexmanager/backups/picker-pagination-20260917-025933。
- 验证：48 项 OpenCodex 测试通过，前端生产构建通过；实际 Codex app-server model/list(limit=100, cursor=null) 返回全部 23 个自定义模型；管理器重启后目录仍保持第 36～58 位；OpenCodex ready=true。
- 限制：这是对当前前端分页缺陷的排序兼容处理；超过 100 项后的部分参数变体仍不会出现在选择器第一页。以后非参数主模型超过 100 项时，仍需客户端完整分页修复。官方更新或重装管理器可能覆盖本地修复。
- 待用户验收：桌面操作工具明确禁止控制 Codex 本身，因此未绕过限制操作界面，也未重启承载当前任务的 Codex。请完全退出重开后，在选择器查看 Deepseek、Glm、Kimi、MiniMax、Mimo、Qwen。
