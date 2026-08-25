<!--
  This file:        .codemap/codemap.md   (written report)
  Interactive map:  .codemap/codemap.html
-->

# RunDock / Alter — Functional Module Quality Audit

> **Interactive view:** [`.codemap/codemap.html`](codemap.html) — per-module scores, findings, LoC, and the dependency graph. This file is the written report.

**Generated:** 2026-08-25 · **Modules:** 12 · **Size:** 50646 tracked LoC across 208 files

## Health by layer

| Layer | Modules | Avg score |
|---|--:|--:|
| 前端 · 应用壳 | 1 | 52 |
| 前端 · 功能页面 | 3 | 52 |
| 前端 · 数据访问 | 1 | 52 |
| 后端 · API 与系统边界 | 3 | 39 |
| 后端 · 进程运行核心 | 1 | 48 |
| 后端 · 状态与集成 | 2 | 42 |
| 交付 · 构建与文档 | 1 | 38 |

## Per-module lines of code & score

_LoC is the representative file/folder per module; folder-level modules overlap and are not additive._

### 前端 · 应用壳

| Module | LoC | Score | Tags |
|---|--:|:--|:--|
| React 应用壳与导航 | 3,518 | 52 D | god-component, bloat, silent-except, duplication, legacy, fallback |

### 前端 · 功能页面

| Module | LoC | Score | Tags |
|---|--:|:--|:--|
| 前端进程与项目工作台 | 6,120 | 48 D | bloat, god-component, duplication, fallback, silent-except, any-escape |
| 前端设置、AI 与终端 | 4,257 | 55 D | fallback, silent-except, bloat, god-component, glue |
| 前端观测与运维页面 | 3,588 | 54 D | fallback, silent-except, bloat, god-component, duplication, any-escape |

### 前端 · 数据访问

| Module | LoC | Score | Tags |
|---|--:|:--|:--|
| 前端 API、认证与轮询层 | 1,916 | 52 D | any-escape, fallback, silent-except, bloat, god-component, duplication, over-fit, legacy, dual-format |

### 后端 · API 与系统边界

| Module | LoC | Score | Tags |
|---|--:|:--|:--|
| 认证与本机系统能力 API | 2,642 | 30 F | any-escape, fallback, silent-except, stub, duplication, bloat |
| CLI、守护进程与 Web 入口 | 1,587 | 38 F | any-escape, fallback, silent-except, fake-output, legacy, duplication, glue |
| 进程、项目与 Ecosystem API | 1,329 | 49 D | any-escape, fallback, silent-except, bloat, god-component, duplication, glue, over-fit |

### 后端 · 进程运行核心

| Module | LoC | Score | Tags |
|---|--:|:--|:--|
| 进程监督与日志核心 | 3,012 | 48 D | god-component, bloat, duplication, silent-except, fallback, stub |

### 后端 · 状态与集成

| Module | LoC | Score | Tags |
|---|--:|:--|:--|
| AI、通知、隧道与观测集成 | 4,036 | 35 F | any-escape, fallback, silent-except, fake-output, duplication, bloat, god-component, glue |
| 配置、模型与 JSON 持久化 | 2,065 | 50 D | fallback, silent-except, dual-format, legacy, duplication, bloat, stub, over-fit |

### 交付 · 构建与文档

| Module | LoC | Score | Tags |
|---|--:|:--|:--|
| 构建、发布与工程文档 | 16,576 | 38 F | dual-format, fallback, silent-except, legacy, duplication, placeholder, glue, any-escape |

## Worst offenders

- **认证与本机系统能力 API (30/F)** — src/api/middleware.rs:32: 未配置 Web 密码时直接执行 next.run(req)，完全跳过 token 校验；src/api/mod.rs:13-34 将 system、scripts、ports、terminals、update、git 等高权限路由全部置于该中间件后，非回环暴露时可形成未授权本机能力访问。
- **AI、通知、隧道与观测集成 (35/F)** — src/api/routes/ai.rs:460: openai_base_url/ollama_base_url 可由设置请求写入（:125-130），list_openai_models 随后将 API Key 发送到该 URL 的 /models（:462-465）；chat 也将 openai/ollama 请求发送到配置地址（:569-574），无协议、主机或内网地址限制，存在凭据外泄与 SSRF。
- **CLI、守护进程与 Web 入口 (38/F)** — src/client/daemon_client.rs:16: CLI 从本地 auth 配置读取 master token，并在 :18-25 注入到 http://{host}:{port}；host 可由 CLI/ALTER_HOST 任意指定（src/cli/args.rs:13-19），因此连接非本机 daemon 时会通过明文 HTTP 发送高权限凭据。
- **构建、发布与工程文档 (38/F)** — .github/workflows/release-linux.yml:271: APT 仓库签名被明确设计为可选；APT_GPG_KEY 缺失时 :275-277 直接跳过 GPG 并继续 :294-301 提交和推送 gh-pages，最终可发布未签名的软件包，破坏发行物真实性校验。
- **前端进程与项目工作台 (48/D)** — web-ui/src/pages/ProcessDetailPage.tsx:46: 首次 getProcess 失败在 :50 直接静默吞掉；process 保持 null，渲染分支 :154 永远显示加载中，没有错误态或重试入口。进程详情页在核心请求失败时会卡在伪加载状态。
- **进程监督与日志核心 (48/D)** — src/process/manager.rs:829: watch 模式创建 FileWatcher 后立即丢弃返回值；FileWatcher 仅持有 RecommendedWatcher（src/process/watcher.rs:11），start 返回 Self（src/process/watcher.rs:62），因此返回后 watcher 被释放，watch_paths 不会持续触发重启；同时启动错误被 let _ 丢弃。
- **进程、项目与 Ecosystem API (49/D)** — src/api/routes/processes.rs:47: is_env_filename 只检查 .env 前后缀（47-50），未拒绝路径分隔符；../outside.env 或 Windows 下 ..\outside.env 会通过校验，随后在 499、526 直接与 cwd join，GET 可越界读取、PUT 可越界覆盖工作目录外文件。
- **配置、模型与 JSON 持久化 (50/D)** — src/daemon/state.rs:206: 恢复 cron 进程时，stale PID 存活但 kill_orphan_pid 失败只记录 warn（206-212），随后仍在 215-218 重新注册 Sleeping 和调度器；旧进程可能继续运行并与新调度实例重复执行。
- **React 应用壳与导航 (52/D)** — web-ui/src/App.tsx:84-702: App.tsx 共 2,516 行；Layout 同时承担进程/项目轮询、通知托盘、AI 面板、终端面板、侧栏筛选、保存/关闭守护进程、全部 React Router 路由和 StatusBar 编排。其后还在同一文件内承载约 500 行 ServerSwitcher（:1728-2265）和 AuthGuard（:2348-2455）。根壳协调全局状态和导航是合理的，但把连接配置、认证锁屏和大量功能细节集中在单文件中，职责泛化不适当，已构成真实维护风险。
- **前端 API、认证与轮询层 (52/D)** — web-ui/src/lib/servers.ts:76: direct 远程连接固定构造 http:// URL（70-76），而统一请求会附加 Bearer token（web-ui/src/lib/api.ts:64-76）；连接非本机 daemon 时凭据可能明文传输。SSH 模式仅因本地转发到 127.0.0.1 而例外。

