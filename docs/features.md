# Codex Manager 功能说明

本文按用户能感知的功能说明边界、入口和结果。

## 安装与更新 Codex

Manager 会先识别系统、CPU 架构、当前安装路径、版本和来源，再生成安装或更新计划。用户确认后才执行下载和替换。下载阶段会校验清单、哈希、包身份和平台签名；失败时保留当前可运行版本。

## Manager 自更新

Manager 自身使用独立的 Tauri updater 通道。启动或定时检查发现新版本时，会在全局显示更新提示。用户可以稍后提醒、跳过当前版本或确认安装。更新前会重新检查目标版本，安装后重启并清除已完成的待更新状态。

详细说明：[Manager 自更新闭环](manager-update-closure.md)。

## Codex 配置管理

配置页负责 Base URL、模型、MCP、普通 API Key、图片 API Key、权限和恢复。保存前会检查格式，写入采用原子替换并保留上一版备份。页面不会回显已保存的 API Key。

## OpenCodex 多模型

OpenCodex 是可选的本地多模型组件。Manager 会识别已有环境；没有 Node/npm 时准备私有运行时；安装后等待服务 ready；授权后同步账户、订阅、路由和模型，并验证默认路由。

详细说明：[OpenCodex 安装闭环](opencodex-install-closure.md)。

## 主题

主题支持在线浏览、本地导入、预览、试穿、应用和恢复。主题只改变 Codex 的外观注入和配置，不修改 Codex 应用文件或签名内容。

## API 服务入口

OSIR API 是独立服务，负责 API Key、模型市场、控制台、价格、额度和 API 文档。Manager 只负责把 API 配置安全保存到本机并提供连接入口。

API 入口：[api.osirclaw.com](https://api.osirclaw.com/)。

## 语言

Manager、官网和 GitHub 文档分别支持中文和 English。

详细说明：[语言切换说明](language-switching.md)。

## 发布与安全

正式发布包含 Mac 和 Windows 四个平台资产、GitHub Release、updater 签名、版本清单、SHA-256 和公开回读校验。网站对象镜像必须与 GitHub Release 指向同一批最终字节。

## 隐私边界

Manager 不上传用户对话、工作区、API Key 或本地配置，不运行项目自营遥测。诊断信息需要用户主动复制或提供。
