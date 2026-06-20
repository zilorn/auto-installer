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

仓库包含 GitHub Actions 工作流：`.github/workflows/release.yml`。

推送形如 `v0.1.0` 的 tag 后，工作流会：

1. 安装 Linux 构建依赖。
2. 安装 Bun 与 Rust stable。
3. 安装前端依赖。
4. 构建 Linux AppImage。
5. 从 `CHANGELOG.md` 中提取对应版本内容作为 GitHub Release 描述。
6. 上传 AppImage 到 GitHub Release。

发布示例：

```bash
git tag v0.1.0
git push origin v0.1.0
```

发布前请确保 `CHANGELOG.md` 中存在对应版本章节，例如：

```markdown
## [0.1.0] - 2026-06-20
```

## Changelog

变更记录见 [CHANGELOG.md](CHANGELOG.md)。
