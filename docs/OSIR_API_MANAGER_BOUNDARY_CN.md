# OSIRAPI 与 OSIR Codex Manager 接入边界

来源会话：`01a0109f-8289-7013-af31-3371764c84e4`  
整理日期：2026-08-17

## 结论

Manager 是运行在用户电脑上的客户端；OSIRAPI 是用户、模型、额度和审计控制面。客户端不应持有上游供应商密钥、数据库凭据、发布私钥或对象存储密钥。

## 客户端消费合同

- Provider ID：`osir`。
- Provider Name：`OSIR`。
- API Base URL：暂定 `https://api.osirclaw.com/v1`。
- 模型目录：`GET /v1/models`。
- Responses：`POST /v1/responses`。
- Chat Completions：`POST /v1/chat/completions`。
- 图片：`POST /v1/images/generations`，具体异步能力以后端已发布合同为准。

## Key 与日志约束

- API Key 只保存在用户本机 Codex 配置中。
- Key、一次性票据不得进入 URL、日志、诊断包、更新清单和安装包。
- Manager 专用 Key 应能单独撤销和轮换，不影响用户的其他 Key。
- 鉴权失败、额度不足、模型不可用和上游故障需要返回用户可理解的错误。

## 推荐正式接入流程

1. 用户登录 OSIRAPI。
2. 控制面签发短时、一次性的 Manager 接入票据。
3. Manager 消费票据，换取专用 API Key 和公开配置。
4. 控制面记录用途、设备标签、签发时间、最后使用时间和撤销状态。
5. 用户可在控制台撤销、轮换或重新连接。

首版如果尚未完成票据接口，可以保留手动粘贴 Key 作为开发联调路径，但不能把主 Key 写进安装包或默认配置。

## 不属于 API 合同的能力

- Tauri updater 私钥、公钥和 `latest.json`。
- Windows/macOS 代码签名。
- Codex 安装包镜像与校验清单。
- 在线皮肤目录。
- 国际化隧道。

这些能力可以由同一团队运营，但应使用独立密钥、部署、监控和回滚流程。
