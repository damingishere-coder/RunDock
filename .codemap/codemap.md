<!--
  This file:        .codemap/codemap.md   (written report)
  Interactive map:  .codemap/codemap.html
-->

# RunDock / Alter — Functional Module Quality Audit

> **Interactive view:** [`.codemap/codemap.html`](codemap.html) — per-module scores, findings, LoC, and the dependency graph. This file is the written report.

**Generated:** 2026-08-30 · **Modules:** 13 · **Size:** 85193 tracked LoC across 241 files

## Health by layer

| Layer | Modules | Avg score |
|---|--:|--:|
| 前端 · 应用壳 | 2 | 79 |
| 前端 · 功能页面 | 3 | 62 |
| 前端 · 数据访问 | 1 | 69 |
| 后端 · API 与系统边界 | 3 | 77 |
| 后端 · 进程运行核心 | 1 | 78 |
| 后端 · 状态与集成 | 2 | 83 |
| 交付 · 构建与文档 | 1 | 88 |

## Per-module lines of code & score

_LoC is the representative file/folder per module; folder-level modules overlap and are not additive._

### 前端 · 应用壳

| Module | LoC | Score | Tags |
|---|--:|:--|:--|
| Windows 桌面壳与托盘入口 | 7,596 | 82 B | silent-except |
| React 应用壳与导航 | 3,690 | 76 B | bloat, god-component |

### 前端 · 功能页面

| Module | LoC | Score | Tags |
|---|--:|:--|:--|
| 前端进程与项目工作台 | 9,428 | 56 D | bloat, god-component, duplication, over-fit |
| 前端设置、AI 与终端 | 6,489 | 62 C | bloat, god-component |
| 前端观测与运维页面 | 6,341 | 68 C | bloat, duplication, god-component |

### 前端 · 数据访问

| Module | LoC | Score | Tags |
|---|--:|:--|:--|
| 前端 API、认证与轮询层 | 3,904 | 69 C | any-escape |

### 后端 · API 与系统边界

| Module | LoC | Score | Tags |
|---|--:|:--|:--|
| 认证与本机系统能力 API | 5,253 | 82 B | — |
| CLI、守护进程与 Web 入口 | 3,184 | 78 B | fallback, legacy, glue |
| 进程、项目与 Ecosystem API | 2,331 | 70 C | bloat, duplication |

### 后端 · 进程运行核心

| Module | LoC | Score | Tags |
|---|--:|:--|:--|
| 进程监督与日志核心 | 7,397 | 78 B | fallback |

### 后端 · 状态与集成

| Module | LoC | Score | Tags |
|---|--:|:--|:--|
| AI、通知、隧道与观测集成 | 6,759 | 78 B | fallback |
| 配置、模型与 JSON 持久化 | 5,224 | 88 B | — |

### 交付 · 构建与文档

| Module | LoC | Score | Tags |
|---|--:|:--|:--|
| 构建、发布与工程文档 | 17,597 | 88 B | duplication |

## Worst offenders

