# auto-installer

一个面向 Linux 桌面的本地安装器，用来把常见第三方应用包整理到用户目录中，并按需生成桌面入口。

项目基于 Tauri 2、React、TypeScript 和 Bun 构建。

## 功能

- 选择本地安装包并自动识别类型。
- 支持 AppImage、`.tar.gz`、`.tgz`、`.tar.xz`、`.tar.bz2` 和 `.zip`。
- 预览可执行文件、图标候选和桌面入口信息。
- 将应用安装到用户目录，默认路径为 `~/.local/share/auto-installer/apps`。
- 可选择创建 `.desktop` 启动入口。
- 可选择把应用入口链接到 PATH 目录。

## 环境要求

- Linux
- Bun
- Rust stable
- Tauri Linux 构建依赖

在 Ubuntu 22.04 上可以安装：

```bash
sudo apt-get update
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  libfuse2 \
  patchelf
```

## 开发

安装依赖：

```bash
bun install
```

启动开发环境：

```bash
bun run tauri dev
```

构建前端：

```bash
bun run build
```

构建 Linux AppImage：

```bash
bun run tauri build --bundles appimage
```

构建产物位于：

```text
src-tauri/target/release/bundle/appimage/
```

## 发布

发布前请确保 `CHANGELOG.md` 中存在对应版本章节，例如：

```markdown
## [0.1.0] - 2026-06-20
```

## Changelog

变更记录见 [CHANGELOG.md](CHANGELOG.md)。
