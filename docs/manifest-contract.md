# AWAI 镜像接口契约

Manager 从 `https://codexapp.awai.cc` 读取当前版本。契约的目标是让镜像可以更换
存储后端，同时让客户端在字段缺失、架构不可用或校验失败时安全停止。

## 当前端点

| 端点 | 用途 |
| --- | --- |
| `/latest/manifest` | 当前 Codex 版本、Windows 包身份和架构信息 |
| `/latest/checksums` | 当前发布工件的 SHA-256 清单 |
| `/latest/win` | Windows 当前架构包；服务端按请求架构路由 |
| `/latest/win-x64`、`/latest/win-arm64` | Windows 显式架构包 |
| `/latest/appcast.xml`、`/latest/appcast-x64.xml` | macOS Sparkle 更新源 |
| `/manager/latest.json` | Manager Tauri updater 清单 |

所有端点都必须使用 HTTPS。下载响应可以支持 Range，但客户端不能因为服务器返回
了重定向就跳过最终 URL 的 HTTPS 和校验。

## Windows manifest

`schemaVersion` 为整数，当前客户端接受 `2` 或更高版本。字段采用 camelCase；未知
字段必须忽略，已知字段缺失则返回明确错误，不得猜测包名。

```json
{
  "schemaVersion": 2,
  "codexVersion": "26.623.42026",
  "publishedAt": "2026-08-15T00:00:00Z",
  "sources": {
    "windows": {
      "version": "26.623.5546.0",
      "appVersion": "26.623.42026",
      "packageMoniker": "OpenAI.Codex_26.623.5546.0_x64__2p2nqsd0c76g0",
      "architecture": "x64",
      "contentLength": 123456789,
      "lastModified": "2026-08-15T00:00:00Z",
      "etag": "\"example\"",
      "updateManifest": {
        "storeProductId": "9PLM9XGG6VKS",
        "packageIdentity": "OpenAI.Codex_2p2nqsd0c76g0"
      },
      "architectures": {
        "x64": {
          "version": "26.623.5546.0",
          "appVersion": "26.623.42026",
          "packageMoniker": "OpenAI.Codex_26.623.5546.0_x64__2p2nqsd0c76g0",
          "architecture": "x64",
          "contentLength": 123456789,
          "downloadable": true
        },
        "arm64": {
          "version": "26.623.5546.0",
          "appVersion": "26.623.42026",
          "packageMoniker": "OpenAI.Codex_26.623.5546.0_arm64__2p2nqsd0c76g0",
          "architecture": "arm64",
          "contentLength": 120000000,
          "downloadable": true
        }
      }
    }
  }
}
```

`architectures` 存在时优先匹配当前 Windows 架构。`downloadable: false` 或缺少
ARM64 条目必须让客户端返回“当前架构不可用”，不能静默下载 x64 包。`packageMoniker`
用于精确匹配校验清单中的 `.msix` 文件名。

## Checksums

清单是 OpenSSL/GNU 常见的两列格式，每行是 64 位十六进制 SHA-256 和文件名；空行
与 `#` 注释允许存在：

```text
<64 lowercase hex>  OpenAI.Codex_26.623.5546.0_x64__2p2nqsd0c76g0.msix
<64 lowercase hex>  Codex-mac-arm64.dmg
```

客户端只接受完整的 64 位哈希。Windows 安装前必须找到与 manifest `packageMoniker`
完全一致的 `.msix` 条目；重复或缺失都属于失败。

## macOS appcast

镜像可以把 Sparkle enclosure URL 改为 AWAI URL，但必须原样保留 OpenAI 的
`sparkle:edSignature`、版本信息和 delta 字节。客户端下载后先做 Sparkle EdDSA
校验，再做 Apple Developer ID / Team ID 校验。无匹配 delta 时使用同一 appcast
条目的完整 zip；delta 失败不能破坏已安装应用。

## Manager updater

`/manager/latest.json` 使用 Tauri updater 格式。每个平台条目的 `url`、`signature`
和版本必须指向同一批最终字节：

```json
{
  "version": "0.5.2",
  "pub_date": "2026-08-15T00:00:00Z",
  "notes": "AWAI Codex App Manager update",
  "platforms": {
    "windows-x86_64": {
      "url": "https://codexapp.awai.cc/manager/0.5.2/CodexAppManager_x64-setup.nsis.zip",
      "signature": "<Tauri updater signature>"
    }
  }
}
```

客户端在安装前验证 updater 签名。`.sig` 只证明更新字节完整，不等同于 Windows
Authenticode 发行者身份。

## 兼容性规则

- 客户端只接受 HTTPS、受支持的架构和可解析的版本字段。
- 内容长度、ETag 或 Range 失败时可以重试，但不能跳过 SHA-256 或原生签名。
- 发布流程必须在 GitHub Release、AWAI 镜像和 `latest.json` 之间保持字节一致。
- 契约变化先增加兼容字段，再提高 `schemaVersion`；删除客户端仍需要的字段属于破坏性变更。
