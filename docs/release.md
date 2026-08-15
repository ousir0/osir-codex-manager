# AWAI 发布指南

本文是本仓库维护者发布 Manager 的操作约定。发布前必须先阅读
[`code-signing-policy.md`](./code-signing-policy.md) 和
[`windows-signing.md`](./windows-signing.md)。

## 版本与说明

1. 在默认分支更新应用版本、变更摘要和必要的 manifest 契约。
2. 在 `docs/releases/vX.Y.Z.md` 写用户能验证的变化；文件缺失时工作流使用
   [`releases/FALLBACK.md`](./releases/FALLBACK.md)。
3. 在 CI 通过后创建 `vX.Y.Z` tag。版本号必须同时匹配 Tauri 配置、Cargo 和前端元数据。
4. 发布说明只写已验证的功能、已知限制、安装链接和校验方式，不提前承诺签名或镜像状态。

## 本地检查

```bash
npm ci
npm run check
npm test
cargo test --manifest-path src-tauri/Cargo.toml --lib
git diff --check
```

在目标平台可用时，再执行对应的 Tauri 构建和打包冒烟。不要把 `dist/`、私钥、证书、
API Key、镜像凭据或真实用户配置提交进仓库。

## GitHub Actions 流程

发布工作流完成以下阶段：

1. 从受保护的默认分支和 tag 构建 Windows x64/ARM64、macOS arm64/x64。
2. 运行 Rust/TypeScript 测试、平台架构检查和安装生命周期冒烟。
3. macOS 在打包后按“嵌套二进制 → App → 公证 → 重打包”的顺序处理签名；Windows
   只有受保护环境提供证书时才进行 Authenticode，未配置时明确保持未签名状态。
4. 对最终字节生成 Tauri updater 签名和 `latest.json`，不在签名后修改安装包。
5. 先上传 GitHub Release，再把同一批工件同步到 `codexapp.awai.cc` 的版本化路径和
   `latest` 指针。
6. 从 GitHub、AWAI 公开 URL 回读，重新验证长度、SHA-256、updater 签名和 manifest，
   任一回读失败都阻止指针更新。

## 受保护变量

证书、Tauri updater 私钥、Apple notarization key、R2/S3 写入凭据和 GitHub API token
只能放在 GitHub protected environment。变量命名以 `.github/workflows/release.yml`
为准；新仓库不要复制旧项目的 secrets。任何日志都不得打印 secret 值或完整预签名 URL。

## 镜像目录

当前服务器使用：

```text
https://codexapp.awai.cc/latest/manifest
https://codexapp.awai.cc/latest/checksums
https://codexapp.awai.cc/manager/latest.json
https://codexapp.awai.cc/manager/<version>/...
```

版本化对象不可覆盖；`latest` 只在候选工件完成公开回读后推进。发布脚本或手工同步时
必须保留官方包字节、原始签名和同版本 SHA-256。

## Windows 发布决策

当前 Windows 构建按未 Authenticode 签名发布。发布说明必须直接披露 SmartScreen 可能
出现提示，并提供 `SHA256SUMS` 核验命令。只有真实证书、时间戳和最终 PE 验证全部通过，
才可以把版本描述为 Authenticode-signed；SignPath 申请状态不能代替签名事实。

## 回滚与撤回

发现错误版本时，先停止 `latest` 指针，再保留 GitHub Release 和版本化对象供审计。修复
版本必须使用新版本号；只有经过维护者审查的事件处理流程才允许把 `latest` 指回旧的、已
验证的版本。不得用覆盖同一路径的方式“修复”已发布文件。
