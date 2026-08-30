<!--
  This file:        .codemap/codemap.md   (written report)
  Interactive map:  .codemap/codemap.html
-->

# RunDock / Alter — Functional Module Quality Audit

> **Interactive view:** [`.codemap/codemap.html`](codemap.html) — per-module scores, findings, LoC, and the dependency graph. This file is the written report.

**Generated:** 2026-08-30 · **Modules:** 13 · **Size:** 83436 tracked LoC across 236 files

## Health by layer

| Layer | Modules | Avg score |
|---|--:|--:|
| 前端 · 应用壳 | 2 | 79 |
| 前端 · 功能页面 | 3 | 61 |
| 前端 · 数据访问 | 1 | 68 |
| 后端 · API 与系统边界 | 3 | 65 |
| 后端 · 进程运行核心 | 1 | 52 |
| 后端 · 状态与集成 | 2 | 84 |
| 交付 · 构建与文档 | 1 | 72 |

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
| 前端进程与项目工作台 | 9,425 | 52 D | bloat, god-component, duplication, fallback, over-fit |
| 前端设置、AI 与终端 | 6,489 | 78 B | bloat, god-component |
| 前端观测与运维页面 | 6,328 | 52 D | bloat, duplication, god-component |

### 前端 · 数据访问

| Module | LoC | Score | Tags |
|---|--:|:--|:--|
| 前端 API、认证与轮询层 | 3,820 | 68 C | bloat, any-escape |

### 后端 · API 与系统边界

| Module | LoC | Score | Tags |
|---|--:|:--|:--|
| 认证与本机系统能力 API | 5,164 | 56 D | bloat, over-fit |
| CLI、守护进程与 Web 入口 | 3,184 | 78 B | fallback, legacy, glue |
| 进程、项目与 Ecosystem API | 2,288 | 62 C | bloat |

### 后端 · 进程运行核心

| Module | LoC | Score | Tags |
|---|--:|:--|:--|
| 进程监督与日志核心 | 6,380 | 52 D | bloat, god-component, duplication |

### 后端 · 状态与集成

| Module | LoC | Score | Tags |
|---|--:|:--|:--|
| AI、通知、隧道与观测集成 | 6,751 | 84 B | bloat |
| 配置、模型与 JSON 持久化 | 4,929 | 84 B | — |

### 交付 · 构建与文档

| Module | LoC | Score | Tags |
|---|--:|:--|:--|
| 构建、发布与工程文档 | 17,392 | 72 C | glue |

## Worst offenders

- **前端进程与项目工作台 (52/D)** — web-ui/src/components/EnvFileModal.tsx:142: 保存并重启先调用 api.restartProcess(processId)，随后调用 onRestart；ProcessDetailPage:395-400 传入的 onRestart=doRestart 会再次调用同一 API，导致一次用户操作连续重启两次，第二次调用还未被 await。
- **前端观测与运维页面 (52/D)** — web-ui/src/pages/TunnelsPage.tsx:513: 首次渲染 tunnels 为空时 hasStarting=false，useSingleFlightPoll 的 enabled=false 不会执行初始 tick；当前没有独立初始 load，因而不会调用 GET /tunnels，loading 会一直为 true、空态被隐藏，必须手动点击刷新才恢复。整改前曾有 useEffect(() => { load() })。
- **进程监督与日志核心 (52/D)** — src/process/manager.rs:2323: Cron 运行结束的 inline Exited 路径只切换 Sleeping、清空 PID/identity 和 log_writer，未调用 terminate_retained_process_tree；进程树 guard 已在 2508 放入 ManagedProcess，且 2455 已 preserve_on_drop，下一次运行覆盖旧 guard 时不会终止残留 descendants。
- **认证与本机系统能力 API (56/D)** — src/api/routes/ports.rs:248: Linux 的 ss -Hntlpu 输出含 users 字段时，parse_line 先按 fields.len()>=7 进入 netstat 分支，把 fields[3] 的 RecvQ 当作本地地址，extract_port 返回 None 丢弃整行；ss 成功时不会回退到 netstat，因此 Linux 监听端口会被漏报，破坏端口页及项目 WebPort 关联。
- **进程、项目与 Ecosystem API (62/C)** — src/api/routes/processes.rs:730: PATCH 更新将 req.cron、req.cwd、req.notify 和 req.log_alert 的 None 回填 existing_config；serde Option 无法区分字段省略与显式 null，因此无法清除已有 cron、CWD、通知覆盖或日志告警配置。
- **前端 API、认证与轮询层 (68/C)** — web-ui/src/hooks/useSingleFlightPoll.ts:71: enabled=false 时调度器完全跳过首次 tick；useProcesses:31 与 useProjects:30 将用户的 autoRefresh 直接作为 enabled，因此关闭自动刷新或设置加载竞态下可能没有首次进程/项目请求，只能依赖手动 reload 才获得数据。
- **构建、发布与工程文档 (72/C)** — web-ui/vite.config.ts:74: Vite 开发服务器绑定 0.0.0.0，并在 76-80 行将 /api（含 WebSocket）代理到本机 127.0.0.1:2999。LAN 客户端可访问 5173 的同源代理，导致 passwordless daemon 的本地控制面暴露；这与 docs/API.md:928、docs/CLI.md:388 的 loopback-only 假设不一致。
- **React 应用壳与导航 (76/B)** — web-ui/src/App.tsx:81: Layout 从第81行延伸到第841行（约761行），同时持有 settings/processes/projects/health/通知查询、AI/终端/统计/开发工具状态、移动端焦点陷阱与侧栏副作用，并内嵌第722-817行的完整路由表及多个 overlay；本轮虽删除 StatusBar，应用壳仍承担过高的 core 变更耦合。
- **前端设置、AI 与终端 (78/B)** — web-ui/src/components/TerminalPanel.tsx:131: TerminalPanel 当前约 1,136 行，仍在同一组件内处理 xterm 生命周期、WebSocket 建连/重试、输入与命令历史持久化、标签页/分屏、布局和快捷键；本轮虽删除 TerminalStatusBarBtn 并修正底部偏移，未见新增功能回归，但核心边界和维护回归面仍偏大。
- **CLI、守护进程与 Web 入口 (78/B)** — src/utils/pid.rs:174: capture_process_identity(...)=None 会被 is_some_and 直接解释为 daemon 不在运行；write_pid_file 随后可能删除仍存活 daemon 的 PID 文件，旧进程继续监听而新实例因端口冲突启动失败，违背身份未知时 fail-closed 的不变量。