- **前端进程与项目工作台 (56/D)** — web-ui/src/pages/ProcessesPage.tsx:64: ProcessesPage 共 1794 行，同时承载列表状态、筛选、批量操作、卡片视图、表格视图；ProcessCard(766-1097) 与 ProcessRow(1117-1427) 重复维护操作定义和渲染逻辑，已构成真实的 god-file 维护风险。
- **前端设置、AI 与终端 (62/C)** — web-ui/src/components/AiPanel.tsx:18: 前端 MAX_CHAT_MESSAGES=100，发送时 history 仍截取 100 条；服务端拒绝超过 50 条。连续约 26 轮后请求持续失败，且失败请求仍留下空 assistant 占位，必须清空聊天才能恢复。
- **前端观测与运维页面 (68/C)** — web-ui/src/pages/PortFinderPage.tsx:120: PortFinderPage 约 1,065 行，把端口扫描、进程匹配、筛选、分页和操作 UI 放在单一页面组件内。
- **前端 API、认证与轮询层 (69/C)** — web-ui/src/hooks/useNotificationTray.ts:30-36: INACTIVE 未包含 stopping；若轮询捕获 running/watching→stopping→stopped，中间状态会覆盖前态，最终不会生成 stopped 通知。现有测试未覆盖该状态链。
- **进程、项目与 Ecosystem API (70/C)** — src/api/routes/processes.rs:23: processes.rs 当前约 1369 行，同时承载进程 CRUD、生命周期、日志 SSE、环境文件、通知、项目关联、克隆和命名空间批量操作，职责边界和回归面仍然过宽；属于真实维护风险但不是本轮阻断项。
- **React 应用壳与导航 (76/B)** — web-ui/src/App.tsx:81: Layout 延伸至约 841 行，单组件同时管理设置、进程与项目健康轮询、通知、AI、终端、统计、移动端焦点及完整路由 overlay 装配，跨领域状态耦合较高；当前未发现新的路由或移动导航回归。
- **CLI、守护进程与 Web 入口 (78/B)** — src/utils/pid.rs:174: capture_process_identity(...)=None 会被 is_some_and 直接解释为 daemon 不在运行；write_pid_file 随后可能删除仍存活 daemon 的 PID 文件，旧进程继续监听而新实例因端口冲突启动失败，违背身份未知时 fail-closed 的不变量。
- **进程监督与日志核心 (78/B)** — src/process/tree.rs:114: runner/hooks 在附加失败且兜底清理无法确认时返回明确错误；Child 不再进入 ManagedChild/注册表，极端残留树只能依赖外部清理。属于 fail-closed 路径的运维风险，不是旧 HIGH。
- **AI、通知、隧道与观测集成 (78/B)** — src/tunnel/mod.rs:241: TunnelManager::stop 仅调用 kill_orphan_pid；Windows 该函数只终止并等待根 PID，ProcessTreeGuard 仍由后台 watch_output 任务持有。停止请求会先删除 pids 并标记 Stopped，树级 Job 清理要等任务结束；若 descendants 继承输出管道，watch_output 最长可继续等待45秒，期间残留进程已脱离跟踪且可能占用端口。当前无 stop/descendant 清理测试。
- **认证与本机系统能力 API (82/B)** — src/api/routes/ports.rs:195: Windows 分支执行 netstat -ano 并包含 ESTABLISHED 等连接，Unix 分支执行带 -l 的 ss 仅返回监听 socket，导致同一 /ports API 的状态集合跨平台不一致；前端 ESTABLISHED 筛选在 Linux 无结果。Ss/Netstat parser 已按命令来源分流，IPv4/IPv6、TCP/UDP、users/PID fixture 无回归。

## All findings

### HIGH (1)

- **前端进程与项目工作台** · `web-ui/src/pages/ProcessesPage.tsx:64` — ProcessesPage 共 1794 行，同时承载列表状态、筛选、批量操作、卡片视图、表格视图；ProcessCard(766-1097) 与 ProcessRow(1117-1427) 重复维护操作定义和渲染逻辑，已构成真实的 god-file 维护风险。

### MED (29)

