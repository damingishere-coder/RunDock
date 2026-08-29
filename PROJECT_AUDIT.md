# RunDock P0–P2 工程整改验收报告

> 验收日期：2026-08-26（Asia/Shanghai）
> 方法：原始全面审计 + Codemap 增量审计 + Code Overhaul 复核 + SonarQube 复扫 + 全量构建/测试 + 隔离运行态 smoke
> 项目：`rundock-alter-v1-audit`
> 范围：仓库中的 Rust daemon/CLI、React 控制台、状态文件、外部集成、测试、CI、发布、安装与文档

## 0. 结论

原报告中的 **P0 4 项、P1 8 项及功能性 P2 均已完成整改并通过自动化验证**。控制面、env 文件边界、持久化、更新信任链、进程生命周期、PID 身份、出站请求、Telegram、发布权限、错误语义、资源上限和质量门禁已经从“依赖运行前提”改为代码或 CI 可执行的不变量。

本轮没有把“P2 完成”解释成机械清空所有大文件或 Sonar smell。高风险职责已经完成第一轮提取并补行为测试；剩余问题是大组件继续拆分、同用户本机 TOCTOU、非 Linux POSIX 极窄 PGID 复用窗口、低覆盖率与可访问性细节，均归入 P3/后续渐进治理，不再是 P0–P2 功能阻塞。

关键验收结果：

- Rust：`182` 个单元测试 + `3` 个集成测试全部通过；fmt、check、clippy `-D warnings` 全部通过。
- 前端：`25` 个测试文件、`107` 项测试全部通过；format、lint、typecheck、build 全部通过。
- 前端覆盖率：Statements `37.58%`、Branches `36.43%`、Functions `34.00%`、Lines `39.47%`。
- 依赖：npm `0 vulnerabilities`；RustSec `0 vulnerabilities`，仅 1 个允许的 unmaintained transitive warning。
- Codemap：12/12 模块最新，平均 `80.9/100`；`2 A / 8 B / 2 C`；`0 HIGH / 11 MED / 7 LOW`。
- SonarQube：分析任务成功，`0 vulnerabilities`、`0 security hotspots`、`19` 个 Bugs 且均为 Minor；Coverage `29.6%`。
- 隔离 smoke：端口 `33991` health=`ok`、Dashboard HTTP `200`、受控 shutdown 成功且进程/监听均退出。

## 1. 证据边界

### 已实际完成

- 检查了完整 Git diff、源文件、测试、依赖锁文件、CI、安装/发布脚本、Codemap 和本机 SonarQube 结果。
- 使用工作区隔离目录 `.codex/validation-data/final-20260826-1051` 和端口 `33991` 启动当前构建。
- 复核真实 `2999/5173` 监听仍属于原安装版 daemon/Vite 进程；未替换、停止或接管。
- 运行了 Rust/前端全套门禁、依赖审计、YAML/PowerShell 语法检查和秘密扫描。

### 未执行或不能由本机证明

- 没有调用真实 AI、Webhook、Telegram、Tunnel、自更新、安装、发布、部署或远程写操作。
- 没有读取或修改真实 `%APPDATA%\alter-pm2` 数据。
- 没有本机 Git Bash，因此 Debian shell 脚本由 CI 的 `bash -n`/真实打包 smoke 负责；本机只完成静态复核。
- 本机未安装 `cargo-llvm-cov`/`llvm-tools-preview`，所以 Sonar 只导入前端 LCOV；CI 已固定 `cargo-llvm-cov 0.9.0` 并要求 Rust line coverage 至少 20%。
- Sonar Scanner 上传成功不等于 Quality Gate 通过；第 8 节单独报告门禁失败条件。

## 2. P0 完成矩阵

| ID | 状态 | 关键实现 | 验证 |
|---|---|---|---|
| P0-1 控制面认证/CORS | 完成 | 明文 listener 仅允许 loopback；非回环拒绝启动；CORS 仅接受受信 loopback Origin；长连接使用一次性、路径/查询绑定 ticket；普通会话不再进入 URL | auth/CORS/stream ticket/loopback 单测；隔离 daemon smoke |
| P0-2 env 路径穿越 | 完成 | 文件名必须是单一安全 env 组件；限定到已注册进程 cwd；拒绝绝对路径、Windows/Unix traversal、ADS、符号链接/reparse point；读取 no-follow 且限制大小 | 跨平台 traversal、dangling link、merge_env 单测 |
| P0-3 状态并发与损坏恢复 | 完成 | 配置/状态写入串行；唯一临时文件；原子替换；主/备份语义校验；跨文件 marker；损坏主文件只能从有效备份恢复，不能伪装首次运行 | atomic file、state transaction、重复 ID、并发/恢复单测 |
| P0-4 自更新信任链 | 完成 | 固定仓库/tag/平台资产；限制重定向与大小；必须有 SHA-256；Windows 安装器还校验固定发布者证书 hash；任何缺项 fail-closed | URL、SemVer、digest、证书 hash、篡改候选单测 |

