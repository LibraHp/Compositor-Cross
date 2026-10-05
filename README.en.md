# Compositor

[中文](README.md) | English

A cross-platform layer compositor and image editor. It is written in Rust with [egui](https://github.com/emilk/egui), runs on Windows, macOS, and Linux, and can open and save `.comp` projects from the macOS [Compositor](https://github.com/robbietilton/Compositor) app.

Current version: **0.1.0**.

## Download

Release packages are on GitHub Releases. Each version includes:

- `compositor-windows-x64.zip`
- `compositor-macos-arm64.tar.gz`
- `compositor-linux-x64.tar.gz`

Each archive contains the executable, both READMEs, and the MIT license. On Windows, run `compositor.exe`.

To publish a version, set `version` in `Cargo.toml` to the same number, then push a matching tag:

```bash
git tag v0.1.0
git push origin v0.1.0
```

Pushing a `v*` tag runs [`.github/workflows/release.yml`](.github/workflows/release.yml). It builds the three platforms, runs the tests, and creates a Release. The tag must match `Cargo.toml`. `v0.1.0` matches `version = "0.1.0"`.

## Build from source

You need Rust 1.92 or a newer stable release, and a GPU that can run wgpu. Windows usually needs no extra graphics libraries.

```powershell
cargo run
```

Release build:

```powershell
cargo run --release
```

Pass an image or a `.comp` project folder as the first argument:

```powershell
cargo run -- path\to\demo.comp
```

```powershell
cargo test
```

## Editing

- Create a canvas, open PNG, JPEG, BMP, GIF, TIFF, and WebP, or drop a file into the window
- Read and write `.comp` projects (`manifest.json` + `images/`), versions 1–11. Text and adjustments this build does not evaluate are written back unchanged
- Layers, groups, opacity, blend modes, masks, and clipping
- Move, scale, and rotate. Dragging a group moves the layers inside it
- Brush, eraser, bucket, gradient, shapes, type, clone, heal, blur, and liquify
- Rectangular, elliptical, lasso, polygonal lasso, and magic-wand selections, plus expand, contract, feather, and select subject
- Levels, curves, hue/saturation, exposure, gradient map, blur, and noise. These can be adjustment layers
- Layer styles: drop shadow, inner shadow, outer glow, inner glow, stroke, and color overlay
- Crop, canvas size, image size, and rotate or flip the canvas
- Several project tabs. Each tab has its own close button
- Undo and redo, export PNG and JPEG, copy the merged image, and paste from the clipboard

The tool rail uses icons. Hover a button to see its name and shortcut.

Common shortcuts: `V` move, `B` brush, `E` eraser, `S` clone, `Ctrl+Z` undo, `Ctrl+S` save, `Ctrl+0` fit, `Ctrl+1` actual pixels. Space is a temporary hand. Scroll pans. Ctrl+scroll or a pinch gesture zooms.

The interface loads a system CJK font, such as Microsoft YaHei, SimHei, PingFang, or Noto Sans CJK. If none is found, the menus fall back to English.

## Differences from the macOS app

The main editing flow is covered: layers, blending, masks, styles, selections, paint, clone, heal, liquify, blur, develop, PSD/PSB, and simple SVG. A project reloads when it is changed on disk.

These parts are local substitutes. They do not depend on macOS or Adobe:

- HEIC uses the largest embedded JPEG preview
- RAW uses rawloader, then bilinear demosaic and camera white balance
- Develop is a local tone panel, not Adobe Camera Raw
- Select Subject uses an edge color model and center bias, not a neural network
- Shortcuts are stored on this machine. Check for Updates queries the upstream Compositor GitHub releases

When the whole image fits on screen, the preview’s longest side is about 2048 pixels. Zooming in recomposites the visible area more sharply. Export and save use the original resolution.

## Project format

A `.comp` file is a folder, not a zip archive. Choose that folder with Open Project. Saving writes a temporary directory beside it, then replaces the project when the write finishes.

The upstream format notes are in the [project format](https://github.com/robbietilton/Compositor/blob/main/docs/project-format.md) document.

## License

MIT. See [LICENSE](LICENSE). This is an independent implementation, not a line-by-line translation of the Swift source.