## All findings

### HIGH (6)

- **前端进程与项目工作台** · `web-ui/src/components/EnvFileModal.tsx:142` — 保存并重启先调用 api.restartProcess(processId)，随后调用 onRestart；ProcessDetailPage:395-400 传入的 onRestart=doRestart 会再次调用同一 API，导致一次用户操作连续重启两次，第二次调用还未被 await。
- **前端观测与运维页面** · `web-ui/src/pages/TunnelsPage.tsx:513` — 首次渲染 tunnels 为空时 hasStarting=false，useSingleFlightPoll 的 enabled=false 不会执行初始 tick；当前没有独立初始 load，因而不会调用 GET /tunnels，loading 会一直为 true、空态被隐藏，必须手动点击刷新才恢复。整改前曾有 useEffect(() => { load() })。
- **认证与本机系统能力 API** · `src/api/routes/ports.rs:248` — Linux 的 ss -Hntlpu 输出含 users 字段时，parse_line 先按 fields.len()>=7 进入 netstat 分支，把 fields[3] 的 RecvQ 当作本地地址，extract_port 返回 None 丢弃整行；ss 成功时不会回退到 netstat，因此 Linux 监听端口会被漏报，破坏端口页及项目 WebPort 关联。
- **进程监督与日志核心** · `src/process/manager.rs:2323` — Cron 运行结束的 inline Exited 路径只切换 Sleeping、清空 PID/identity 和 log_writer，未调用 terminate_retained_process_tree；进程树 guard 已在 2508 放入 ManagedProcess，且 2455 已 preserve_on_drop，下一次运行覆盖旧 guard 时不会终止残留 descendants。
- **进程监督与日志核心** · `src/process/runner.rs:280` — ProcessTreeGuard::new 失败时仅 child.kill()+wait 根进程，未按 Unix process group/Windows job 清理 descendants；hooks.rs:60-65 存在同样的根进程单独清理路径，树所有权建立失败可能留下失控子进程。
- **构建、发布与工程文档** · `web-ui/vite.config.ts:74` — Vite 开发服务器绑定 0.0.0.0，并在 76-80 行将 /api（含 WebSocket）代理到本机 127.0.0.1:2999。LAN 客户端可访问 5173 的同源代理，导致 passwordless daemon 的本地控制面暴露；这与 docs/API.md:928、docs/CLI.md:388 的 loopback-only 假设不一致。

### MED (33)