## 3. P1 完成矩阵

| ID | 状态 | 关键实现与证据 |
|---|---|---|
| P1-1 副作用/持久化部分成功 | 完成 | 进程、项目、端口和 Git 操作保存前捕获完整 snapshot；失败执行补偿恢复；无法安全回滚时返回明确 partial-state 错误，不再重复执行未知副作用。 |
| P1-2 watcher 所有权 | 完成 | watcher handle 保存在 `ManagedProcess`，stop/restart/rollback 明确释放；创建失败不再静默忽略。 |
| P1-3 生命周期漂移 | 完成 | start/watch/crash restart/cron/adopt 共用受控 spawn/commit；hooks、health、watcher、日志与 generation 校验一致；spawn 聚合 future 不再在 OS child 创建后被外层 timeout 取消。 |
| P1-4 LogWriter 泄漏 | 完成 | writer 具有明确停止/Drop 所有权；替换/stop 会终止旧任务；轮转写入错误可观察。 |
| P1-5 PID 复用/进程树 | 完成 | 保存不可变启动时间；Windows 使用稳定 HANDLE + Job Object，Linux 使用 pidfd + 独立进程组；cron/自动重启也转移树所有权；身份不明时拒绝 kill。 |
| P1-6 AI/通知/隧道出站 | 完成 | 统一协议/主机/IP 策略，拒绝凭据 URL、私网/回环绕过与 DNS rebinding；AI 上下文脱敏并有总量限制；Device Flow 每流绑定随机 polling credential；installer pipe 始终 drain 且输出有界。 |
| P1-7 Telegram 空白名单 | 完成 | enabled bot 必须有 token 和显式 chat/sender allowlist；checkpoint 与 token fingerprint 绑定；空名单 fail-closed。 |
| P1-8 发布安全 | 完成 | workflow 默认 `contents: read`；打包/签名 job 无写权限；Release 上传在只消费 artifact 的独立 write job；APT 缺签名或固定 fingerprint 不匹配即停止；Actions pin SHA。 |

额外复核关闭的问题：

- session `DashMap` guard 不再跨 SSE/WebSocket `await` 持有，logout 不会被长连接阻塞。
- logout、密码/PIN/安全设置变更在锁内复核 session，避免撤销竞态。
- daemon restart handoff 的提交与外部 shutdown 由同一互斥锁线性化；外部 shutdown 先到时新 daemon 会被回收。
- Unix 活跃进程组 leader 身份暂时不可见时 fail-closed，不会继续 signal 数字 PGID。

## 4. P2 完成矩阵

| 主题 | 状态 | 本轮结果 |
|---|---|---|
| 错误语义 | 完成 | API 统一非 2xx 与 `success:false`；UI 区分 loading/failed/empty/stale/partial；配置损坏、日志/流解析、保存和刷新错误不再显示成功 toast 或零值。 |
| 日志/流资源上限 | 完成 | 日志 tail 从文件尾部有界读取；lines/bytes/aggregate 有硬上限；SSE/脚本/AI/provider 行和总量有界；前端日志 ring buffer。 |
| 轮询并发 | 完成 | 引入 single-flight + AbortController + generation；页面卸载/服务器切换阻止旧响应回写；health 告警有抑制、恢复和退避。 |
| CI/质量门禁 | 完成 | PR/main/codex 分支运行 Rust/前端质量；Release 复用门禁；固定 Rust 1.98.0、Node 24.15.0、npm 11.12.1；coverage/audit/package smoke/Sonar 等待齐备。 |
| npm 漏洞 | 完成 | lockfile 更新后 `npm audit --audit-level=high` 为 0；未使用 `audit fix --force`。 |
| Rust 直接依赖 | 完成 | 删除未引用 `thiserror`、`tokio-util`、`rand`，使用实际需要的 `rand_core`；保留经编译证明需要的传递依赖。 |
| 锁文件真相 | 完成 | 删除漂移的 `bun.lock`，npm/package-lock/CI 成为唯一受支持路径。 |
| bundle | 完成 | 路由/供应商分块并加入 500 KiB 强制预算；最大 JS chunk `290.60 kB`（gzip `72.35 kB`）。 |
| dead/legacy | 完成 | 删除 Vite 模板 CSS/示例测试、未接入通知旧架构、rolling restart stub、未使用 hook 与旧 release 文档；保留兼容字段。 |
| 热点职责第一轮拆分 | 完成 | App 提取 AuthGuard/AppSidebar/ServerSwitcher/StatusBar/SystemStats/ErrorBoundary；AI 提取 context/脱敏；进程核心提取 identity/tree；前端 API 提取 transport/schema/domain helper。 |
| 测试补齐 | 完成 | 覆盖 auth/CORS/ticket、env path、atomic state、事务恢复、PID/tree/lifecycle race、出站策略、Device Flow credential、UI 错误态、single-flight、server switching 与关键页面。 |

