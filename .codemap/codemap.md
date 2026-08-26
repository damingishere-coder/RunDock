<!--
  This file:        .codemap/codemap.md   (written report)
  Interactive map:  .codemap/codemap.html
-->

# RunDock / Alter — Functional Module Quality Audit

> **Interactive view:** [`.codemap/codemap.html`](codemap.html) — per-module scores, findings, LoC, and the dependency graph. This file is the written report.

**Generated:** 2026-08-26 · **Modules:** 12 · **Size:** 75270 tracked LoC across 225 files

## Health by layer

| Layer | Modules | Avg score |
|---|--:|--:|
| 前端 · 应用壳 | 1 | 77 |
| 前端 · 功能页面 | 3 | 70 |
| 前端 · 数据访问 | 1 | 82 |
| 后端 · API 与系统边界 | 3 | 85 |
| 后端 · 进程运行核心 | 1 | 82 |
| 后端 · 状态与集成 | 2 | 88 |
| 交付 · 构建与文档 | 1 | 88 |

## Per-module lines of code & score

_LoC is the representative file/folder per module; folder-level modules overlap and are not additive._

### 前端 · 应用壳

| Module | LoC | Score | Tags |
|---|--:|:--|:--|
| React 应用壳与导航 | 4,002 | 77 B | bloat, god-component |

### 前端 · 功能页面

| Module | LoC | Score | Tags |
|---|--:|:--|:--|
| 前端进程与项目工作台 | 9,425 | 64 C | bloat, god-component |
| 前端设置、AI 与终端 | 6,534 | 78 B | bloat, god-component |
| 前端观测与运维页面 | 6,328 | 68 C | bloat, duplication, god-component |

### 前端 · 数据访问

| Module | LoC | Score | Tags |
|---|--:|:--|:--|
| 前端 API、认证与轮询层 | 3,820 | 82 B | bloat |

### 后端 · API 与系统边界

| Module | LoC | Score | Tags |
|---|--:|:--|:--|
| 认证与本机系统能力 API | 5,164 | 85 B | bloat |
| CLI、守护进程与 Web 入口 | 3,019 | 90 A | bloat |
| 进程、项目与 Ecosystem API | 2,288 | 80 B | bloat |

### 后端 · 进程运行核心

| Module | LoC | Score | Tags |
|---|--:|:--|:--|
| 进程监督与日志核心 | 6,167 | 82 B | bloat, god-component |

### 后端 · 状态与集成

| Module | LoC | Score | Tags |
|---|--:|:--|:--|
| AI、通知、隧道与观测集成 | 6,752 | 84 B | bloat |
| 配置、模型与 JSON 持久化 | 4,929 | 93 A | stub |

### 交付 · 构建与文档

| Module | LoC | Score | Tags |
|---|--:|:--|:--|
| 构建、发布与工程文档 | 16,842 | 88 B | duplication |

## Worst offenders