- **React 应用壳与导航** · `web-ui/src/App.tsx:81` — Layout 延伸至约 841 行，单组件同时管理设置、进程与项目健康轮询、通知、AI、终端、统计、移动端焦点及完整路由 overlay 装配，跨领域状态耦合较高；当前未发现新的路由或移动导航回归。
- **前端进程与项目工作台** · `web-ui/src/components/CronExpressionInput.tsx:29` — 分段步长解析对 1-10/2 仅 parseInt(rangeStr) 得到起点 1，未校验上界 10；因此预览会把 11、13 等超出范围的分钟错误当作匹配。当前未见该组件测试。
- **前端进程与项目工作台** · `web-ui/src/components/EnvFileModal.tsx:402` — 底部取消按钮直接调用 onClose，绕过 requestClose 的 dirty/mutation 保护；未保存内容可被无确认丢弃，保存/重启进行中也可关闭。
- **前端进程与项目工作台** · `web-ui/src/pages/ProcessesPage.tsx:1020` — 终端 action 设置 disabled: !p.cwd，但卡片和表格 inline.map(1080-1089、1410-1419) 未把 disabled 传给 ActionBtn，RowOverflowMenu(1563-1671) 也不支持/处理该字段；无 cwd 时按钮仍可点击并静默 no-op。
- **前端进程与项目工作台** · `web-ui/src/pages/StartPage.tsx:143` — saveEnvFile 写入完成后无版本/代际校验即 setEnvDirty(false)，而 textarea(461-483) 在保存期间仍可编辑；并且启动按钮(339)只检查 loading，不等待 envSaving，存在并发编辑被误标记为已保存及启动读取旧文件的竞态。
- **前端进程与项目工作台** · `web-ui/src/components/EnvFilePanel.tsx:19` — EnvFilePanel(432 行) 与 EnvFileModal(468 行) 两套活动组件重复实现环境文件列表、加载、编辑、保存、同步和关闭状态机，并分别使用 onRestart 与 onRestarted 两种回调契约，存在持续漂移和修复遗漏风险。
- **前端观测与运维页面** · `web-ui/src/pages/PortFinderPage.tsx:120` — PortFinderPage 约 1,065 行，把端口扫描、进程匹配、筛选、分页和操作 UI 放在单一页面组件内。
- **前端观测与运维页面** · `web-ui/src/pages/AnalyticsPage.tsx:1047` — AnalyticsPage 与 LogVolumePage:281 各自维护相近的批量统计拉取和聚合流程；两者已使用 single-flight，但统计数据适配仍重复。
- **前端观测与运维页面** · `web-ui/src/components/NotifModal.tsx:12` — NotifModal 与 NotificationsPage:127 分别维护通知事件默认结构，新增事件字段时存在两处同步成本。
- **前端设置、AI 与终端** · `web-ui/src/components/AiPanel.tsx:18` — 前端 MAX_CHAT_MESSAGES=100，发送时 history 仍截取 100 条；服务端拒绝超过 50 条。连续约 26 轮后请求持续失败，且失败请求仍留下空 assistant 占位，必须清空聊天才能恢复。
- **前端设置、AI 与终端** · `web-ui/src/components/settings/AiTab.tsx:166` — 服务端可返回 status=idle，但前端未处理该分支；authPhase 仍为 in_progress，轮询持续到本地超时，界面长期停留在等待授权。
- **前端设置、AI 与终端** · `web-ui/src/components/settings/AiTab.tsx:306` — 取消 Device Flow 只清理前端状态，没有取消请求；后端流程保留至过期且最多允许 8 个活动流程，重复取消可耗尽登录容量最长约 30 分钟。
- **前端设置、AI 与终端** · `web-ui/src/components/settings/AiTab.tsx:423` — 设置描述声明关闭后不显示侧栏 AI 按钮，但 App.tsx 始终渲染并允许打开 AiPanel；后端 enabled=false 时发送请求才返回禁用错误，开关未实现声明的界面效果。
- **前端设置、AI 与终端** · `web-ui/src/components/TerminalPanel.tsx:130` — 约 1136 行的单一组件同时处理 xterm 生命周期、WebSocket ticket/重试、输入 ANSI 解析、历史持久化、标签页/分屏、布局和快捷键；边界修改会牵动较大的维护与回归面。
- **前端 API、认证与轮询层** · `web-ui/src/hooks/useNotificationTray.ts:30-36` — INACTIVE 未包含 stopping；若轮询捕获 running/watching→stopping→stopped，中间状态会覆盖前态，最终不会生成 stopped 通知。现有测试未覆盖该状态链。
- **前端 API、认证与轮询层** · `web-ui/src/lib/api.ts:401-402` — 通用 request<T> 将 JSON.parse 结果直接断言为 T，多个 API 端点仍绕过 validatedRequest；服务端返回 2xx 但结构漂移时，错误数据会静默进入业务层。
- **前端 API、认证与轮询层** · `web-ui/src/lib/schemas.ts:216-263` — isProcessInfo 未校验可选 notify 字段；畸形非空 notify 仍能通过进程响应校验，ProcessNotifModal 后续读取 config.events 可能触发运行时错误。
- **CLI、守护进程与 Web 入口** · `src/utils/pid.rs:174` — capture_process_identity(...)=None 会被 is_some_and 直接解释为 daemon 不在运行；write_pid_file 随后可能删除仍存活 daemon 的 PID 文件，旧进程继续监听而新实例因端口冲突启动失败，违背身份未知时 fail-closed 的不变量。
- **CLI、守护进程与 Web 入口** · `src/utils/pid.rs:141` — 兼容 numeric PID 时将 start_time_secs 置零、executable 置空；daemon_record_matches_identity 会跳过启动时间并退回 current_exe。旧 PID 被复用给同路径普通 CLI 进程时可能误判为 daemon，阻塞启动/重启并削弱 PID reuse 防护。
- **进程、项目与 Ecosystem API** · `src/api/routes/processes.rs:23` — processes.rs 当前约 1369 行，同时承载进程 CRUD、生命周期、日志 SSE、环境文件、通知、项目关联、克隆和命名空间批量操作，职责边界和回归面仍然过宽；属于真实维护风险但不是本轮阻断项。
- **进程、项目与 Ecosystem API** · `src/api/routes/ecosystem.rs:77` — Ecosystem 导入只在每个应用启动前检查总截止时间，manager.start(app).await 未受剩余时间约束；单个启动或 hook 阻塞时可超过 5 分钟并长期持有 state_mutation_lock。
- **进程、项目与 Ecosystem API** · `src/api/routes/ecosystem.rs:84` — 项目关联失败且 manager.delete 清理也失败时，若 save_to_disk 同样失败，没有设置 background_persistence_error，健康检查无法持续暴露孤儿进程诊断。
- **进程、项目与 Ecosystem API** · `src/api/routes/processes.rs:714` — UpdateProcessRequest、项目 PATCH 和 notifications PATCH 已具备 missing/null/value 分支，但测试主要验证解析，缺少 handler 配置合并、落盘和持久化失败回滚集成测试。
- **认证与本机系统能力 API** · `src/api/routes/ports.rs:195` — Windows 分支执行 netstat -ano 并包含 ESTABLISHED 等连接，Unix 分支执行带 -l 的 ss 仅返回监听 socket，导致同一 /ports API 的状态集合跨平台不一致；前端 ESTABLISHED 筛选在 Linux 无结果。Ss/Netstat parser 已按命令来源分流，IPv4/IPv6、TCP/UDP、users/PID fixture 无回归。
- **进程监督与日志核心** · `src/process/tree.rs:114` — runner/hooks 在附加失败且兜底清理无法确认时返回明确错误；Child 不再进入 ManagedChild/注册表，极端残留树只能依赖外部清理。属于 fail-closed 路径的运维风险，不是旧 HIGH。
- **进程监督与日志核心** · `src/process/manager.rs:494` — 恢复 adopted process 时已设置 Watching；FileWatcher::start 失败仅记录日志，file_watcher 保持 None，对外仍报告 Watching 但文件变化不会触发重启。
- **AI、通知、隧道与观测集成** · `src/tunnel/mod.rs:241` — TunnelManager::stop 仅调用 kill_orphan_pid；Windows 该函数只终止并等待根 PID，ProcessTreeGuard 仍由后台 watch_output 任务持有。停止请求会先删除 pids 并标记 Stopped，树级 Job 清理要等任务结束；若 descendants 继承输出管道，watch_output 最长可继续等待45秒，期间残留进程已脱离跟踪且可能占用端口。当前无 stop/descendant 清理测试。
- **AI、通知、隧道与观测集成** · `src/api/routes/tunnels.rs:551` — install_provider_stream 的超时和等待失败分支只 kill_spawned_process 后 drop(ProcessTreeGuard)，未调用 terminate_and_wait；Drop 仅发起 Unix 进程组 SIGKILL 或关闭 Windows Job，未确认 descendants 已消失，随后仍发送 done=true/ok=false。非流式 install_provider 在同类分支显式 terminate_and_wait，存在路径语义不一致。
- **Windows 桌面壳与托盘入口** · `desktop-shell/src/windows_app.rs:173` — 自启动初始化与托盘切换都静默丢弃错误：initialize_autostart 在 :181 仅判断 is_ok()，失败无提示；托盘 :296 用 unwrap_or(false)，:298-301 对 enable/disable 仅判断结果，失败不更新勾选也不给用户诊断。登录自启动可能未生效而界面无可操作反馈。

