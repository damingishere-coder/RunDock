# Windows loopback proxy fix

## 背景

RunDock daemon 实际监听 `127.0.0.1:2999` 且健康接口可由浏览器和 PowerShell
正常访问，但 Rust 客户端的健康请求在 500 ms 后超时。现场 Windows 启用了本机
代理；客户端没有声明本机 daemon 通信必须绕过代理，导致桌面启动器等待 10 秒后
终止原本健康的 daemon。

## 目标

- 所有 `DaemonClient` HTTP 通信固定直连已验证的 loopback 地址。
- 保留现有严格健康契约、PID 文件和进程身份校验。
- 让 CLI、桌面壳和浏览器入口一致识别已启动 daemon。

## 允许修改范围

- `src/client/daemon_client.rs`
- 与 loopback 客户端代理行为直接相关的测试
- 本任务说明文件

## 禁止修改范围

- 不修改 `state.json`、`projects.json` 或任何用户项目状态。
- 不降低陌生进程保护，不按端口结束进程。
- 不修改 REST 数据结构、前端、桌面壳行为或安装器。
- 不修改 Windows 全局代理设置，不增加第三方依赖。

## 已确定实现要求

- 为普通、健康探测和流式三个 loopback `reqwest::Client` 显式调用 `no_proxy()`。
- 继续只允许 `DaemonClient` 连接 `localhost`、`127.0.0.1` 或 `::1`。
- 不保留临时诊断输出或调试环境变量。

## 验收标准

- `cargo fmt --check`、`cargo check --all-targets`、`cargo test --all-targets`、
  `cargo clippy --all-targets -- -D warnings` 通过。
- 开启 Windows 本机代理时，`alter daemon status` 仍能识别健康 daemon。
- daemon 停止后运行 `alter daemon start`，10 秒内成功；2999 健康。
- 启动 `rundock.exe` 后恢复桌面控制台，5173 没有监听。
- 退出桌面壳后 daemon 仍存活，项目保存状态不变。

## 测试命令

```powershell
cargo fmt --check
cargo check --all-targets
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```

## 返回格式

报告根因、修改文件、测试结果、现场 2999/5173 状态、Git 提交与推送结果。