- **前端进程与项目工作台 (64/C)** — web-ui/src/pages/ProcessesPage.tsx:64: ProcessesPage 约 1,794 行，列表、批量操作、筛选、排序和多种弹窗仍在同一组件中，功能已受契约测试保护但维护耦合仍高。
- **前端观测与运维页面 (68/C)** — web-ui/src/pages/PortFinderPage.tsx:120: PortFinderPage 约 1,065 行，把端口扫描、进程匹配、筛选、分页和操作 UI 放在单一页面组件内。
- **React 应用壳与导航 (77/B)** — web-ui/src/App.tsx:77: Layout 仍在约 924 行的 App.tsx 中集中编排全局查询、通知、更新、路由和多个 overlay；侧栏、状态栏、服务器切换及认证已提取，但应用壳继续承担较高的变更耦合。
- **前端设置、AI 与终端 (78/B)** — web-ui/src/components/TerminalPanel.tsx:131: TerminalPanel 约 1,208 行，同时处理 WebSocket 生命周期、xterm、输入历史、布局、标签页和连接状态；资源清理已补齐，但组件边界仍偏大。
- **进程、项目与 Ecosystem API (80/B)** — src/api/routes/processes.rs:110: processes.rs 约 1,354 行，生命周期、日志、通知、项目关联与批量操作路由仍在单模块内；副作用与持久化已增加补偿事务，但文件级回归面仍较大。
- **前端 API、认证与轮询层 (82/B)** — web-ui/src/lib/api.ts:493: 统一 api 对象仍在单文件中聚合进程、项目、日志、认证、AI、Telegram、隧道和系统能力；错误校验与 transport 已统一，但后续可按领域拆文件以降低导航成本。
- **进程监督与日志核心 (82/B)** — src/process/manager.rs:62: manager.rs 仍约 3,200 行并承载 registry、spawn、restart、cron、health、watcher、metrics 与告警编排；进程身份和进程树已提取，功能竞态已收敛，但结构拆分仍是后续维护任务。
- **AI、通知、隧道与观测集成 (84/B)** — src/api/routes/ai.rs:402: ai.rs 约 1,495 行，仍集中 provider、Device Flow、模型发现、流式请求和设置路由；出站策略、上下文脱敏、大小上限与每流凭据已收紧，但按领域拆分仍有维护收益。src/api/routes/ai_context.rs:400-404 已将模拟 provider/GitHub token 改为运行时拼接，stale 触发点已消除。未发现 P0-P2 功能或安全风险。
- **认证与本机系统能力 API (85/B)** — src/config/env_file.rs:37: env 访问已限制到注册 cwd、直接子文件并拒绝最终符号链接；剩余父目录换位竞态仅能由具备同用户本机文件系统权限的进程触发，属于当前单用户威胁模型之外的平台限制。
- **构建、发布与工程文档 (88/B)** — README.md:16: 根 README 与 docs/README.md 仍分别维护部分重叠的安装、构建和功能说明；当前内容已同步，但缺少自动一致性检查。quality.yml:145-153 改为显式 bash 调用仅修复脚本执行权限问题，不引入 P0-P2 风险。

## All findings

### MED (11)

- **React 应用壳与导航** · `web-ui/src/App.tsx:77` — Layout 仍在约 924 行的 App.tsx 中集中编排全局查询、通知、更新、路由和多个 overlay；侧栏、状态栏、服务器切换及认证已提取，但应用壳继续承担较高的变更耦合。
- **前端进程与项目工作台** · `web-ui/src/pages/ProcessesPage.tsx:64` — ProcessesPage 约 1,794 行，列表、批量操作、筛选、排序和多种弹窗仍在同一组件中，功能已受契约测试保护但维护耦合仍高。
- **前端进程与项目工作台** · `web-ui/src/pages/ProcessDetailPage.tsx:95` — ProcessDetailPage 约 1,690 行，同时承载运行信息、日志、指标、Git、环境变量与操作面板；轮询已收敛为 single-flight，但视图职责仍需后续按面板拆分。
- **前端进程与项目工作台** · `web-ui/src/pages/ProjectsPage.tsx:41` — ProjectsPage 约 1,022 行，项目 CRUD、成员操作与桌面启动配置仍集中维护，结构性回归面偏大。
- **前端观测与运维页面** · `web-ui/src/pages/PortFinderPage.tsx:120` — PortFinderPage 约 1,065 行，把端口扫描、进程匹配、筛选、分页和操作 UI 放在单一页面组件内。
- **前端观测与运维页面** · `web-ui/src/pages/AnalyticsPage.tsx:1047` — AnalyticsPage 与 LogVolumePage:281 各自维护相近的批量统计拉取和聚合流程；两者已使用 single-flight，但统计数据适配仍重复。
- **前端观测与运维页面** · `web-ui/src/components/NotifModal.tsx:12` — NotifModal 与 NotificationsPage:127 分别维护通知事件默认结构，新增事件字段时存在两处同步成本。
- **前端设置、AI 与终端** · `web-ui/src/components/TerminalPanel.tsx:131` — TerminalPanel 约 1,208 行，同时处理 WebSocket 生命周期、xterm、输入历史、布局、标签页和连接状态；资源清理已补齐，但组件边界仍偏大。
- **进程、项目与 Ecosystem API** · `src/api/routes/processes.rs:110` — processes.rs 约 1,354 行，生命周期、日志、通知、项目关联与批量操作路由仍在单模块内；副作用与持久化已增加补偿事务，但文件级回归面仍较大。
- **进程监督与日志核心** · `src/process/manager.rs:62` — manager.rs 仍约 3,200 行并承载 registry、spawn、restart、cron、health、watcher、metrics 与告警编排；进程身份和进程树已提取，功能竞态已收敛，但结构拆分仍是后续维护任务。
- **AI、通知、隧道与观测集成** · `src/api/routes/ai.rs:402` — ai.rs 约 1,495 行，仍集中 provider、Device Flow、模型发现、流式请求和设置路由；出站策略、上下文脱敏、大小上限与每流凭据已收紧，但按领域拆分仍有维护收益。src/api/routes/ai_context.rs:400-404 已将模拟 provider/GitHub token 改为运行时拼接，stale 触发点已消除。未发现 P0-P2 功能或安全风险。

