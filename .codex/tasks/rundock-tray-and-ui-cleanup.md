# RunDock 托盘关闭与底部 UI 精简实施任务

## 背景

当前 Windows 桌面壳已拦截原生关闭事件并调用 `hide()`，但忽略隐藏失败，也没有显式区分“隐藏到托盘”和“真正退出”。React 控制台同时永久显示左下服务器切换器和黑色状态栏，造成本机信息重复、功能入口含义不清。桌面 WebView 与浏览器共用 `127.0.0.1:2999` 页面，因此前端变更需要随 daemon 一并构建和部署。

## 目标

- 原生 X 只隐藏到托盘；托盘菜单退出才关闭桌面壳。
- 首次成功隐藏后显示一次 Windows 原生说明，后续不重复。
- 托盘退出保留 daemon 和全部托管项目。
- 删除正常界面的悬浮“本地”和黑色状态栏，把服务器、终端、AI、统计移入有文字的入口。
- 新增“设置 → 服务器”管理页，同时保留认证失败时的服务器恢复入口。
- 完成自动化验证、Windows 本机部署、Git 提交、普通 Push 和 PR；不得自动合并。

## 允许修改范围

- `desktop-shell/` 的窗口、托盘、状态与测试。
- `web-ui/src/` 的应用外壳、服务器管理、设置页、终端偏移、响应式样式及测试。
- `.github/workflows/quality.yml` 的 Windows 安装 smoke。
- 本任务文件及本次改动直接需要的锁文件。

## 禁止修改范围

- 不修改 `/api/v1`、2999 端口、daemon/项目数据 schema、远程服务器 localStorage 格式。
- 不停止、删除或重建任何托管项目，不修改项目外源码、数据库、`.env` 或凭据。
- 不引入 Tauri notification 插件，不恢复 5173，不改变登录/认证方式。
- 不 reset、stash、clean、强推、改写历史或自动合并 PR。

## 已确定实现要求

1. 桌面状态包含 `quitting` 与进程内一次性提示保护；托盘退出和内部 `--quit` 统一走显式退出函数。
2. 非退出态 `CloseRequested` 必须 `prevent_close`；成功隐藏后才写入 `%APPDATA%\alter-pm2\desktop-shell-tray-notice.json` 并异步显示一次原生说明。
3. 隐藏失败时窗口保持可见并显示原生错误；不得因错误退出桌面壳。
4. 托盘左键、菜单“打开 RunDock”和重复启动均恢复、取消最小化并聚焦同一窗口。
5. 托盘退出文案明确“项目继续运行”，且退出后 2999 与托管项目保持健康。
6. 正常认证界面不再渲染固定 `ServerSwitcher` 或 `StatusBar`；`AuthGuard` 的 recovery 入口保留。
7. 侧栏“工具”包含服务器连接、终端、AI 助手、系统统计、端口查找、隧道；开发工具仅在原开发条件下出现。
8. `/settings/servers` 使用现有服务器存储与校验逻辑，支持本机、HTTPS 远程直连与 SSH 配置，不改变持久化格式。
9. 终端面板保留现有能力，但底部定位改为 0；移动端不再为已删除状态栏预留空间。
10. 本机部署前记录 2999 监听者、daemon/shell 命令行、项目 ID/PID 和文件哈希；备份后只替换已验证的 RunDock 二进制并做可回滚重启。

## 验收标准

- WM_CLOSE 后原 `rundock.exe` PID 存活、主窗口不可见；重复启动恢复同一 PID。
- 首次隐藏显示说明，第二次隐藏不再显示；marker 为有效且持久的本地文件。
- 托盘退出结束桌面壳，但 daemon health、2999 listener 和托管项目 ID/PID 不变。
- 正常主界面没有固定“本地/local”和黑色底栏；服务器管理、终端、AI、统计均有明确文字入口。
- 认证失败或服务器配置损坏时仍能切回本机。
- 桌面与浏览器页面一致，移动端无底部空隙，生产环境无 5173 listener。
- Git diff 不含范围外修改、敏感信息、构建产物、调试代码或临时文件。

## 测试命令

```powershell
cargo fmt --all -- --check
cargo test --all-targets --locked
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo fmt --manifest-path desktop-shell/Cargo.toml -- --check
cargo check --manifest-path desktop-shell/Cargo.toml --locked
cargo test --manifest-path desktop-shell/Cargo.toml --locked
cargo build --manifest-path desktop-shell/Cargo.toml --release --locked
npm --prefix web-ui run format:check
npm --prefix web-ui run lint
npm --prefix web-ui run typecheck
npm --prefix web-ui test
npm --prefix web-ui run build
```

## 返回格式

- 改动摘要、关键行为与接口变化。
- 自动化测试、Windows smoke 和本机真实验收证据。
- 部署前后 2999、daemon、桌面壳和项目 PID/health 对比及回滚材料。
- Git 初始状态、默认分支、任务分支、提交/远端 SHA、Push、PR、CI 与合并状态。