### LOW (5)

- **React 应用壳与导航** · `web-ui/src/components/ServerSwitcher.tsx:84` — ServerSwitcher 约 645 行，同时处理本地、远程与 SSH 配置、存储恢复、活动服务器切换、隧道命令预览及表单 UI；属于可延期的维护性膨胀，不构成当前阻断。
- **前端进程与项目工作台** · `web-ui/src/components/FolderBrowser.tsx:94` — 路径已统一替换为斜杠后，面包屑仍固定用反斜杠 join；Unix 路径点击面包屑会生成类似 home\me 的错误路径。
- **CLI、守护进程与 Web 入口** · `src/lib.rs:204` — Web 入口直接拼接 host；--host ::1 会生成缺少 IPv6 authority 方括号的 URL，浏览器启动失败。
- **进程、项目与 Ecosystem API** · `src/api/routes/processes.rs:56` — process_is_active 与 projects.rs 的 is_active 重复维护 PID 与状态活跃判定，状态枚举扩展时存在轻微漂移风险。
- **构建、发布与工程文档** · `README.md:16` — 根 README 与 docs/README.md 仍分别维护部分重叠的安装、构建和功能说明；当前内容已同步，但缺少自动一致性检查。Debian smoke 在首次安装和升级后均等待健康响应，卸载后进行有界停止、包移除、状态保留和 unit 清理检查，并在超时/失败时输出 systemd 状态与 journal；未发现新的 HIGH。