### LOW (7)

- **前端 API、认证与轮询层** · `web-ui/src/lib/api.ts:493` — 统一 api 对象仍在单文件中聚合进程、项目、日志、认证、AI、Telegram、隧道和系统能力；错误校验与 transport 已统一，但后续可按领域拆文件以降低导航成本。
- **CLI、守护进程与 Web 入口** · `src/daemon/mod.rs:1` — daemon/mod.rs 仍集中启动、持久化后台任务、PID 文件和重启 handoff 编排；状态提交竞态已线性化，但端到端 handoff 故障注入仍主要依赖 CI/隔离 smoke。
- **认证与本机系统能力 API** · `src/config/env_file.rs:37` — env 访问已限制到注册 cwd、直接子文件并拒绝最终符号链接；剩余父目录换位竞态仅能由具备同用户本机文件系统权限的进程触发，属于当前单用户威胁模型之外的平台限制。
- **认证与本机系统能力 API** · `src/api/routes/system.rs:84` — system.rs 约 837 行，仍同时承载健康、状态保存、受限文件访问、统计、重启和桌面打开等系统边界，后续可按能力分路由模块。
- **进程监督与日志核心** · `src/process/identity.rs:103` — macOS/BSD 缺少可移植的稳定进程组句柄，验证身份后到数字 PGID signal 间仍存在极窄复用窗口；Linux pidfd 与 Windows HANDLE/Job 路径不受此限制。
- **配置、模型与 JSON 持久化** · `src/config/auth_config.rs:15` — StoredPasskey 仍保留 raw JSON 兼容字段，但 API 与文档已明确 passkey 尚不支持；该占位结构只用于兼容读取，不代表已实现 WebAuthn。
- **构建、发布与工程文档** · `README.md:16` — 根 README 与 docs/README.md 仍分别维护部分重叠的安装、构建和功能说明；当前内容已同步，但缺少自动一致性检查。quality.yml:145-153 改为显式 bash 调用仅修复脚本执行权限问题，不引入 P0-P2 风险。

## Cross-cutting themes

- **控制面安全不变量已由代码强制.** 明文监听严格限制回环地址，浏览器来源采用回环 allowlist，高权限流接口需要 Bearer 或一次性 path-bound ticket；非回环部署不再依赖操作者自觉。
- **失败语义与资源上限已显式化.** API 业务失败、配置损坏、日志截断、轮询重叠和流式协议异常都有明确错误或上限；UI 区分 loading、failed、empty 与 stale，避免把故障伪装为成功。
- **文件状态采用串行原子提交与恢复协议.** 状态、项目及配置写入使用唯一临时文件、备份和语义校验；跨文件提交带 marker，副作用失败通过快照补偿，损坏数据不再静默覆盖主文件。
- **进程生命周期已统一所有权和身份验证.** 普通、watch、cron、自动重启与恢复路径共享受控 spawn；Windows Job、Linux pidfd/进程组和保存的启动时间防止误杀复用 PID，后台退出会触发持久化。
- **PR、主分支和 Release 共用质量门禁.** 固定 Rust、Node 与 npm 版本；执行 fmt、lint、typecheck、测试、覆盖率、audit、clippy、真实包 smoke 和 Sonar gate，发布权限与签名密钥按 job 隔离并校验固定指纹。
- **架构规模合适，剩余债务集中在大组件.** 单体 Rust daemon + React 控制台仍符合本地产品规模；P0-P2 功能风险已收敛，后续优先按行为测试拆分 ProcessManager、进程页面、Analytics、Terminal 与领域 API，而非引入微服务。