结构性 P2 的验收标准是“先提取最高风险边界并以行为测试固定语义”，不是一次把所有大文件重写。Codemap 仍将 Processes/ProcessDetail/PortFinder/Terminal/manager/AI 等标为 MED bloat，这些已降为 P3 维护债，不应在同一安全整改提交里继续做高风险大重构。

## 5. 架构与数据边界

整改后的核心链路：

```text
Browser / CLI
  -> strict loopback listener + trusted Origin / Bearer / one-time ticket
  -> Axum route validation + bounded blocking/outbound work
  -> ProcessManager generation + ProcessIdentity + ProcessTreeGuard
  -> serial mutation locks + runtime snapshots + compensate
  -> atomic primary/backup pair + semantic validation + transaction marker
```

项目仍没有业务数据库；持久化是受控 JSON 文件集。没有证据支持为此引入 Redis、消息队列、微服务或数据库迁移。更合理的后续工作是继续缩小模块内部职责。

秘密边界：

- API 只返回 `*_set`/hint，AI、Telegram、通知等设置不再回传原始秘密。
- ProcessInfo 对外投影不再泄露完整 env。
- AI 诊断在出站前处理常见 key/token/header/URL query 并限制 UTF-8 字符数。
- 当前工作树与 Sonar TextAndSecretsSensor 未发现凭据命中；这不是对完整 Git 历史的绝对保证。

## 6. Codemap 最终结果

本轮扫描 `225` 个跟踪文件、`75,264` 行（包含 lockfile、文档与交付资产），12 个模块全部 fresh：

| 等级 | 模块 |
|---|---|
| A | bootstrap_daemon 90、state_persistence 93 |
| B | frontend_shell 77、frontend_settings_ai_terminal 78、frontend_transport 82、api_process_projects 80、api_security_os 85、process_lifecycle 82、integrations_observability 84、delivery_docs 88 |
| C | frontend_process_projects 64、frontend_operations 68 |

平均分 `80.9/100`，发现项 `0 HIGH / 11 MED / 7 LOW`。交互地图与 Markdown 均已重建：`.codemap/codemap.html`、`.codemap/codemap.md`。

## 7. 自动化验证

| 命令 | 结果 |
|---|---|
| `cargo fmt --all -- --check` | 通过 |
| `cargo check --all-targets --locked` | 通过；本机仅提示资源编译器不可用，debug build 不嵌入图标 |
| `cargo test --all-targets --locked` | 通过；182 unit + 3 integration |
| `cargo clippy --all-targets --all-features --locked -- -D warnings` | 通过 |
| `npm run format:check` | 通过 |
| `npm run lint` | 通过 |
| `npm run typecheck` | 通过 |
| `npm test` | 通过；25 files / 107 tests |
| `npm run test:coverage` | 通过；37.58 / 36.43 / 34.00 / 39.47 |
| `npm run build` | 通过；1,876 modules；max chunk 290.60 kB |
| `npm audit --audit-level=high` | 通过；0 vulnerabilities |
| `cargo-audit 0.22.2 audit --no-fetch` | 通过；380 dependencies；0 vulnerabilities；1 allowed warning |
| PyYAML parse workflows/lefthook | 通过 |
| PowerShell AST parse `scripts/release.ps1` | 通过 |
| Codemap scan | 通过；12/12 fresh |
| Sonar analysis task | SUCCESS；Analysis ID `d635c7f4-33fc-4e14-9a9e-dbfcee18d119` |

