# SlideShuttle 标识

分层的橙红梭形在后，深色字母砖在前。砖上的 S 是梭子来回走出的纬线：三道横线、两次折返，外形恰为 16:9，两端收成梭尖。

| 文件 | 用途 |
|---|---|
| `slideshuttle-symbol.svg` | 彩色图形标，默认使用 |
| `slideshuttle-horizontal.svg` | 横排组合，浅色背景 |
| `slideshuttle-horizontal-dark.svg` | 横排组合，深色背景 |
| `slideshuttle-symbol-black.svg` / `-white.svg` | 单色版，S 与砖缝为真实镂空 |
| `slideshuttle-icon.svg` | 小尺寸版，只保留字母砖；32px 及以下、favicon 使用 |

## 颜色

| 位置 | HEX |
|---|---|
| 梭形上层 | `#FF8A5C` |
| 梭形中层 | `#EE5A2E` |
| 梭形下层 | `#C6401F` |
| 字母砖 | `#9C2C10` |
| 字标（浅色背景） | `#1F1A17` |

## 使用

- 四周留白不小于字母砖边长的四分之一。
- 彩色图形标最小 48px；更小时改用 `slideshuttle-icon.svg`。
- 不拉伸、不旋转、不改色阶顺序、不加阴影或描边；彩色版放在照片或杂色背景上时改用单色版。
- 字标为 Lato Heavy（SIL OFL 1.1）转出的轮廓，文件不依赖本机字体。

SlideShuttle 是独立项目，与 Microsoft 无关联；PowerPoint 是 Microsoft 的商标。
