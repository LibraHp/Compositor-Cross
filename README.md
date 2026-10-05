# Compositor

中文 | [English](README.en.md)

跨平台图层合成与图像编辑器。用 Rust 和 [egui](https://github.com/emilk/egui) 写成，可在 Windows、macOS 和 Linux 上运行，并能打开、保存 macOS 版 [Compositor](https://github.com/robbietilton/Compositor) 的 `.comp` 工程。

当前版本 **0.1.0**。

## 下载

发布包在 GitHub Releases。每个版本包含：

- `compositor-windows-x64.zip`
- `compositor-macos-arm64.tar.gz`
- `compositor-linux-x64.tar.gz`

压缩包里有可执行文件、中英文说明和 MIT 许可证。Windows 版是 `compositor.exe`，双击即可运行。

发布新版本时，把 `Cargo.toml` 里的 `version` 改成同一个号，然后推送匹配的标签：

```bash
git tag v0.1.0
git push origin v0.1.0
```

推送 `v*` 标签后，[`.github/workflows/release.yml`](.github/workflows/release.yml) 会编译三个平台、跑测试，并创建 Release。标签必须和 `Cargo.toml` 的版本一致，例如 `v0.1.0` 对应 `version = "0.1.0"`。

## 从源码运行

需要 Rust 1.92 或更新的稳定版，以及支持 wgpu 的显卡。Windows 上一般不需要额外安装图形库。

```powershell
cargo run
```

发布构建：

```powershell
cargo run --release
```

可以把图像或 `.comp` 工程文件夹作为第一个参数：

```powershell
cargo run -- path\to\demo.comp
```

```powershell
cargo test
```

## 编辑

- 新建画布，打开 PNG、JPEG、BMP、GIF、TIFF、WebP，也可以把文件拖进窗口
- 读写 `.comp` 工程（`manifest.json` + `images/`），版本 1–11。还不计算的文字和调整会原样写回
- 图层、组、不透明度、混合模式、蒙版和剪贴
- 移动、缩放、旋转。拖动组时，组内图层一起移动
- 画笔、橡皮、油漆桶、渐变、形状、文字、仿制、修复、模糊和液化
- 矩形、椭圆、套索、多边形套索和魔棒选区，以及扩展、收缩、羽化和选择主体
- 色阶、曲线、色相/饱和度、曝光、渐变映射、模糊和杂色。可以做成调整图层
- 图层样式：投影、内外阴影、内外发光、描边和颜色叠加
- 裁剪、画布大小、图像大小、旋转和翻转
- 多个工程标签。每个标签自带关闭按钮
- 撤销和重做，导出 PNG、JPEG，复制合并图像，从剪贴板粘贴

左侧工具是图标。把指针放在按钮上可以看到名称和快捷键。

常用快捷键：`V` 移动，`B` 画笔，`E` 橡皮，`S` 仿制，`Ctrl+Z` 撤销，`Ctrl+S` 保存，`Ctrl+0` 适合窗口，`Ctrl+1` 实际像素。空格临时抓手。滚轮平移，Ctrl+滚轮或捏合缩放。

界面会加载系统中文字体，例如微软雅黑、黑体、苹方或 Noto Sans CJK。找不到时菜单改用英文。

## 和 macOS 原版的差别

主要编辑流程已经覆盖：图层、混合、蒙版、样式、选区、画笔、仿制、修复、液化、模糊、显影、PSD/PSB 和简单 SVG。工程在磁盘上被改写后会自动刷新。

这些部分用本地替代实现，不依赖 macOS 或 Adobe：

- HEIC 取出文件里最大的 JPEG 预览
- RAW 用 rawloader 读取，再做双线性去马赛克和相机白平衡
- 显影是本地色调面板，不是 Adobe Camera Raw
- 选择主体用边缘颜色模型和画面中心权重，不是神经网络
- 快捷键保存在本机。检查更新会查询上游 Compositor 的 GitHub 发布页

缩小看全图时，预览最长边约 2048 像素。放大后会按当前视野重绘更清晰的局部。导出和保存使用原始分辨率。

## 工程格式

`.comp` 是文件夹，不是压缩包。用「打开工程」选择这个文件夹。保存时先写到旁边的临时目录，写完再替换。

格式说明见上游文档：[project format](https://github.com/robbietilton/Compositor/blob/main/docs/project-format.md)。

## 许可

MIT。见 [LICENSE](LICENSE)。这是独立实现，不是把 Swift 源码逐行翻译过来。
