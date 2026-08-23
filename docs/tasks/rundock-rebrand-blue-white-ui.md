# RunDock 品牌迁移与蓝白 UI A 执行任务

## 背景

当前项目基于 `alter` Rust/React 进程管理器，工作区包含尚未提交的项目化管理、可靠启停、Web 入口和中文化功能。用户已确认：

- 产品名称：`RunDock`
- UI 方向：方案 A，蓝白 Apple 风侧边栏工作台
- 图标方向：方案 A，蓝色运行/连接流线图标
- GitHub 目标：`https://github.com/damingishere-coder/RunDock.git`

## 目标

1. 将对外可见品牌、网页、文档、安装器和发布链接迁移为 RunDock。
2. 将主控制台实现为蓝白、明亮、项目优先的 UI A，并保留现有全部业务能力。
3. 集成可缩放的 RunDock 图标，提供 Web favicon、UI Logo、PNG 和 Windows ICO。
4. 保留已有 CLI、数据和安装升级兼容性。
5. 完成必要验证后，将当前完整项目成果提交并普通推送到指定 GitHub 仓库。

## 允许修改范围

- `web-ui/` 的主题、根布局、登录页、项目页、品牌文案、favicon 与相关测试。
- `assets/`、`installer/`、`scripts/`、`.github/workflows/` 的品牌和图标资源/发布元数据。
- `README.md`、`docs/`、`release/README.md`、`packaging/` 的对外品牌与仓库链接。
- `src/api/routes/update.rs` 等直接与发布仓库、产品显示名称相关的源码。
- `.gitignore` 与本任务文件。
- 为完成用户“同步当前项目成果”目标而纳入的现有工作区改动，但必须先验证并检查 secrets。

## 禁止修改范围

- 不更换 Codex 模型、登录方式、认证配置或外部 Coding CLI。
- 不读取或提交 `.env`、API Key、Token、密码、Cookie、浏览器数据、运行时数据库和日志。
- 不 reset、stash、覆盖或清理用户已有工作区改动。
- 不改变 `alter` Rust crate/lib/bin 名称、`alter.exe` 命令兼容性、`ALTER_*` 环境变量、`%APPDATA%\alter-pm2` / `~/.alter-pm2` 数据目录、`alter-daemon` 服务名、既有 metrics/localStorage key。
- 不改变 Inno Setup `AppId` GUID。
- 不删除或改写 MIT 上游版权归属。
- 不强推、不重写历史、不删除远端内容、不改变 GitHub 仓库可见性。

## 已确定实现要求

- 全局主题使用雪白、雾蓝、钴蓝和深海军蓝文字；圆角、轻阴影、克制玻璃感。
- 桌面为窄侧栏 + 主内容；窄屏折叠为可操作的导航，不产生横向溢出。
- 项目优先，技术进程默认收起；日志、端口和组件渐进展开。
- 保留并美化 AI、通知、终端、对话框和底部状态栏；保证层级与键盘可达。
- Logo 使用选定图标 A 的矢量化生产版本；不得带棋盘格背景。
- 对外 GitHub URL 指向 `damingishere-coder/RunDock`；第三方 GitHub/Copilot 协议 URL 不替换。
- 安装器显示名改为 RunDock，但继续安装/调用 `alter.exe`，保留 AppId。
- 目标 GitHub 仓库当前为空且公开；保持现状，仅普通推送。

## 验收标准

- 主要页面在 1440、1024、768、390px 下可用，无关键控件遮挡或横向溢出。
- UI 可见品牌为 RunDock，favicon、侧栏、登录页、安装器均使用一致图标。
- 项目启停、打开网页、日志、端口、AI、通知、终端等现有交互不回退。
- 前端构建、测试、lint/format 检查完成；Rust fmt/check/test/build 完成或准确说明基线失败。
- 发布资产命名、更新检查地址和工作流相互匹配。
- 无 secrets、`.env`、日志、缓存、构建产物或生成器临时文件进入提交。
- Git diff 范围、TODO/debug、硬编码、依赖变化和 GitHub 远端均由主代理复核。
- GitHub 普通推送成功，远端默认 `main` 可见提交。

## 测试命令

```powershell
git diff --check
git status --short --branch

Set-Location web-ui
npm run build
npm test
npm run lint
npm run format:check

Set-Location ..
cargo fmt -- --check
cargo check
cargo test
cargo build --release
```

品牌与安全检查：

```powershell
rg -n "thechandanbhagat|outernet-io/alter|alter-pm/releases|vite.svg|react.svg" README.md docs .github installer packaging scripts src web-ui
git diff --cached --check
git diff --cached --stat
```

## 返回格式

- 修改摘要与关键文件。
- UI/交互验收结果。
- 图标最终路径和 ImageGen/矢量化说明。
- 测试命令、通过/失败结果及基线差异。
- Git 状态、提交哈希、分支、目标仓库地址和推送结果。