前端测试有 jsdom 对 Canvas/navigation 的 3 条非致命提示，不影响结果。RustSec 唯一 warning 是 `serial 0.4.0` unmaintained，经 `portable-pty 0.8.1` 传递引入；没有已知 vulnerability，应在上游替换可行时升级，不应为消警贸然改 terminal backend。

## 8. SonarQube 最终结果

本机 SonarQube Community Build `26.8.0.126808`，最终扫描 210 个文件并成功上传：

| 指标 | 原审计快照 | 最终 |
|---|---:|---:|
| NCLOC | 29,690 / 167 metric files | 53,281 |
| Bugs | 33 | 19（全部 Minor） |
| Vulnerabilities | 1 | 0 |
| Security Hotspots | 0 | 0 |
| Code Smells | 551 | 571 |
| Coverage | 0.0% | 29.6% |
| Duplication | 3.2% | 3.4% |

NCLOC 和 smells 不能直接横向解释为退化：最终扫描纳入了更多生产源文件与本轮新增边界代码；本轮目标不是机械清空全部 smell。两个 `String.sort()` Critical Bugs 已改为 `localeCompare` 并在复扫后消失，剩余 19 个 Bugs 均是 Sonar 将 click/keyboard 可访问性规则归类为 Minor。

### Quality Gate：真实状态是 ERROR

分析任务 `SUCCESS`，但服务端门禁未通过：

- new coverage `32.9% < 80%`
- new duplicated lines density `3.82426% > 3%`
- new violations `291 > 0`

这与旧报告中“Quality Gate OK 但没有 conditions”不同：现在门禁已有 3 个真实条件，并会 fail-closed。当前工作树相对旧版本包含整轮大规模整改，且本机没有 Rust LCOV，所以无法诚实声称达到 80% 新代码覆盖率。CI 会生成 Rust LCOV，但 80% 仍是后续测试投资目标；不应通过排除业务源文件或伪造覆盖率让门禁变绿。

## 9. 隔离运行态验收

使用：

- data：`.codex/validation-data/final-20260826-1051`
- logs：同目录 `logs/`
- bind：`127.0.0.1:33991`

结果：

- `/api/v1/system/health`：`status=ok`，version `1.1.0`
- `/`：HTTP `200`
- `POST /api/v1/system/shutdown`：`success=true`
- PID `129700` 已退出，端口 `33991` 无残留 listener
- 原端口 `2999` 仍由 `C:\Program Files\alter\alter.exe` 管理；`5173` 仍为原 Vite 进程

因此 smoke 证明的是当前构建在隔离数据和备用端口可启动/关闭，不是对真实用户状态的迁移，也不是发布或部署。

## 10. 剩余 P3 / 平台限制

1. `manager.rs`、ProcessesPage、ProcessDetailPage、AnalyticsPage、PortFinderPage、TerminalPanel 和 `ai.rs` 仍偏大，应按既有测试逐模块拆分。
2. 同一 Windows 用户下的恶意本机进程仍可能在 env 校验与按路径 rename 之间替换父目录；远程 traversal 与最终 symlink/reparse 已关闭。完全消除需目录句柄相对 I/O。
3. macOS/BSD 没有可移植稳定进程组句柄；验证 leader 身份到数字 PGID signal 间有极窄复用窗口。Linux pidfd、Windows HANDLE/Job 不受该限制。
4. 锁屏服务端 logout 是 best-effort；网络失败时本地 token 已清除，但服务端 session 会保留到 24 小时过期。
5. `StoredPasskey` 仅为兼容 raw 字段；API/文档明确 passkey 尚未实现。
6. 根 README 与 docs/README 有重叠内容，后续应确定单一权威来源或增加一致性检查。
7. 本机 Sonar Rust 插件产生 comment highlighting offset 警告，且缺 Rust LCOV；CI 工具链是权威覆盖率验证路径。

## 11. 最终判断

RunDock 仍保持“单体 Rust daemon + React 控制台 + 本地文件状态”的合适架构，但其安全与可靠性已经不再主要依靠单用户、回环地址和低并发碰巧成立。P0/P1 的高风险入口和一致性问题已形成代码级护栏，功能性 P2 已形成测试与 CI 门禁；剩余工作是可规划、可渐进的 P3 维护和覆盖率投资。