## All findings

### HIGH (26)

- **React 应用壳与导航** · `web-ui/src/App.tsx:84-702` — App.tsx 共 2,516 行；Layout 同时承担进程/项目轮询、通知托盘、AI 面板、终端面板、侧栏筛选、保存/关闭守护进程、全部 React Router 路由和 StatusBar 编排。其后还在同一文件内承载约 500 行 ServerSwitcher（:1728-2265）和 AuthGuard（:2348-2455）。根壳协调全局状态和导航是合理的，但把连接配置、认证锁屏和大量功能细节集中在单文件中，职责泛化不适当，已构成真实维护风险。
- **前端进程与项目工作台** · `web-ui/src/pages/ProcessDetailPage.tsx:46` — 首次 getProcess 失败在 :50 直接静默吞掉；process 保持 null，渲染分支 :154 永远显示加载中，没有错误态或重试入口。进程详情页在核心请求失败时会卡在伪加载状态。
- **前端观测与运维页面** · `web-ui/src/pages/AnalyticsPage.tsx:457` — 每个进程的 getLogStats 失败在 :458 被转换为 [processId, []]，没有错误状态；LogVolumePage 同样在 :217-224 转为空 buckets，LogLibraryPage 在 :37-47 转为空日志元数据。运维图表会把后端故障显示成无日志或零值，无法区分真实零数据与 API 失败，属于危险的观测降级。
- **前端设置、AI 与终端** · `web-ui/src/components/settings/GeneralTab.tsx:72` — 更新调用被 catch(() => {}) 吞掉；随后第74-79行只轮询健康状态，旧守护进程仍健康时会被标记为更新成功并 reload，无法区分更新失败与服务仍在线，产生假成功。
- **前端 API、认证与轮询层** · `web-ui/src/lib/servers.ts:76` — direct 远程连接固定构造 http:// URL（70-76），而统一请求会附加 Bearer token（web-ui/src/lib/api.ts:64-76）；连接非本机 daemon 时凭据可能明文传输。SSH 模式仅因本地转发到 127.0.0.1 而例外。
- **CLI、守护进程与 Web 入口** · `src/client/daemon_client.rs:16` — CLI 从本地 auth 配置读取 master token，并在 :18-25 注入到 http://{host}:{port}；host 可由 CLI/ALTER_HOST 任意指定（src/cli/args.rs:13-19），因此连接非本机 daemon 时会通过明文 HTTP 发送高权限凭据。
- **CLI、守护进程与 Web 入口** · `src/daemon/server.rs:15` — HTTP 服务允许任意 Origin、方法和请求头（:15-18），同时绑定地址完全由配置 host 决定（:26-28）；非回环部署时入口层不提供来源约束，安全性完全依赖下游认证配置。
- **CLI、守护进程与 Web 入口** · `src/utils/pid.rs:5` — PID 文件通过普通 std::fs::write 覆盖（:5-8），daemon 启动时直接调用（src/daemon/mod.rs:21-22），没有排他创建、锁、PID 身份校验或已有实例仲裁；并发启动可互相覆盖 PID 文件，停止/退出时还可能删除其他实例的 PID 文件。
- **进程、项目与 Ecosystem API** · `src/api/routes/processes.rs:47` — is_env_filename 只检查 .env 前后缀（47-50），未拒绝路径分隔符；../outside.env 或 Windows 下 ..\outside.env 会通过校验，随后在 499、526 直接与 cwd join，GET 可越界读取、PUT 可越界覆盖工作目录外文件。
- **认证与本机系统能力 API** · `src/api/middleware.rs:32` — 未配置 Web 密码时直接执行 next.run(req)，完全跳过 token 校验；src/api/mod.rs:13-34 将 system、scripts、ports、terminals、update、git 等高权限路由全部置于该中间件后，非回环暴露时可形成未授权本机能力访问。
- **认证与本机系统能力 API** · `src/api/routes/system.rs:196` — read-env 接受请求提供的任意绝对或相对路径并直接 read_to_string；write-env 在 :217-220 对任意 path 直接 tokio::fs::write，未做根目录边界、规范化、文件类型或大小校验。
- **认证与本机系统能力 API** · `src/api/routes/system.rs:229` — sync-env 接受任意 source_path，读取其内容并枚举同目录 env 文件后直接写回 :263-300；源文件路径和目标目录均无允许范围约束。
- **认证与本机系统能力 API** · `src/api/routes/terminal.rs:62` — WebSocket query 的 cwd 原样传入 PTY；:123-143 启动 powershell.exe 或 SHELL 指定的 shell 并设置该 cwd。结合全局 passwordless 放行可提供远程交互式本机 Shell。
- **认证与本机系统能力 API** · `src/api/routes/scripts.rs:123` — save_script 将请求 content 直接写入脚本目录；run_script 在 :260-318 按扩展名调用 powershell/cmd/bash/python 等解释器执行。该任意代码执行能力只依赖可绕过的全局认证。
- **认证与本机系统能力 API** · `src/api/routes/update.rs:151` — 升级 URL 仅使用 starts_with(https://github.com/) 检查，未固定仓库/资产、校验哈希或签名；:224-240 无下载大小限制，随后 :175-192 或 :198-219 执行安装器、替换当前二进制并重启。
- **认证与本机系统能力 API** · `src/api/routes/ports.rs:244` — kill_port_process 只拒绝 PID 0；:248-275 可对任意存在 PID 调用 sysinfo Process::kill，未验证进程归属、命令行或进程树。
- **进程监督与日志核心** · `src/process/manager.rs:829` — watch 模式创建 FileWatcher 后立即丢弃返回值；FileWatcher 仅持有 RecommendedWatcher（src/process/watcher.rs:11），start 返回 Self（src/process/watcher.rs:62），因此返回后 watcher 被释放，watch_paths 不会持续触发重启；同时启动错误被 let _ 丢弃。
- **进程监督与日志核心** · `src/process/manager.rs:1102` — 每个 cron tick 都重新 LogWriter::new 并覆盖 proc.log_writer；LogWriter 在 src/logging/writer.rs:75 订阅同一 broadcast，且只保存 JoinHandle（src/logging/writer.rs:57）而无 Drop/abort。旧任务会继续接收后续日志，导致重复写入、任务和文件句柄累积；普通重启路径也在 src/process/manager.rs:660、897 重复创建。
- **配置、模型与 JSON 持久化** · `src/daemon/state.rs:206` — 恢复 cron 进程时，stale PID 存活但 kill_orphan_pid 失败只记录 warn（206-212），随后仍在 215-218 重新注册 Sleeping 和调度器；旧进程可能继续运行并与新调度实例重复执行。
- **AI、通知、隧道与观测集成** · `src/api/routes/ai.rs:460` — openai_base_url/ollama_base_url 可由设置请求写入（:125-130），list_openai_models 随后将 API Key 发送到该 URL 的 /models（:462-465）；chat 也将 openai/ollama 请求发送到配置地址（:569-574），无协议、主机或内网地址限制，存在凭据外泄与 SSRF。
- **AI、通知、隧道与观测集成** · `src/api/routes/ai.rs:809` — build_system_prompt 将进程名称、命令、cwd 和最近日志原样拼入发送给外部 AI Provider 的 system prompt（:809-832）；日志是非信任输入，可能包含密钥或提示注入内容，未见脱敏、长度上限或出站数据边界。
- **AI、通知、隧道与观测集成** · `src/telegram/bot.rs:147` — 白名单判断只在 allowed_chat_ids 非空时生效；列表为空时所有 Telegram chat/user 都继续到 :163-173 的命令分发，可调用 /start、/stop、/restart 等进程控制命令。
- **AI、通知、隧道与观测集成** · `src/api/routes/notifications.rs:78` — test_notification 接受调用方提供的完整 NotificationConfig 并强制触发事件（:92-106,143-145）；sender.rs 对其中 webhook/slack/teams/discord URL 直接 POST（src/notifications/sender.rs:223-240），无 URL/目标网段限制，可形成 SSRF 或外部消息滥发。
- **AI、通知、隧道与观测集成** · `src/api/routes/notifications.rs:25` — GET notifications 直接返回完整 NotificationsStore（:26-30），未做字段脱敏；配置对象包含 webhook、Slack、Teams、Discord 地址/凭据字段，造成集成秘密泄露。
- **构建、发布与工程文档** · `.github/workflows/release-linux.yml:271` — APT 仓库签名被明确设计为可选；APT_GPG_KEY 缺失时 :275-277 直接跳过 GPG 并继续 :294-301 提交和推送 gh-pages，最终可发布未签名的软件包，破坏发行物真实性校验。
- **构建、发布与工程文档** · `.github/workflows/release-linux.yml:12` — 整个 Linux workflow 默认授予 contents: write、pages: write、id-token: write（:12-15），权限覆盖构建、打包和发布相关 job；构建阶段还执行 npm/cargo 依赖脚本，受污染的构建依赖可获得超出构建所需的发布权限。

### MED (97)

- **React 应用壳与导航** · `web-ui/src/App.tsx:208-218` — handleSave 与 handleShutdown 对 API 错误使用 catch(() => {})；保存失败后仍无条件弹出“状态已保存”（:209-210），关闭失败也没有用户反馈（:218），导致界面成功语义与真实守护进程状态脱节。
- **React 应用壳与导航** · `web-ui/src/App.tsx:2397-2415` — 认证锁屏配置 authStatus() 失败时在 :2409 直接静默吞掉异常，lockConfig 保留默认值；用户无法知道 PIN/超时配置未刷新，且后续锁屏行为可能继续使用过期配置。
- **前端进程与项目工作台** · `web-ui/src/pages/ProjectsPage.tsx:40` — 该页面约 995 行，单一组件同时负责筛选/分组/统计、端口轮询、项目启停重启、启停用、备注和分类编辑、成员列表、项目详情 inspector 及桌面/技术组件两套渲染。页面工作台职责虽应协调这些流程，但当前单组件范围已明显超出适当泛化，形成维护性 bloat/god-component。
- **前端进程与项目工作台** · `web-ui/src/pages/ProcessDetailPage.tsx:20` — 该页面约 907 行；同一组件包含进程轮询、日期日志加载、5 分钟日志统计轮询、60 秒指标轮询、SSE 生命周期、进程控制、Git pull、终端/文件夹/VS Code 操作、.env 模态框和 CPU/内存/日志图表（:46-152、:170-228、:632-907），职责边界过宽。
- **前端进程与项目工作台** · `web-ui/src/pages/ProcessesPage.tsx:372` — 卡片视图 ProcessCard（:372-515）与表格视图 ProcessRow（:534-645）分别复制 stop/start/restart/delete/clone/toggleEnabled 操作、通知检测、二级 action 列表和按钮渲染；同一进程行为需要在两套视图同步维护，属于实质 duplication。
- **前端进程与项目工作台** · `web-ui/src/components/EnvFilePanel.tsx:18` — EnvFilePanel（:18-165）与 EnvFileModal（:17-162）重复 envFileColor/envFileBg、文件列表加载、文件读取、切换、保存、重启和同步状态机；StartPage 也再次复制颜色映射（web-ui/src/pages/StartPage.tsx:18-38），形成多个 env 编辑实现。
- **前端进程与项目工作台** · `web-ui/src/components/EnvFilePanel.tsx:66` — listEnvFiles 失败时 :74-79 人工写入 [{name:.env,path:空}] 并继续 loadFile(.env)；EnvFileModal 同样在 :59-72 执行。真实文件列表不可用被静默降级成伪 .env 条目，后续读取/保存/同步可能只能失败或误导用户。
- **前端进程与项目工作台** · `web-ui/src/pages/StartPage.tsx:113` — saveEnvFile 的 writeEnvFile 异常在 :122 以空 catch 忽略，未设置 error 或失败状态；用户看不到写入失败。该页面同时在 env 列表/检查失败路径 :87-104 多处只做降级或清空状态。
- **前端进程与项目工作台** · `web-ui/src/pages/ProcessesPage.tsx:41` — UI 设置、端口轮询和批量/单进程操作大量使用空 catch（:45、:103、:146-153、:384-398、:543-560），API 失败后仍定时 reload 或保持旧列表，没有统一错误反馈，控制操作的失败与没有变化不可区分。
- **前端进程与项目工作台** · `web-ui/src/pages/CreateCronJobPage.tsx:162` — SSE message 使用 JSON.parse 后直接把 data.stream/data.content 强制断言为目标类型（:170-173），没有运行时字段校验；解析异常在 :175 直接忽略。外部事件数据跨越类型边界时可能把无效值写入运行输出。
- **前端观测与运维页面** · `web-ui/src/pages/PortFinderPage.tsx:59` — PortFinderPage 约 677 行，主组件同时管理端口加载、过滤/分组、PID 终止确认、隧道创建、表格渲染，并在组件内部定义 Th/Td/PortRow（:187-359）；单个页面承载过多状态和职责，形成 god-component/bloat。
- **前端观测与运维页面** · `web-ui/src/pages/LogVolumePage.tsx:203` — LogVolumePage 约 540 行，同时实现全量日志 API 轮询、聚合/排名/缩放计算、筛选、SVG 图表和进程卡片；AnalyticsPage 又在 :449-524 实现相似的单进程日志统计轮询和图表，职责与数据流重复。
- **前端观测与运维页面** · `web-ui/src/pages/NotificationsPage.tsx:92` — NotificationsPage 的 NotifCard 在 :98-111 和 :201-310 手工实现四种通知渠道字段；NotifModal 的 ChannelFields 在 web-ui/src/components/NotifModal.tsx:29-114 另建一套字段数组和更新器，defaultConfig 也与 :9-10 的 defaultNotifConfig 分开维护，通知配置 UI 存在明显 duplication 和潜在语义漂移。
- **前端观测与运维页面** · `web-ui/src/components/NotifModal.tsx:250` — NsNotifModal 用 useState 懒初始化函数执行 api.getNotifications() 副作用（:250-255），不是 effect；请求失败只 setLoading(false)，保留默认 NotificationConfig 且不显示错误，已有命名空间配置可能被静默显示为默认值。
- **前端观测与运维页面** · `web-ui/src/pages/PortFinderPage.tsx:118` — handleTunnel 创建隧道失败在 :126-128 静默忽略；注释声称由 TunnelsPage 显示错误，但 navigate(/tunnels) 只在成功路径 :122-125 执行，失败时当前页面没有错误反馈。
- **前端观测与运维页面** · `web-ui/src/pages/TunnelsPage.tsx:60` — CopyBtn 在 :64 忽略 clipboard.writeText 失败，却立即 setCopied(true) 并展示复制成功状态；剪贴板权限或系统调用失败时 UI 会产生错误成功反馈。
- **前端观测与运维页面** · `web-ui/src/pages/TunnelsPage.tsx:284` — 停止或移除隧道操作在 :285-291 对 API 异常使用空 catch，随后无条件静默刷新；用户无法知道 stop/remove 是否失败。AnalyticsPage 的批量启动/重启也在 :26-35 使用同类吞错。
- **前端设置、AI 与终端** · `web-ui/src/components/settings/GeneralTab.tsx:36` — 重启失败被忽略后仍继续健康检查；第42行可能仅因旧服务健康就显示已恢复连接，隐藏真实重启错误。
- **前端设置、AI 与终端** · `web-ui/src/components/settings/GeneralTab.tsx:28` — 系统路径加载失败静默处理，sysPaths 保持 null，渲染处第191、197行会永久显示加载中，用户没有失败反馈。
- **前端设置、AI 与终端** · `web-ui/src/components/settings/AiTab.tsx:131` — 异常分支注释声称使用 fallback list，但实际没有设置任何 fallback；切换 Provider 时第139行先清空 aiModel，失败后第501行可能渲染空模型选项。
- **前端设置、AI 与终端** · `web-ui/src/components/settings/TelegramTab.tsx:78` — 新令牌写入失败被忽略，随后第81-84行查询已持久化的令牌并清空输入；旧令牌仍有效时会误显示验证成功并丢失用户刚输入的新令牌。
- **前端设置、AI 与终端** · `web-ui/src/components/settings/TunnelsTab.tsx:36` — 隧道配置加载失败静默回落到第10-15行的 Cloudflare/null 默认状态，没有错误提示；用户可能误认为远端配置为空并覆盖保存。
- **前端设置、AI 与终端** · `web-ui/src/components/AiPanel.tsx:167` — 流式请求只在 clearChat 中 abort，关闭或卸载面板没有 cleanup；第187-200行的旧流仍可继续回调并更新状态，关闭后重开或新会话时存在串流和生命周期泄漏风险。
- **前端设置、AI 与终端** · `web-ui/src/components/TerminalPanel.tsx:101` — 单一 721 行组件同时承担标签页、拆分窗格、xterm 初始化、WebSocket/PTy 协议、输入解析、历史持久化、ResizeObserver、布局和完整渲染（约第105-677行），职责虽同属终端域但已形成明显维护热点。
- **前端 API、认证与轮询层** · `web-ui/src/lib/api.ts:233` — EventSource 将 session token 放入 URL 查询参数（streamLogs 231-234、runScript 250-253、streamInstallProvider 549-552）；Bearer token 会进入服务器访问日志、代理日志、浏览器 URL/referrer 等位置。
- **前端 API、认证与轮询层** · `web-ui/src/hooks/useProcesses.ts:27` — useProcesses 用 setInterval(load, intervalMs) 调度异步请求，没有 in-flight 或 AbortController 保护；daemon 响应变慢时会产生重叠轮询，旧响应可能覆盖新状态。useProjects:25 与 useDaemonHealth:24 采用相同模式。
- **前端 API、认证与轮询层** · `web-ui/src/lib/settings.ts:80` — loadSettings 对所有非 2xx 与网络异常直接返回 DEFAULT_SETTINGS（80、83-85），saveSettings 只 await fetch 但不检查 response.ok（89-96）；认证失败或服务端保存失败会被 UI 当作默认值或成功而静默吞掉。
- **前端 API、认证与轮询层** · `web-ui/src/lib/servers.ts:65` — getActiveServer 在活动 ID 不存在时静默回退 LOCAL_SERVER（65-66）；结合 getServers 对 JSON 仅做类型断言且解析失败返回空数组（37-43），过期或损坏的远程选择可能把后续写操作路由到本地 daemon。
- **前端 API、认证与轮询层** · `web-ui/src/lib/api.ts:76` — request 将任意 RequestInit.headers 强制断言为 Record<string,string>，并把未校验的 response.json() 直接断言为 Promise<T>（76、88）；WebAuthn 接口进一步以 Promise<object> 返回并在 auth.ts:44、63 使用 any，服务端字段漂移只能在运行时暴露。
- **前端 API、认证与轮询层** · `web-ui/src/lib/api.ts:379` — AI SSE 解析遇到 JSON 或协议行错误时直接 catch 后忽略（374-380），流结束仍无条件调用 onDone（381-386）；截断或格式损坏的响应可能被呈现为成功的部分输出。
- **前端 API、认证与轮询层** · `web-ui/src/lib/api.ts:92` — 单个 api 对象从 92 延伸至 572 行，集中进程、项目、日志、脚本、通知、系统、AI、认证、Telegram、隧道和终端等职责；settings.ts:66-97 又复制一套 fetch/auth 逻辑并采用不同错误策略，形成 bloat 与 transport duplication。
- **前端 API、认证与轮询层** · `web-ui/src/hooks/useSettings.ts:20` — updateSettings 在 React setState updater 内直接调用异步 saveSettings（20-25）；快速连续 patch 会启动无序写请求，后完成的旧请求可能覆盖新设置，且副作用嵌入状态计算。
- **CLI、守护进程与 Web 入口** · `src/daemon/mod.rs:64` — 状态加载失败被 if let Ok 静默忽略，随后继续恢复信号、启动 Telegram 和 HTTP 服务（:63-80）；持久化损坏或读取错误会以空状态继续运行，调用方看不到明确的恢复失败。
- **CLI、守护进程与 Web 入口** · `src/cli/commands/daemon.rs:62` — 后台 daemon 启动后只轮询 TCP connect，连接成功即打印 started（:57-64），没有等待 HTTP health、路由可用或初始化完成；端口已占用但服务未就绪时可能误报成功。
- **CLI、守护进程与 Web 入口** · `src/cli/commands/daemon.rs:130` — is_daemon_alive 发送 health 请求后只检查返回字节是否以 HTTP/ 开头（:129-137），不校验状态码、响应路径或服务标识；任意占用目标端口的 HTTP 服务都可能被判定为 Alter daemon。
- **CLI、守护进程与 Web 入口** · `src/cli/commands/daemon.rs:77` — stop_daemon 忽略 shutdown POST 的 Result 后直接打印 daemon stopped（:72-79）；网络错误、鉴权失败或服务拒绝时 CLI 仍会给出成功结论。
- **CLI、守护进程与 Web 入口** · `src/cli/commands/daemon.rs:85` — restart_daemon 忽略 shutdown 结果，仅等待最多 3 秒后调用 start_daemon（:84-97）；start_daemon 又只验证 TCP，旧进程未真正退出或新进程未完成初始化时仍可能报告 restarted。
- **CLI、守护进程与 Web 入口** · `src/daemon/signals.rs:6` — 信号处理器保存一次状态后直接 std::process::exit(0)（:7-15），没有通知 Axum server 优雅停止、等待请求/后台任务收尾或确认子任务已退出；保存成功不等于运行时资源已清理。
- **CLI、守护进程与 Web 入口** · `src/client/daemon_client.rs:74` — SSE 日志流把所有 chunk 追加到 buf，只有遇到换行才截断（:78-91），没有单行或累计缓冲上限；服务端持续发送无换行数据时客户端内存可无界增长。
- **CLI、守护进程与 Web 入口** · `src/utils/pid.rs:30` — Windows process_exists 通过 tasklist 输出 contains(pid.to_string()) 判断进程存在（:30-38），PID 12 可能匹配 112 等字符串；Unix 分支仅检查 /proc/{pid} 是否存在（:40-44），两者都未验证进程命令行和身份。
- **CLI、守护进程与 Web 入口** · `src/daemon/mod.rs:43` — daemon tracing 使用 rolling::never 创建固定 daemon.log（:45-56），本模块未设置大小、日期轮转或保留上限；长期运行日志可无限增长，且 tracing 初始化错误路径没有降级诊断。
- **进程、项目与 Ecosystem API** · `src/api/routes/processes.rs:338` — PATCH /processes/:id 用 StartRequest 重建完整 AppConfig，但省略字段会被重置：args/env/watch 等使用 unwrap_or_default，log_file、error_file、env_file、health_check、hooks 直接置 None/默认（338-369），而非保留 existing 配置，部分更新会破坏未提交字段。
- **进程、项目与 Ecosystem API** · `src/api/routes/processes.rs:121` — 多数生命周期路由在返回成功前仅 tokio::spawn detached save_to_disk，并把失败降为 warn（121、142、153、168、179、204、373）；请求成功不代表状态已持久化，快速重启或 daemon 退出可能丢失最新状态。
- **进程、项目与 Ecosystem API** · `src/api/routes/processes.rs:249` — get_logs 对 read_merged_logs/read_merged_logs_for_date 错误统一 unwrap_or_default（248-250），get_log_dates 也在 271-272 静默返回空日期；磁盘读取失败会被客户端误显示为空日志。
- **进程、项目与 Ecosystem API** · `src/api/routes/processes.rs:242` — 日志行数只解析为任意 usize，没有上限或拒绝异常值；结合底层尾读实现需要扫描完整日志，攻击者可用超大 lines 参数放大磁盘扫描和内存占用。
- **进程、项目与 Ecosystem API** · `src/api/routes/processes.rs:110` — start_process 已启动进程后才同步 save_projects（119-120）；保存失败会把请求返回为错误，但没有停止或回滚已运行的进程，调用方会看到失败而运行态已发生改变。
- **进程、项目与 Ecosystem API** · `src/api/routes/projects.rs:323` — update_project 先修改 projects store（323-347），再逐个 set_enabled（349-357）；任何成员更新失败都会直接返回，内存中的项目元数据已变更却没有事务回滚，项目与进程状态可能分裂。
- **进程、项目与 Ecosystem API** · `src/api/routes/ecosystem.rs:30` — load_ecosystem 逐个启动配置中的 app 并返回 started/errors（30-41），但该路由没有调用 save_to_disk、save_projects 或确保项目元数据；通过生态配置启动的进程在 daemon 重启后可能不被恢复。
- **进程、项目与 Ecosystem API** · `src/api/routes/processes.rs:627` — clone_process 先快照 existing_names 再选择唯一名称（627-645），没有保留名或原子占用；并发 clone 请求可以选择相同名称。克隆同时主动丢弃 log_file、error_file、env_file 和 cron 时间字段（663-671），语义边界未明确。
- **认证与本机系统能力 API** · `src/api/routes/auth.rs:104` — 密码登录和 :130-147 PIN 登录仅校验凭据并创建 session，没有失败次数、速率限制或临时锁定；4/6 位 PIN 尤其易受在线猜测。
- **认证与本机系统能力 API** · `src/api/routes/auth.rs:285` — 每次登录都向 DashMap 插入 24 小时 session；:303-305 只在被查询时检查过期，没有全局清理或数量上限，可被大量登录请求持续膨胀。
- **认证与本机系统能力 API** · `src/api/middleware.rs:67` — 为 EventSource 支持将 token 放入 URI query；:77-82 原样提取 token，认证凭据可能进入访问日志、代理日志、浏览器历史或 Referer。
- **认证与本机系统能力 API** · `src/api/routes/scripts.rs:361` — 脚本 child 被移动到独立 tokio::spawn 等待任务；SSE 事件流断开时没有取消该任务或显式终止 child，且执行没有运行时上限，可能积累后台进程。
- **认证与本机系统能力 API** · `src/api/routes/terminal.rs:221` — WebSocket 输入消息直接转为字节并发送给 PTY（:223-244），没有单帧大小、总输入量、会话数或空闲超时限制；终端会话 registry 也没有容量控制。
- **认证与本机系统能力 API** · `src/api/routes/git.rs:161` — git_pull 在进程 cwd 执行 git pull，并按仓库文件自动执行 npm/yarn/pnpm/pip/cargo/go 安装或构建（:164-179），未设超时、并发锁或输出上限；仓库脚本可产生额外执行副作用。
- **认证与本机系统能力 API** · `src/api/routes/terminal_history.rs:49` — history key 和 CmdEntry.cmd 均无长度或字符限制；虽然单 key 截断 150 条（:60-61），但整体 HashMap 无数量上限，且每次请求完整读写文件（:25-40），并发更新可能丢失数据。
- **认证与本机系统能力 API** · `src/api/routes/ui_settings.rs:46` — UI settings 接受并返回任意 serde_json::Value（:47-56），没有 schema、深度或大小限制；:32 直接覆盖文件，异常/并发时缺少原子写入。
- **认证与本机系统能力 API** · `src/api/error.rs:57` — From<anyhow::Error> 直接将 e.to_string() 放入 ApiError；内部 OS 错误、路径、命令或文件信息可原样返回客户端，扩大诊断信息泄露。
- **进程监督与日志核心** · `src/logging/rotation.rs:22` — (1..max_files).rev() 使 i+1 > max_files 的删除分支（src/logging/rotation.rs:26）永远不可达，.max_files 文件不会被删除；Windows 下已有目标文件时 rename 还可能失败，导致轮转写入报错。
- **进程监督与日志核心** · `src/logging/writer.rs:86` — 写入任务用 if let Ok 和 let _ 丢弃 write_line 错误；午夜轮转在 src/logging/writer.rs:105、110 也丢弃 rotate_by_date/reopen 错误，日志可能静默停止或未轮转。
- **进程监督与日志核心** · `src/process/manager.rs:687` — 所有 spawn 路径把 env_file::merge_env 错误静默回退到 config.env（同样位于 src/process/manager.rs:910、1184），缺失或损坏的 env 文件无法被调用方区分，进程可能带着不完整环境启动。
- **进程监督与日志核心** · `src/process/manager.rs:487` — 批量 start/stop/restart 只保留成功项（src/process/manager.rs:487、515、547），单个失败没有错误结果或日志，部分生命周期失败会表现为正常返回的部分列表。
- **进程监督与日志核心** · `src/process/manager.rs:846` — ProcessManager 同时承担 registry、重启、cron、metrics、log-alert 等职责（构造函数在 src/process/manager.rs:67-98 启动四个后台循环，文件约 1477 行）。restart_loop 的重复 spawn 分支（src/process/manager.rs:864-988）未复用 do_spawn 的 pre_start、post_start、health-check、watcher 路径（src/process/manager.rs:671-840），崩溃自动重启不会恢复完整生命周期。
- **进程监督与日志核心** · `src/logging/reader.rs:105` — read_log_stats_today 使用 map_while(Result::ok)，BufRead 错误会被丢弃并继续返回 Ok，损坏或读取中断的日志会产生低估统计且无告警。
- **配置、模型与 JSON 持久化** · `src/daemon/state.rs:262` — saved_app_from_snapshot 持久化 restart_count 和 autorestart_on_restore，但 restore 本身从未读取或传递这两个字段（恢复调用仅传 id/config/PID/cron history，217、226、238、246、250）；重启 daemon 后重启计数可能归零，autorestart_on_restore 成为无效状态字段。
- **配置、模型与 JSON 持久化** · `src/daemon/state.rs:135` — save_to_disk 每次使用固定 state.json.tmp，未见并发写入协调；rename 失败后删除临时文件并直接覆盖目标（137-140），既可能发生并发保存互相覆盖，也失去崩溃时的原子替换保证。
- **配置、模型与 JSON 持久化** · `src/config/project_store.rs:33` — ProjectStore 对读取或 JSON 解析错误统一 ok().and_then(...).unwrap_or_default；notification_store、tunnel_config、telegram_config、log_alert_config 也在各自 load 中静默回退默认值，损坏的持久化数据会被伪装成首次运行状态并可能在后续保存时覆盖。
- **配置、模型与 JSON 持久化** · `src/config/auth_config.rs:128` — 已有 auth.json 读取或解析失败时，load 直接生成新的 master_token（137-144），并忽略 save 结果（145）；临时文件锁或损坏即可静默轮换 CLI 凭据并丢失原认证状态。
- **配置、模型与 JSON 持久化** · `src/config/notification_store.rs:34` — notifications 保存直接调用 std::fs::rename(tmp, path)，没有 Windows 覆盖替换或 fallback；同一文件已存在时 Windows rename 通常失败，通知配置更新可能无法持久化。该实现也与 project_store 的 MoveFileExW 替换逻辑（54-73）不一致。
- **配置、模型与 JSON 持久化** · `src/models/process_info.rs:47` — ProcessInfo 文件声明其结构用于 API 发送（1），但序列化字段包含完整 env HashMap（47-48）；任何返回 ProcessInfo 的接口都可能原样暴露环境变量中的凭据。
- **配置、模型与 JSON 持久化** · `src/models/ai.rs:22` — AiSettings 默认派生 Serialize/Deserialize，且直接保存 github_token、anthropic_key、openai_key（22、31、35），没有 skip 或脱敏边界；任何对该模型的序列化都会原样输出密钥。
- **AI、通知、隧道与观测集成** · `src/api/routes/ai.rs:558` — 每次 chat 请求都 tokio::spawn 独立上游流任务（:558-582）；输入 history/message 在此处未见大小或并发限制，reqwest::Client::new()（:596,655,737）未设置请求超时，客户端断开后上游任务可能继续占用资源或产生计费。
- **AI、通知、隧道与观测集成** · `src/api/routes/ai.rs:198` — GitHub Device Flow 状态存放在全局 state.ai_device_auth（:203-205），auth/status 没有用户/session 绑定；不同客户端可轮询、推进或消费同一设备授权流程。
- **AI、通知、隧道与观测集成** · `src/api/routes/ai.rs:57` — AI 配置使用直接 std::fs::write 覆盖 ai-settings.json（:63-66），未见原子替换、并发写保护或文件权限处理；同时持久化 API token 等敏感配置。
- **AI、通知、隧道与观测集成** · `src/notifications/dispatcher.rs:29` — 每个通知事件对每个渠道直接 tokio::spawn fire-and-forget 任务（:29-67），没有全局并发上限、队列长度或合并策略；崩溃/健康告警风暴可无限堆积任务。
- **AI、通知、隧道与观测集成** · `src/notifications/channels/webhook.rs:15` — Webhook 发送只检查 reqwest transport error，未调用 error_for_status（:15-20）；HTTP 4xx/5xx 仍可能返回 Ok，dispatcher 可能将失败通知当作成功。Slack/Discord 通道有同样模式。
- **AI、通知、隧道与观测集成** · `src/tunnel/mod.rs:108` — 隧道发现 URL 超时或输出结束时仅将状态置 Failed、移除 pids 并 return（:108-121），该分支未调用 child.kill；静态代码未看到子进程终止保证，可能遗留 tunnel 进程。
- **AI、通知、隧道与观测集成** · `src/tunnel/mod.rs:202` — Custom provider 将配置中的 binary_path 直接传给 Command::new，并将 args_template 按空白拆分执行（:202-218）；API 可更新完整 TunnelSettings（src/api/routes/tunnels.rs:90-101），未见路径/参数边界或可执行文件归属校验。
- **AI、通知、隧道与观测集成** · `src/tunnel/mod.rs:146` — stop 依据保存的 PID 直接调用 kill_pid（:146-150）；kill_pid 在 Windows 使用 taskkill /F /T /PID、Unix 发送 SIGTERM（:334-349），未校验进程身份，存在 PID 复用误杀风险。
- **AI、通知、隧道与观测集成** · `src/api/routes/tunnels.rs:127` — Linux provider 安装通过 sh -c 执行 curl | sudo tee、apt-get update/install（:120-135），安装接口没有可见的超时、输出大小或并发控制；SSE 安装流同样启动该子进程（:218-246）。
- **AI、通知、隧道与观测集成** · `src/telegram/bot.rs:167` — 每条 Telegram update 都创建独立 tokio::spawn 命令任务（:163-173），没有并发上限；高频消息可同时触发大量进程查询/控制与 Telegram 回包。
- **AI、通知、隧道与观测集成** · `src/telegram/commands.rs:14` — send_message 只等待 HTTP 请求完成并返回 Ok（:16-26），未解析 Telegram JSON 的 ok 字段或检查 HTTP 状态；服务端拒绝消息时上层仍可能报告成功。
- **AI、通知、隧道与观测集成** · `src/api/routes/telegram.rs:97` — allowed_chat_ids 直接接受任意长度 Vec 并持久化（:97-105），没有数量或输入大小限制；与 bot.rs 空列表放行逻辑共同扩大配置误用和资源风险。
- **AI、通知、隧道与观测集成** · `src/api/routes/logs.rs:28` — flush_logs 对运行中进程的 out.log/err.log 直接 remove_file（:28-34），未停止 writer 或协调文件句柄；Windows 可能删除失败，Unix 可能让 writer 继续写入已删除 inode，导致查询与实际日志分离。
- **AI、通知、隧道与观测集成** · `src/api/routes/metrics.rs:24` — Prometheus label 直接插入用户可控的 process name/namespace（:24-27,35-38,45-48,55-58,66-71），未转义引号、反斜杠或换行，可生成非法或伪造的指标文本。
- **AI、通知、隧道与观测集成** · `src/api/routes/log_alerts.rs:22` — PUT log-alerts 直接反序列化并整体保存 LogAlertStore，namespace override 也直接 insert（:22-39），未见阈值、冷却时间、命名空间数量或字段大小校验；异常输入可造成配置膨胀或告警风暴。
- **构建、发布与工程文档** · `.github/workflows/release.yml:7` — Windows release workflow 仅在 v* tag push 触发（:7-10），Windows/Linux 构建 job 只执行 npm run build 和 cargo build（Windows :54-63，Linux workflow :39-43、:84-87），没有 PR/main 的 cargo test、clippy、cargo audit、前端 test/lint 门禁。
- **构建、发布与工程文档** · `.github/workflows/release.yml:32` — 发布流程使用 dtolnay/rust-toolchain@stable、actions/checkout@v4、setup-node@v4 和 softprops/action-gh-release@v2（:29-32,47-52,189-197），均未 pin 到 commit SHA；release workflow 具备 contents: write，供应链变更可直接影响发行物。
- **构建、发布与工程文档** · `Cargo.toml:3` — 项目包版本为 1.1.0，但 installer/alter-setup.iss 默认 AppVersion 仍为 0.1.0（:6），docs/API.md 示例 health version 为 0.3.0（:360）；版本真相分散，依赖构建脚本或 CI 临时改写。
- **构建、发布与工程文档** · `scripts/release.ps1:20` — 本地 release 脚本直接修改受版本控制的 installer/alter-setup.iss（:22-24），没有备份、回滚或 finally 清理；构建、ISCC 或哈希步骤失败后工作树会残留版本改写。
- **构建、发布与工程文档** · `.gitignore:24` — 仓库忽略 winget/manifests（:24），且当前没有 winget 目录；scripts/release.ps1 :47-57 只有目录存在时才更新 WinGet SHA256，当前仓库执行发布脚本会静默跳过该完整性元数据更新。
- **构建、发布与工程文档** · `web-ui/package.json:6` — 前端 package.json 以 npm scripts 和 package-lock 为主（:6-15），仓库同时保留 bun.lock；bun.lock 的 devDependencies（:23-38）缺少 package.json 中的 testing-library、msw、prettier、vitest 等条目，存在双锁文件漂移，CI 又固定使用 npm ci。
- **构建、发布与工程文档** · `Justfile:31` — Rust 测试 recipe 将 cargo nextest 的 stderr 重定向到 /dev/null，并对任意非零结果执行 cargo test（:31-34）；nextest 不可用、崩溃或测试行为差异时诊断被隐藏，可能把失败原因伪装成普通 fallback。
- **构建、发布与工程文档** · `lefthook.yml:5` — pre-commit 只覆盖 staged 前端 lint-staged 与 Rust cargo fmt --check（:5-16），没有 Rust clippy、cargo test、cargo audit、前端 test/build；本地钩子与 Justfile 的完整 lint/test 目标不一致。
- **构建、发布与工程文档** · `scripts/build-deb.sh:13` — VERSION、ARCH 和 binary-path 全部来自位置参数（:13-19），脚本不校验参数数量、架构枚举或版本格式；:43-47 将 VERSION/ARCH 直接插入 sed 替换表达式，特殊字符可破坏 control 文件或生成非预期包名。
- **构建、发布与工程文档** · `installer/alter-setup.iss:61` — 安装器以 HKLM 写入系统 PATH（:61-66），并在安装后执行 setx /M PATH（:85-94）；全局环境变更和 setx 的长度或展开语义未做保护，可能破坏已有系统 PATH。
- **构建、发布与工程文档** · `packaging/debian/prerm:5` — 卸载前对 alter daemon stop 和 systemctl stop/disable 的错误全部使用 2>/dev/null || true（:5-12）；服务停止失败仍继续卸载，可能留下运行进程、PID 或占用文件。

### LOW (29)

- **React 应用壳与导航** · `web-ui/src/App.tsx:137-144` — 更新检查失败在 :143 使用空 catch；更新徽标静默缺失，壳层没有诊断信号。该功能可降级，但错误不可观测。
- **React 应用壳与导航** · `web-ui/src/App.tsx:1844-1855` — SSH 隧道 RemoteServer 预览对象在 copyTunnelCmd 中手工构造，并在 JSX :2189-2199 再次复制同一字段映射；端口默认值和字段变化需要同步维护，属于局部 duplication。
- **React 应用壳与导航** · `web-ui/src/components/GitHubStarBanner.tsx:237-249` — GitHubStarWidget 的外部 fetch 失败在 :246-248 静默吞掉，页面退化为不显示数量；注释明确这是非关键降级，因此严重度为 LOW，但仍缺少可观测信号。
- **React 应用壳与导航** · `web-ui/src/App.css:1-42` — 内容仍是 Vite 初始模板的 #root、.logo、.card、.read-the-docs 和 logo-spin 样式；指定入口 main.tsx 仅导入 index.css（:1-4），App.tsx 也未导入 App.css，当前范围内表现为未接入的 legacy 样式资产。
- **前端进程与项目工作台** · `web-ui/src/components/CodeEditor.tsx:126` — 将 CSS 属性值 off 强制断言为 React.CSSProperties[overflowWrap]，绕过类型检查；该值并非标准 overflow-wrap 值，属于不必要的 any-escape。
- **前端观测与运维页面** · `web-ui/src/pages/AnalyticsPage.tsx:527` — AnalyticsPage :527-530 与 LogVolumePage :16-20 重复实现相同的 ISO 时间到 HH:MM 格式化函数；公共展示逻辑分散维护。
- **前端观测与运维页面** · `web-ui/src/components/NotificationTray.tsx:218` — 每一条通知的 RelativeTime 都独立创建一个 30 秒 setInterval（:220-223）；通知数量增长时定时器数量线性增长，属于局部 bloat。
- **前端观测与运维页面** · `web-ui/src/components/NotifModal.tsx:200` — ProcessNotifModal/NsNotifModal 的异常处理均使用 catch (e: any)（:200、:208、:274、:281），错误边界绕过 TypeScript 类型约束，属于低严重度 any-escape。
- **前端设置、AI 与终端** · `web-ui/src/components/settings/SecurityTab.tsx:141` — 自动锁定保存异常被完全忽略，finally 只恢复 saving 状态；用户看不到失败原因，也无法确认锁定设置是否真正持久化。
- **前端 API、认证与轮询层** · `web-ui/src/lib/auth.ts:44` — prepareCreationOptions 和 prepareRequestOptions 明确以 any 接收服务端 WebAuthn 数据（44-45、62-63），没有运行时结构校验；挑战字段缺失或类型错误会在 base64urlToBuffer/浏览器 API 深处才失败。
- **前端 API、认证与轮询层** · `web-ui/src/lib/processWeb.test.ts:13` — 模块测试目前只覆盖 processWeb helpers（13-57）和 projects helpers（projects.test.ts:24-46）；api/auth/servers/settings 以及三个轮询 hook 没有同目录测试，最高风险的鉴权、SSE、fallback 和并发轮询路径缺少回归保护。
- **CLI、守护进程与 Web 入口** · `src/web/mod.rs:20` — 嵌入式 SPA 响应只设置 Content-Type（:24-39），未设置 CSP、X-Content-Type-Options、Referrer-Policy 等安全响应头；未知路径统一回退 index.html，调试/错误路由边界较弱。
- **CLI、守护进程与 Web 入口** · `src/lib.rs:72` — web 命令将可控 host/port 拼成 URL，并在 Windows 通过 cmd /c start 执行（:72-78）；输入未做 URL/字符校验，存在 shell 参数解释和打开非预期地址的边界风险。
- **CLI、守护进程与 Web 入口** · `src/cli/commands/startup.rs:15` — 生成的 PowerShell/systemd 启动文本直接插入 current_exe 与 USER（:7-17,22-44），未做引号、换行或特殊字符转义；路径或环境变量含特殊字符时，复制生成内容可能得到错误或可注入的启动配置。
- **CLI、守护进程与 Web 入口** · `src/client/daemon_client.rs:97` — 响应解析失败时直接 unwrap_or(Value::Null)，非 JSON 错误体被转换成 unknown error（:97-110），丢失服务端诊断信息并使 CLI 难以区分协议错误与业务错误。
- **进程、项目与 Ecosystem API** · `src/api/routes/processes.rs:688` — resolve 将 manager.resolve_id 的所有错误统一映射为 404（688-693），无法区分非法 ID、名称解析失败和真实不存在，诊断信息被静默折叠。
- **进程、项目与 Ecosystem API** · `src/api/routes/processes.rs:1` — processes.rs 约 693 行，单文件同时承载 CRUD、生命周期、日志/SSE、指标、终端、env 文件、批量操作和 clone；职责明显泛化，且多个路由重复 detached autosave 与错误处理逻辑。
- **认证与本机系统能力 API** · `src/api/routes/auth.rs:275` — 四个 passkey 路由固定返回 501 stub，认证能力表面存在但实际未实现，前端调用只能得到占位错误。
- **认证与本机系统能力 API** · `src/api/routes/auth.rs:155` — logout 仅从 Authorization header 提取 token；middleware 同时支持 query token（:67-84），因此通过 SSE query 建立的 session 无法由该 logout 路由清除。
- **认证与本机系统能力 API** · `src/api/routes/auth.rs:309` — 认证 token 解析逻辑在 auth.rs 与 middleware.rs 重复实现（middleware :67-84），两处仅支持精确 Bearer 前缀，存在行为漂移和维护重复。
- **进程监督与日志核心** · `src/process/rolling_restart.rs:20` — 公开导出的 rolling_restart（模块由 src/process/mod.rs:9 导出）仍直接返回 not yet implemented，多实例滚动重启目前是 stub，调用方无法获得该能力。
- **进程监督与日志核心** · `src/logging/reader.rs:17` — read_last_lines 先把整个文件读入 Vec 再截取尾部（src/logging/reader.rs:17-22）；在日志文件由 src/logging/writer.rs:15 限制到 10MB、且接口只需 n 行的情况下存在不必要的内存与扫描开销。
- **配置、模型与 JSON 持久化** · `src/daemon/state.rs:23` — SavedState 只有 saved_at 和 apps，没有 schema/version 或迁移分派；兼容性依赖零散的 serde default（36、40、45），而 load_from_disk 在解析失败时直接返回错误（148-152），未来状态结构变更缺少明确迁移边界。
- **配置、模型与 JSON 持久化** · `src/daemon/state.rs:226` — cron_run_history 虽在 SavedApp 中持久化并在活动 cron 恢复时传递（217），但手动停止的 cron 走 register_stopped(id, config)（226），历史记录在 daemon 重启后被丢弃。
- **配置、模型与 JSON 持久化** · `src/config/auth_config.rs:16` — StoredPasskey 仅保存 raw serde_json::Value；注释明确真实 WebAuthn backend 尚未接入（16-18），但 AuthConfig 已将其作为可持久化 passkey 记录，属于未完成的占位能力。
- **AI、通知、隧道与观测集成** · `src/tunnel/mod.rs:276` — Custom provider 只从任意输出行提取第一个 https:// 字符串并保存为 public_url（:318-329），没有 URL 解析、域名或路径校验；恶意/异常 provider 输出可造成错误的公开地址展示。
- **AI、通知、隧道与观测集成** · `src/notifications/sender.rs:275` — 通知 payload 将 process name、namespace、script、PID 和重启信息发送至外部 webhook（:275-292），未见敏感字段过滤或长度限制；配置误指向第三方时可能扩大运行环境信息泄露。
- **构建、发布与工程文档** · `README.md:123` — 根 README、release/README.md 和 docs/README.md 都维护独立安装、功能和文档索引副本（根 README :123-134，release README :114-125），没有生成或一致性校验机制，品牌、命令和版本说明容易继续漂移。
- **构建、发布与工程文档** · `docs/CHANGELOG.md:187` — CHANGELOG 将 depends_on、rolling restart 标记为代码已提交但尚未在 binary 激活（:187-190），同时 docs/ECOSYSTEM_CONFIG.md 还公开 Reserved 或自定义日志字段（:122-125）；文档混合描述实现、预留和未来特性，发布能力边界不清。

## Cross-cutting themes

- **安全依赖运行前提而非代码强制.** 当前监听 127.0.0.1 限制了网络暴露，但默认无密码与全开放 CORS 仍让恶意网页或本机进程接近文件、终端、脚本和更新接口；一旦改绑非回环地址，风险进一步扩大为网络远程控制面。
- **失败经常被伪装成成功、空数据或首次运行.** 从状态配置加载、日志读取、前端轮询到更新/重启流程，大量 silent catch 与默认回退让真实故障表现为零值、加载中、默认配置或成功提示，削弱恢复与诊断。
- **持久化与副作用缺少事务边界.** 进程启动、项目元数据和 state.json 分步保存；固定 tmp 文件、并发异步写、保存失败不回滚以及重复 resurrect 都可能产生部分成功、旧快照覆盖或运行态与磁盘态分裂。
- **重复生命周期实现已经产生行为漂移.** 首次启动、崩溃重启、cron 启动和恢复分别维护相似的 spawn/日志/watch/health 流程；watcher 被立即释放、自动重启漏掉 hook/健康检查等缺陷说明重复不再只是代码风格问题。
- **测试与 CI 没有覆盖最高风险边界.** 已有 Rust/前端测试能保护部分解析与 UI helper，但认证、文件边界、持久化并发、恢复幂等、真实 API、更新完整性和通知/AI 出站均缺少系统级回归门槛；Release workflow 也不先跑完整质量门禁。
- **架构规模合适，模块内部边界欠清晰.** 单体 Rust daemon + React 控制台符合本地 V1 规模，不需要微服务；真正的问题集中在 ProcessManager、App.tsx、API 路由与设置页面等 God 模块，应通过小轮次提取状态机、IO 边界和错误语义。