- **React 应用壳与导航** · `web-ui/src/App.tsx:81` — Layout 从第81行延伸到第841行（约761行），同时持有 settings/processes/projects/health/通知查询、AI/终端/统计/开发工具状态、移动端焦点陷阱与侧栏副作用，并内嵌第722-817行的完整路由表及多个 overlay；本轮虽删除 StatusBar，应用壳仍承担过高的 core 变更耦合。
- **前端进程与项目工作台** · `web-ui/src/pages/EditPage.tsx:165` — 编辑表单把空 cron、args、cwd、notify 转成省略字段；后端 update_process 对省略字段保留旧值，因此界面留空无法真正清除已有定时任务、参数、目录或通知配置。
- **前端进程与项目工作台** · `web-ui/src/components/EnvFileModal.tsx:402` — 底部取消按钮直接执行 onClose，绕过 requestClose 的 dirty 确认与 mutationRef 保护；用户可在未保存时直接丢弃修改，保存进行中也可关闭弹窗。
- **前端进程与项目工作台** · `web-ui/src/pages/ProcessesPage.tsx:1023` — 终端动作设置 disabled: !p.cwd，但两套菜单均未处理该字段；无 cwd 时按钮仍可点击，回调仅短路且无错误提示。
- **前端进程与项目工作台** · `web-ui/src/pages/StartPage.tsx:143` — 环境文件保存期间仍允许切换和编辑，保存完成无 request/generation 校验就标记已保存；并发编辑会被错误标记为已保存，启动表单也未等待 envSaving。
- **前端进程与项目工作台** · `web-ui/src/pages/ProcessesPage.tsx:64` — ProcessesPage、ProcessDetailPage、ProjectsPage 仍集中多类职责并复制动作/视图逻辑；EnvFileModal 与 EnvFilePanel 也重复加载、编辑、保存、同步状态流程，持续存在漂移风险。
- **前端观测与运维页面** · `web-ui/src/pages/PortFinderPage.tsx:120` — 约 1,065 行页面同时承担端口扫描、进程匹配、过滤、分页、杀进程、隧道创建及多个内嵌渲染组件，仍是明显的 bloat/god-component，维护和回归风险较高。
- **前端观测与运维页面** · `web-ui/src/pages/AnalyticsPage.tsx:1042` — AnalyticsPage 与 LogVolumePage:278 各自维护相近的批量日志统计拉取、并发限制、增量 map 合并、错误处理及 single-flight 轮询流程，日志统计适配和策略变更仍需改两份。
- **前端观测与运维页面** · `web-ui/src/pages/NotifModal.tsx:10` — NotifModal:10 与 NotificationsPage:125 重复维护相同的 notification event 默认配置，新增或调整事件开关时存在漂移风险。
- **前端观测与运维页面** · `web-ui/src/pages/PortFinderPage.tsx:232` — 状态过滤只匹配 p.state.toUpperCase() === LISTENING（快速隧道按钮在 :364 也如此），但非 Windows 后端直接返回 ss/netstat 的标准状态 LISTEN；Linux 监听端口因此无法被“监听中”筛选，也不会出现隧道按钮。
- **前端观测与运维页面** · `web-ui/src/pages/LogLibraryPage.tsx:401` — 后端 list_log_dates 按 newest-first 返回日期，但这里使用 dates.slice(-3).reverse()，超过 3 天时显示最旧三天而不是最新三天；同时日期按钮在 :410 不传日期，:247 的 onView 仅导航到 /processes/:id，点击日期不会打开所选日期日志。
- **前端观测与运维页面** · `web-ui/src/pages/AnalyticsPage.tsx:681` — 上游 :129-132 将 sleeping/starting 计入 NsRow.other，但 statusLabel 只判断 crashed/running，否则固定显示“全部已停止”；纯 sleeping/starting 命名空间会被错误标记为已停止。
- **前端设置、AI 与终端** · `web-ui/src/components/TerminalPanel.tsx:131` — TerminalPanel 当前约 1,136 行，仍在同一组件内处理 xterm 生命周期、WebSocket 建连/重试、输入与命令历史持久化、标签页/分屏、布局和快捷键；本轮虽删除 TerminalStatusBarBtn 并修正底部偏移，未见新增功能回归，但核心边界和维护回归面仍偏大。
- **前端 API、认证与轮询层** · `web-ui/src/hooks/useSingleFlightPoll.ts:71` — enabled=false 时调度器完全跳过首次 tick；useProcesses:31 与 useProjects:30 将用户的 autoRefresh 直接作为 enabled，因此关闭自动刷新或设置加载竞态下可能没有首次进程/项目请求，只能依赖手动 reload 才获得数据。
- **前端 API、认证与轮询层** · `web-ui/src/hooks/useNotificationTray.ts:30` — INACTIVE 集合遗漏 stopping；当轮询观测到 running/watching → stopping → stopped 时，:86 的 ACTIVE.has(prevStatus) 条件不会成立，停止事件不会进入通知托盘。
- **前端 API、认证与轮询层** · `web-ui/src/lib/api.ts:401` — 通用 request<T> 直接将 JSON.parse 结果强制转换为 T；多个实际端点仍绕过运行时 schema，异常 2xx 响应可被当作合法成功数据继续进入页面。
- **CLI、守护进程与 Web 入口** · `src/utils/pid.rs:174` — capture_process_identity(...)=None 会被 is_some_and 直接解释为 daemon 不在运行；write_pid_file 随后可能删除仍存活 daemon 的 PID 文件，旧进程继续监听而新实例因端口冲突启动失败，违背身份未知时 fail-closed 的不变量。
- **CLI、守护进程与 Web 入口** · `src/utils/pid.rs:141` — 兼容 numeric PID 时将 start_time_secs 置零、executable 置空；daemon_record_matches_identity 会跳过启动时间并退回 current_exe。旧 PID 被复用给同路径普通 CLI 进程时可能误判为 daemon，阻塞启动/重启并削弱 PID reuse 防护。
- **进程、项目与 Ecosystem API** · `src/api/routes/processes.rs:730` — PATCH 更新将 req.cron、req.cwd、req.notify 和 req.log_alert 的 None 回填 existing_config；serde Option 无法区分字段省略与显式 null，因此无法清除已有 cron、CWD、通知覆盖或日志告警配置。
- **进程、项目与 Ecosystem API** · `src/api/routes/projects.rs:351` — 项目更新仅在 patch.web_port 为 Some 时写入；显式 null 与省略都落到 None，无法清除已有 web_port。effective_launch_uri 同样会把 None 回填旧值。
- **进程、项目与 Ecosystem API** · `src/api/routes/ecosystem.rs:84` — Ecosystem 导入中项目关联失败且 delete 清理也失败时，仅尝试 save_to_disk 并拼接错误；若持久化也失败，未设置 background_persistence_error 或明确 persistence failed，可能留下运行中但未持久化的孤儿进程。
- **进程、项目与 Ecosystem API** · `src/api/routes/ecosystem.rs:77` — 导入总时限只在每个 app 开始前检查，manager.start(app).await 未受 import_deadline/timeout 约束；单个启动阻塞时，批量接口可能超过声明的 5 分钟并长期持有 state_mutation_lock。
- **进程、项目与 Ecosystem API** · `src/api/routes/processes.rs:110` — processes.rs 仍同时承载生命周期、日志、环境变量、通知、项目关联、克隆和命名空间批量操作；补偿事务虽完善，但文件级职责和回归面仍高。
- **认证与本机系统能力 API** · `src/api/routes/ports.rs:180` — Windows netstat 返回所有连接而 Unix -l 仅返回监听 socket；API 注释和前端状态筛选宣称支持全部状态，跨平台语义不一致，且 Windows 可对非监听连接进入受管理进程停止流程。
- **进程监督与日志核心** · `src/process/manager.rs:437` — 恢复已运行进程时先将 watch 配置映射为 Watching，FileWatcher::start 失败却只记录日志并继续，留下 status=Watching 但 file_watcher=None 的假监督状态；正常 do_spawn 路径则会清理并返回错误，恢复路径语义不一致。
- **进程监督与日志核心** · `src/process/manager.rs:2183` — manager.rs 当前约 3431 行；cron_trigger_loop 在 2200-2669 重新复制 do_spawn_with_event_inner:1338 的 hooks、env、PID/tree、health 和提交流程，已与普通生命周期路径发生漂移并直接造成 Cron 树清理遗漏，维护回归面较大。
- **配置、模型与 JSON 持久化** · `src/config/telegram_checkpoint.rs:57` — save 先读取当前 offset、执行单调性校验，再独立写入；没有覆盖检查到写入的锁。并发调用可同时读到旧 offset，较小 offset 后写覆盖较大 offset，导致 Telegram 更新重复消费。atomic_file 锁只覆盖单次文件操作。
- **配置、模型与 JSON 持久化** · `src/models/project.rs:75` — ProjectPatch 的 web_port、launch_uri 等字段均为 Option；serde 会把字段缺失和显式 null 都解码为 None，模型没有 presence/clear 标记，PATCH 无法同时表达保持原值和清除值，项目端口或启动 URI 存在无法清除的状态风险。
- **AI、通知、隧道与观测集成** · `src/api/routes/ai.rs:161` — ai.rs 仍约 1495 行，集中承载 AI 设置 CRUD、GitHub Device Flow、模型发现、多个 Provider 出站请求及 SSE/NDJSON 流式解析；当前未核实到新的 P0-P2 安全或功能回归，但该 god-file 仍有明确维护风险。
- **Windows 桌面壳与托盘入口** · `desktop-shell/src/windows_app.rs:173` — 自启动初始化与托盘切换都静默丢弃错误：initialize_autostart 在 :181 仅判断 is_ok()，失败无提示；托盘 :296 用 unwrap_or(false)，:298-301 对 enable/disable 仅判断结果，失败不更新勾选也不给用户诊断。登录自启动可能未生效而界面无可操作反馈。
- **构建、发布与工程文档** · `sonar-project.properties:4` — Sonar 仅扫描 src 与 web-ui/src，完全不纳入 desktop-shell；quality.yml:190-193 只对桌面壳执行 cargo test/build，没有 fmt、clippy 或覆盖率，托盘、单实例和升级入口不受 Sonar quality gate 约束。
- **构建、发布与工程文档** · `.github/workflows/quality.yml:143` — Linux package smoke 仅执行脚本语法检查、构建 deb 和 payload grep，没有 dpkg 安装、postinst/prerm/postrm、systemd 启停或健康探针；文档宣传的 Linux 服务安装生命周期仍未被 CI 验证。
- **构建、发布与工程文档** · `.github/workflows/release.yml:234` — 正式 Windows release 的 package-installer 只编译、签名、哈希并上传安装包，没有对最终 release artifacts 执行安装、启动、升级、卸载 smoke；quality.yml 的安装测试使用另一套构建产物，不能完全证明实际发布包可用。

