# Codex Manager — 官网

中英双语单页官网(滚动叙事 + GSAP ScrollTrigger 管线可视化)。纯静态产物,可直接部署到
Cloudflare Pages / GitHub Pages(`base: "./"`,任意子路径均可)。

## 开发

```bash
npm install
npm run dev        # Vite dev server
npm run build      # 产出 dist/
```

## 资产管线

生成素材(git-ignored 的 `assets/raw/`、`assets/fonts-src/`)→ 优化产物(`public/`):

```bash
npm run fonts      # Source Han Serif SC + Fraunces 子集化 → public/fonts/*.woff2
npm run images     # assets/raw/*.png → public/img/*.{avif,webp} 多分辨率 + og.jpg
```

- 装饰图像由项目维护者生成或取得授权;透明素材经
  `transparent generate` / `transparent extract --method dual` 提取并通过 `--strict` 验收。
- 修改 `src/locales/*.ts` 或 `index.html` 文案后需重跑 `npm run fonts`
  (子集按实际用字收集,缺字会回退到系统字体)。
- 真实 logo 来自两个仓库的 `assets/logo.png`,缩放为 192px 后自托管。

## 信息架构

首页以用户任务为主线：产品能力、下载、用户手册、OSIR API 和 GitHub。

- Manager 官网：安装、更新、配置、多模型和主题；
- OSIR API 官网：API Key、控制台、模型市场、价格和 API 文档；
- GitHub：源码、精确版本资产和问题反馈。

官网首页的正式下载入口指向 `app.osirclaw.com/manager/0.5.8` 版本化镜像；Manager 更新清单固定使用 `/manager/latest.json`，由发布目录原子切换推进。

## i18n

- 默认 `zh-CN`(HTML 静态文案即中文,SEO 友好);`en` 在运行时通过 `data-i18n` 替换。
- 首次访问按 `navigator.language` 选择,手动切换持久化于 `localStorage("cam-site-lang")`,
  并同步 `<html lang>` 与 title/description/og:* 元信息。
- 文案唯一来源:`src/locales/zh.ts` / `src/locales/en.ts`。

## 动效

- GSAP + ScrollTrigger + MotionPath;桌面端管线段(`#pipeline-scroll`)与管理器三步
  (`#manager-stage`)为 pin + scrub 叙事。
- `prefers-reduced-motion: reduce` 或窄屏(< 1024px)时,管线自动切换为纵向时间线
  (`.pipeline-rail`),所有状态静态落定。
