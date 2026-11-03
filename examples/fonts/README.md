# 预览器内置字体

本目录为预览应用提供 Noto Sans CJK 的常规与粗体 TTC，默认使用其中的 `Noto Sans CJK SC`。字体不进 Git，运行 `python3 tools/fetch_fonts.py` 从上游 https://github.com/notofonts/noto-cjk 的 `Sans2.004` 标签下载并校验 SHA-256；文件与 Debian `fonts-noto-cjk` 逐字节一致，没有裁剪字库或更改字体名称。

许可和版权见 [LICENSE.txt](LICENSE.txt)，文件大小与 SHA-256 见 [manifest.json](manifest.json)。两份文件合计 39,535,544 字节（约 37.7 MiB）；首次预览从同源下载，后续打开复用页面缓存的字体字节。

字体是浏览器宿主的静态资产，不编入 WASM，也不让核心扫描系统字体。发布预览页面时必须一起发布本目录。默认替代策略只在预览器显式开启；缺失字形仍然报错，不把任意文稿宣称为无损预览。