### LOW (6)

- **React 应用壳与导航** · `web-ui/src/components/ServerSwitcher.tsx:84` — ServerSwitcher 从第84行延伸到第704行（约621行），在单组件内同时处理本地/远程/SSH 配置、存储恢复与保存、活动服务器切换、隧道命令复制、弹层焦点和全部表单 UI；职责仍属同一领域且未见新增功能错误，但维护回归面偏大。
- **前端进程与项目工作台** · `web-ui/src/components/FolderBrowser.tsx:94` — 面向跨平台项目的面包屑始终用反斜杠拼接，Unix 路径点击后会生成错误目录。
- **前端 API、认证与轮询层** · `web-ui/src/lib/api.ts:493` — api 对象仍在约 1,342 行单文件中聚合多个领域端点；统一 transport 已存在，但按领域拆分可降低维护成本。
- **CLI、守护进程与 Web 入口** · `src/lib.rs:204` — Web 入口直接拼接 host；--host ::1 会生成缺少 IPv6 authority 方括号的 URL，浏览器启动失败。
- **认证与本机系统能力 API** · `src/api/routes/system.rs:84` — system.rs 仍同时承载健康、状态保存恢复、受限 env 文件、统计、重启和打开目录等多个系统边界。
- **认证与本机系统能力 API** · `src/config/env_file.rs:37` — env 路径已限制到注册 cwd、直接子文件并拒绝最终符号链接；canonicalize 与后续 I/O 间仍有同用户恶意替换父目录的 TOCTOU 窗口。

