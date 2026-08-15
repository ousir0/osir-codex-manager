# AWAI Codex App Manager 路线图

路线图按可验证的用户结果排列，不把实验性想法当成已发布能力。状态：✅ 已完成，
🟡 进行中，⬜ 计划中。

| 方向 | 状态 | 当前说明 |
| --- | --- | --- |
| Windows x64/ARM64 安装、更新、卸载 | 🟡 | MSIX 优先，便携路径和真实 Windows 机器验证持续完善 |
| macOS Sparkle delta 更新 | ✅ | appcast、EdDSA、完整包回退、替换和回滚引擎已实现 |
| Manager 自更新 | 🟡 | Tauri updater 客户端和 AWAI `/manager/latest.json` 已接入，正式签名仍需配置 |
| AWAI 镜像 | ✅ | `codexapp.awai.cc` 提供 manifest、checksums、安装包和 Manager 更新入口 |
| CODEX 配置管理 | ✅ | API Key、AWAI Base URL、模型获取、Goal Mode 和安全字段可读写 |
| AWAI 皮肤库 | 🟡 | `.codexskin` 导入、预览和应用已接入，在线目录与素材审核继续完善 |
| 浏览器预览 | ✅ | Vite 开发模式可先验证界面和 mock 状态，再进入 Tauri 测试 |
| 代码签名 | ⬜ | macOS 公证按发布环境执行；Windows Authenticode/SignPath 尚未作为默认闸门 |

## 下一阶段

1. 在 Windows 真机完成 x64 和 ARM64 的 MSIX/便携安装、运行中更新和回滚验证。
2. 把历史版本选择器迁移到 AWAI 自有版本 API，取消对旧上游 GitHub Releases API 的依赖。
3. 完成 Manager 正式版本的 Tauri updater 签名、镜像回读和 Windows 安装器发布检查。
4. 为皮肤仓库增加 CI：格式校验、预览图尺寸检查、许可证字段和 SHA-256 清单。
5. 在不收集用户内容的前提下补充可导出的脱敏诊断报告和升级失败恢复指引。

## 暂不做

- 不在皮肤包中塞入品牌推广、API 地址或 API Key。
- 不上传对话、工作区、配置或凭据，不增加项目自营遥测。
- 不修改 Codex 安装文件，不绕过 Windows、macOS、OpenAI 或 Microsoft 的签名/授权策略。
- 不把未签名预览包标成稳定发行版。

每个路线图条目都必须对应代码、测试或公开验证记录；只有用户可复现的结果才会标为完成。