## Cross-cutting themes

- **P0 安全边界总体保持，但开发代理存在旁路.** 生产 daemon 的回环、鉴权、CORS 与出站策略仍有效；Vite 开发服务绑定 0.0.0.0 并代理本机 API，密码关闭时会把控制面间接暴露给局域网。
- **进程生命周期仍有未合流的 Cron 分支.** Cron 路径复制了普通 spawn/commit 流程，退出与树所有权失败时可能只处理根进程或遗留 descendants；恢复 watcher 也可能显示假 Watching 状态。
- **前端整改引入了可复现的交互回归.** 条件化 single-flight 轮询可让页面永远不做首次加载，env 保存并重启会重复请求；Linux 端口解析还会丢弃常见 ss 输出。
- **PATCH 与检查点的一致性边界仍不完整.** 进程和项目补丁无法区分字段缺失与显式清空，Telegram checkpoint 的单调性检查与写入没有同一把锁，极端并发下可能回退。
- **桌面壳已成为正式入口但质量门禁未完全覆盖.** 桌面壳有独立测试和 Windows 构建 smoke，但未进入 Sonar、覆盖率与独立 clippy/fmt 门禁；Linux 包和正式 release artifact 的安装生命周期也缺少端到端验证。
- **停止大范围洁癖重构，优先关闭少量运行语义缺陷.** 单体 Rust daemon + React 控制台仍适合产品规模；大文件和重复代码只在已经造成具体缺陷的生命周期与轮询路径上做窄修，不建议全面拆分或引入微服务。