## Cross-cutting themes

- **P0 安全边界总体保持，但开发代理存在旁路.** 生产 daemon 的回环、鉴权、CORS 与出站策略仍有效；Vite 开发服务绑定 0.0.0.0 并代理本机 API，密码关闭时会把控制面间接暴露给局域网。
- **进程生命周期仍有未合流的 Cron 分支.** Cron 路径复制了普通 spawn/commit 流程，退出与树所有权失败时可能只处理根进程或遗留 descendants；恢复 watcher 也可能显示假 Watching 状态。
- **前端整改引入了可复现的交互回归.** 条件化 single-flight 轮询可让页面永远不做首次加载，env 保存并重启会重复请求；Linux 端口解析还会丢弃常见 ss 输出。
- **PATCH 与检查点的一致性边界仍不完整.** 进程和项目补丁无法区分字段缺失与显式清空，Telegram checkpoint 的单调性检查与写入没有同一把锁，极端并发下可能回退。
- **桌面壳已成为正式入口但质量门禁未完全覆盖.** 桌面壳有独立测试和 Windows 构建 smoke，但未进入 Sonar、覆盖率与独立 clippy/fmt 门禁；Linux 包和正式 release artifact 的安装生命周期也缺少端到端验证。
- **停止大范围洁癖重构，优先关闭少量运行语义缺陷.** 单体 Rust daemon + React 控制台仍适合产品规模；大文件和重复代码只在已经造成具体缺陷的生命周期与轮询路径上做窄修，不建议全面拆分或引入微服务。
