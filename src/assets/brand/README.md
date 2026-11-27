# 品牌素材（src/assets/brand）

应用界面里用到的品牌图形都放在这里，按「品牌-用途」命名，全部由源码直接 import，
不放到 `public/`，这样 Vite 会做哈希和体积校验。

## PonteX

| 文件 | 尺寸 | 用途 |
| --- | --- | --- |
| `pontex-mark.svg` | 183×149 | 纯图形，标题栏/图标位；深色下由 `ui.css` 的 `.brand img` 提亮 |
| `pontex-wordmark.svg` | 249×57 | 「PonteX」黑色字标，浅色背景用 |
| `pontex-one.svg` | 130×59 | 「One」紫色字标（#7459D9） |
| `pontex-tagline.svg` | 221×52 | 「经纬即连」中文标语 |
| `pontex-lockup.svg` | 670×268 | 图形 + PonteX One + 经纬即连的完整组合，用于关于页、安装器首屏 |

图形主色 `#5D2AA6` / `#3C1867` / `#391765`，字标为黑色；除 `pontex-one.svg` 外都不带底色。
`pontex-lockup.svg` 自带中文标语，英文界面不要用它，改用 `pontex-mark.svg` + 文字。

## WorkBuddy（腾讯云 CodeBuddy 出品）

| 文件 | 尺寸 | 用途 |
| --- | --- | --- |
| `workbuddy-icon.svg` | 280×280 | 应用图标（绿色圆角方块 + 白色图形），Agent 行、卡片等小图标位 |
| `workbuddy-logo-light.png` | 1620×560 | 官方横版标志，**浅色背景**用（黑色 WORK BUDD> 字标） |
| `workbuddy-logo-dark.png` | 1620×560 | 官方横版标志，**深色背景**用（白色 WORK BUDD> 字标） |

只有字标墨色不同，绿色图标在两种主题下完全一致，所以**不要**再给图标加 `filter: invert()`。

浅色/深色按系统主题切换的推荐写法：

```tsx
import light from "@/assets/brand/workbuddy-logo-light.png";
import dark from "@/assets/brand/workbuddy-logo-dark.png";

<picture>
  <source srcSet={dark} media="(prefers-color-scheme: dark)" />
  <img src={light} alt="WorkBuddy" height={20} />
</picture>
```

来源（2026-11-27 取回，WorkBuddy 网页版正式构建）：

- `workbuddy-logo-light.png` ← `https://download.codebuddy.ai/web/agents/cc1b179dd1e37087b32a3fee8acc442e4740fea4/assets/workbuddy-logo-DdDcVi2S.webp`
- `workbuddy-logo-dark.png` ← `https://download.codebuddy.ai/web/agents/cc1b179dd1e37087b32a3fee8acc442e4740fea4/assets/workbuddy-logo-dark-D6WuruMB.webp`
- `workbuddy-icon.svg` ← `https://download.codebuddy.ai/web/agents/cc1b179dd1e37087b32a3fee8acc442e4740fea4/assets/workbuddy.svg`

CDN 路径带构建哈希，换构建后可能失效；这两张 PNG 已按原像素转存（原图是带透明通道的 WebP），日常直接用本目录的文件。

## 使用注意

- 品牌图形只用于标识对应产品，不做二次创作；WorkBuddy 商标归腾讯云 CodeBuddy 所有。
- 新增素材请沿用「品牌-用途」命名，并在上表补一行，避免下次找不到出处。
