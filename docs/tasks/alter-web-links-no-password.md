# Alter 网页直达与无密码访问执行任务

## 背景

Alter 已能按受管进程 PID 及子进程关系检测监听端口，但进程列表只把端口显示成不可点击标签。认证实现也与文档不一致：即使没有配置密码，前后端仍会要求登录 Token。当前工作区已有前端中文化和进程完整配置持久化修改，必须原样保留。

## 目标

1. 为运行中的本机项目提供“打开网页”入口；单端口直接打开，多端口提供选择菜单。
2. 让未配置密码真正代表无密码模式，并关闭当前安装实例的网页密码。
3. 保持守护进程仅监听 `127.0.0.1:2999`，不开放局域网无密码访问。

## 允许修改范围

- 与进程端口展示、认证守卫和安全设置直接相关的 `web-ui/src/**`。
- `src/api/**`、`src/config/auth_config.rs`、CLI 参数/命令/客户端分发。
- 与上述行为直接相关的测试和 `docs/API.md`。
- 本任务文件、前端构建产物和 Rust 构建产物。
- 验证完成后备份并替换 `C:\Program Files\alter\alter.exe`，通过正式 CLI 关闭认证。

## 禁止修改范围

- 不覆盖或回滚现有中文化、`src/daemon/state.rs`、`src/process/manager.rs` 及其他用户修改。
- 不读取或输出密码、Token、Cookie、`.env`、浏览器数据或其他 secrets。
- 不绑定 `0.0.0.0`，不开放局域网/公网无密码访问。
- 不提交、push、stash、reset、重写历史、发布或部署到远程环境。
- Luna 子代理不得修改生产源码、安装二进制或操作项目外路径。

## 已确定实现要求

1. 网页候选仅使用受管进程/子进程的监听 TCP 端口；项目运行期间定时刷新。
2. 一个端口时“网页”按钮直接打开 HTTP 地址；多个端口时弹出菜单。远程 SSH 模式不生成错误的本机链接。
3. `password_hash = None` 是无密码模式的唯一判断；此时认证中间件放行，前端直接进入控制台。
4. 新增经现有会话或 CLI 主 Token 授权的密码删除接口；关闭时清除密码、PIN、通行密钥、自动锁定和网页会话，但保留 CLI 主 Token。
5. 新增 `alter auth disable`，命令不得打印或要求用户密码。
6. 安全设置页支持关闭和重新设置密码；无密码时不显示锁屏入口，也不自动锁定。
7. 使用当前工作树构建，安装前保存状态并备份现有二进制；重启后关闭认证并验证。

## 验收标准

- 无密码、无 Authorization 的受保护 API 返回 200；配置密码时仍返回 401。
- 删除密码后相关认证配置和网页会话被清理，CLI 主 Token 保留可用。
- 全新浏览器会话直接进入控制台，不出现设置密码或登录页。
- 单端口和多端口项目分别可直接打开或选择网页端口；无监听端口不显示网页按钮。
- GroupBrief 可从进程列表选择 `5173` 打开。
- 现有工作区修改和受管进程保持完整，守护进程仍只监听 `127.0.0.1:2999`。

## 测试命令

```powershell
cd web-ui
npm run format:check
npm run lint
npm test
npm run build

cd ..
cargo test
cargo build --release
git diff --check
git status --short
git diff --stat
```

安装后验证守护进程监听地址、`GET /api/v1/auth/status`、无 Token 的健康接口、进程列表网页入口和 GroupBrief 页面。

## 返回格式

- `status`
- `changedFiles`
- `implementationSummary`
- `testSummary`
- `installationSummary`
- `risks`
- `reviewTargets`
- `blockers`
