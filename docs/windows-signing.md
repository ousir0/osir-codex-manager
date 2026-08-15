# Windows 安装器签名与核验

AWAI Codex App Manager 当前发布 Windows NSIS 安装器，并通过 `codexapp.awai.cc` 提供
镜像。是否存在 Authenticode 签名必须以对应版本的实际 PE 检查结果为准；当前预览构建
按“未签名”处理。

## 三种签名不要混淆

- **官方 Codex 签名**：验证下载的 Codex MSIX 是否由官方发布者签署，是安装前的主要
  身份锚点。
- **Tauri updater 签名**：`latest.json` 或 `.sig` 验证 Manager 更新包字节是否被替换。
  它不改变 Windows SmartScreen 的发行者判断。
- **Authenticode**：Windows 对 Manager 的 PE 和安装器显示发行者身份。没有可信证书时，
  SmartScreen 可能提示未知发布者。

## 用户下载核验

从 [AWAI 镜像](https://codexapp.awai.cc) 下载后，从同一版本取得 `SHA256SUMS`，在
PowerShell 中运行：

```powershell
Get-FileHash .\CodexAppManager_x64-setup.exe -Algorithm SHA256
# ARM64 安装器替换为 CodexAppManager_arm64-setup.exe
```

将结果与清单中同名文件的 64 位 SHA-256 比较。不一致就删除文件并重新下载；不要关闭
SmartScreen 来“修复”哈希不一致。

如果版本提供 Authenticode，可以在 PowerShell 查看：

```powershell
Get-AuthenticodeSignature .\CodexAppManager_x64-setup.exe | Format-List Status,SignerCertificate
```

`Status = Valid` 只说明该文件的证书链和签名有效，仍需核对发行者、时间戳、版本和
`SHA256SUMS`。没有签名时，检查结果为 `NotSigned` 是已公开披露的状态。

## 发布流水线

Windows release job 的顺序必须是：

1. 构建 x64/ARM64 工件并记录 PE 架构。
2. 如果受保护环境提供证书，给最终 EXE、卸载器和安装器做 Authenticode 签名。
3. 验证签名状态和期望发行者；要求签名的版本在此处失败即停止。
4. 对最终字节生成 Tauri updater `.sig`，再生成 `latest.json`。
5. 运行安装、启动、升级、卸载冒烟测试，上传 GitHub Release 和 AWAI 镜像。
6. 从公开 URL 回读并重新计算哈希，确认发布内容没有发生二次变更。

证书、密码和时间戳服务只能从 GitHub protected environment 读取。不要把 PFX、私钥或
密码放在仓库、Issue、构建日志或皮肤包中。SignPath Foundation 接入需要单独审查，旧的
可选 PFX 脚本不代表已经完成 SignPath 集成。

## SmartScreen 说明

SmartScreen 会综合证书身份、下载量、历史信誉和文件风险。即使 Authenticode 有效，新版
也可能暂时显示警告；未签名安装器更容易出现警告。项目只能通过可信签名、公开哈希、可
追溯 release 和透明文档降低风险，不能承诺绕过 Microsoft 的判断。

## English summary

Treat preview Windows installers as unsigned until the release itself proves otherwise. Verify
the matching `SHA256SUMS`, then inspect Authenticode with `Get-AuthenticodeSignature` when a
signature is advertised. Tauri updater signatures protect update bytes only; they are not
Windows publisher identity. See the [code signing policy](./code-signing-policy.md).
