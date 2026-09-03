# RunDock Windows 桌面壳与可靠启动实施任务

## 背景

RunDock 当前由 `alter.exe` 同时提供 CLI、后台 daemon、`127.0.0.1:2999` API 与内嵌 React 控制台。正式环境已移除 5173，但 Windows 安装缺少有效桌面入口和登录自启动；`alter web` 只打开网址，不会确保 daemon 已就绪。

## 目标

- 新增 Windows-only `rundock.exe` Tauri 2 桌面壳，复用 2999 页面。
- 收敛 CLI 与桌面壳共用的 daemon 探测、启动、等待和失败诊断逻辑。
- 提供单实例、托盘、关闭到托盘、默认登录自启动和安全外链处理。
- 将桌面壳纳入 Inno 安装器、Windows CI、签名发布和真实安装 smoke。
- 保持现有 CLI、API、端口、数据目录和项目状态兼容。

## 允许修改范围

- `src/client/`、`src/daemon/`、`src/cli/commands/daemon.rs`、`src/lib.rs` 及相关测试。
- 新增 `desktop-shell/` Windows 桌面壳。
- `installer/alter-setup.iss`、`scripts/release.ps1`。
- `.github/workflows/quality.yml`、`.github/workflows/release.yml`。
- `README.md`、`docs/ARCHITECTURE.md`、`docs/CLI.md` 与本任务文档。
- `.gitignore`、锁文件及桌面构建所需的非敏感配置。

## 禁止修改范围

- 不改 `/api/v1` 现有响应结构、默认端口 2999、`%APPDATA%\alter-pm2` 目录或现有状态 schema。
- 不恢复生产 5173，不引入 Electron，不把 daemon 改成 Windows 服务。
- 不自动清理 `state.json`、`projects.json`、孤立元数据、日志或用户备份。
- 不读取或写入 API Key、Token、密码、Cookie、`.env` 内容。
- 不批量启动、停止或删除用户已登记项目；不改写 Git 历史。

## 已确定实现要求

1. `DaemonClient` 暴露严格探测结果：离线、健康、端口被占用/健康契约不兼容；只有离线状态允许拉起 daemon。
2. daemon 启动函数显式接收 `alter.exe` 路径；CLI 传当前可执行文件，桌面壳传同目录 sibling，禁止把 `rundock.exe` 作为 daemon 启动。
3. `alter web` 先确保 daemon 健康，再打开浏览器；失败返回非零和可操作诊断，不结束未知 PID。
4. Tauri 页面先显示本地启动/错误页，健康后导航到 `http://127.0.0.1:2999/`；localhost 页面无 Tauri IPC、shell、filesystem 或 process 权限。
5. WebView 只内嵌 2999；外部 HTTP(S) 和自定义协议交给 Windows。重复启动聚焦现有窗口。
6. 无参数显示窗口，`--background` 静默进入托盘；关闭窗口隐藏；托盘退出仅退出桌面壳。
7. 登录自启动默认开启且可在托盘切换，显式保存“已初始化”状态，用户关闭后不得被下次启动重新开启。
8. 保留原 Inno AppId；旧安装原地升级，新装默认 `C:\Program Files\RunDock`；桌面入口指向 `rundock.exe`，兼容 CLI 仍为 `alter.exe`。
9. 安装器按需部署经 Microsoft Authenticode 验证的 Evergreen WebView2 bootstrapper；不引入第二套 Tauri 安装器/更新器。
10. 自动更新保持关闭，直到双可执行文件升级与 daemon 交接 smoke 通过；卸载不删除用户数据，也不自动批量停止运行项目。

## 验收标准

- daemon 停止时启动桌面壳，10 秒内健康并打开控制台；2999 被未知程序占用时仅显示诊断。
- 同一时间最多一个桌面壳和一个可验证 daemon；重复启动聚焦现有窗口。
- 关闭窗口可从托盘恢复；托盘退出后 daemon 与基线项目继续运行。
- 登录自启动使用 `--background`，关闭开关后保持关闭。
- 外部项目网页、GitHub、`wanmotai://` 等不在受信 WebView 内导航。
- 新装、旧版原地升级、卸载均保留 `%APPDATA%\alter-pm2`，且不产生第二套安装记录。
- Linux 现有 Rust/打包流程不依赖 Tauri；Windows CI 构建、签名并安装 smoke 两个可执行文件。

## 测试命令

```powershell
cargo fmt --all -- --check
cargo check --all-targets --locked
cargo test --all-targets --locked
cargo clippy --all-targets --all-features --locked -- -D warnings
npm --prefix web-ui run format:check
npm --prefix web-ui run lint
npm --prefix web-ui run typecheck
npm --prefix web-ui test
npm --prefix web-ui run build
cargo fmt --manifest-path desktop-shell/Cargo.toml -- --check
cargo check --manifest-path desktop-shell/Cargo.toml --locked
cargo test --manifest-path desktop-shell/Cargo.toml --locked
cargo build --manifest-path desktop-shell/Cargo.toml --release --locked
```

## 返回格式

- 改动摘要与关键接口。
- 测试命令、结果和失败证据。
- 安装/升级/托盘/浏览器实测结果与未覆盖边界。
- Git 状态、提交哈希、分支、远端地址和推送结果。
