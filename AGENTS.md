# AGENTS.md

本文件约束所有在本仓库中工作的 AI 编码代理。

## 工作约定

- 不要启动开发服务器，包括但不限于 `bun run dev`、`bun run tauri dev`、`npm run dev`、`pnpm dev`、`yarn dev` 和类似会常驻运行的命令。
- 功能开发完成后，必须把用户可见的重要变更写入 [CHANGELOG.md](CHANGELOG.md) 的 `Unreleased` 部分。
- 修改代码前先了解现有结构和风格，保持变更范围聚焦。
- 可以运行一次性检查、构建或测试命令，但避免留下后台进程。
