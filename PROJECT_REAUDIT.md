# RunDock / Alter 第二次工程复检报告

> 复检日期：2026-08-30
> 对比基线：`PROJECT_AUDIT.md`（2026-08-26）
> 当前分支：`codex/p0-p2-remediation`
> 当前 HEAD：`f920c50289134fc8d6d6febba1e6e8895ee859a6`
> 方法：Codemap 13 模块独立复审 + Code Overhaul 全面复核 + SonarQube/LCOV/CI 证据交叉验证
> 边界：本轮只复检；未修改生产代码、测试代码、运行配置或远端状态

## 0. 最终结论

当前工程健康度为 **68.9/100（C）**，低于上次 Codemap 的 `80.9/100`。这不代表全部整改失效：核心架构仍合适，多数 P0 安全护栏、资源上限、依赖治理、bundle 和基础测试仍然有效；但本次更严格的调用链复核找到了 **6 个 HIGH**，其中包括开发代理暴露、Cron 进程树所有权、重复重启、首次轮询失效和 Linux 端口漏报。

按上次报告的 23 个主题逐项严格复核：

- **13/23 完全解决并仍有当前证据支持**；
- **10/23 已有主要整改，但关键边界仍未闭合，改判为部分解决/重新打开**；
- **0/23 完全未处理**。

这是一种“按主题整行验收”的保守计数：某一主题只要还有一个会影响真实行为的关键分支，就不继续记为完成。

本轮结论是：**可以停止大范围的代码洁癖和全面拆文件，但还不能停止本轮优化。** 应先做一轮范围很窄的回归修复，关闭第 4 节的 6 个 HIGH，并补对应行为测试；随后恢复可执行的 Sonar/CI 门禁。完成这些后即可停止，不建议继续做全面重构。

## 1. 证据边界与当前状态

### 1.1 Git 与代码范围

- 工作分支相对 `origin/codex/p0-p2-remediation` **ahead 1**；本地提交 `f920c50` 是 Linux Clippy 条件编译修复，尚未推送，不能视为远端已验证。
- 上次验收提交 `d3b4d36` 之后新增了 Windows daemon 重启、PID 0、桌面壳、loopback proxy、品牌与托盘/UI 修正等代码；当前 Codemap 覆盖 **236 个文件、83,436 LOC**，上次为 225 个文件、75,270 LOC。
- 本轮仅更新审计产物：本文件和 `.codemap/`；没有 commit、push、PR 更新、运行时启停或生产变更。

### 1.2 自动化验证

| 验证 | 当前结果 |
|---|---|
| 根 Rust fmt/check/clippy | 通过 |
| 根 Rust 测试 | 通过：189 单元 + 3 集成，共 192 个 |
| 前端 lint/typecheck | 通过 |
| 前端测试 | 通过：26 个测试文件、113 个测试 |
| 前端 build | 通过：1,876 modules；最大 chunk 290.60 kB |
| npm 高危漏洞审计 | 通过：0 vulnerabilities |
| 前端 format:check | 本机失败：89 个 CRLF 工作树文件；Git 索引为 LF，远端 CI 同提交通过，属于 Windows 行尾可复现性问题，不记作源代码格式回归 |
| desktop-shell fmt | 通过 |
| desktop-shell test/clippy | 本机失败：当前工具链是 `x86_64-pc-windows-gnu` 且缺 `windres`；测试二进制还出现 `STATUS_ENTRYPOINT_NOT_FOUND`；远端 Windows installer smoke 在 `eb4ad22` 通过，因此暂定为本机工具链/产物不一致，不能宣称本地门禁通过，也不能直接定性为功能回归 |
| cargo-audit | 本机未安装，未进行系统级安装；CI 路径仍是权威证据 |

### 1.3 当前远端门禁

- PR：`https://github.com/damingishere-coder/RunDock/pull/1`，状态 `OPEN / UNSTABLE`，远端 HEAD 为 `eb4ad22`。
- 最新 Quality run `33262470419`：Frontend quality 和 Windows installer smoke 成功；Rust quality 因 Linux Clippy 失败；Linux package 与 Sonar job 随后跳过。
- 本地 `f920c50` 已让根 Rust Clippy 通过，但尚未 push，故没有远端证据。
- SonarQube gate configuration 因 `SONAR_HOST_URL` / `SONAR_TOKEN` 未配置而失败，当前质量门禁不是可执行状态。

## 2. 上次 P0 / P1 / P2 的真实关闭状态

### 2.1 P0：3/4 完全解决，1/4 部分解决

| 上次主题 | 当前状态 | 复检结论 |
|---|---|---|
| P0-1 控制面认证/CORS | **部分解决 / 重新打开** | 生产 daemon 的回环监听、鉴权、CORS 和 stream ticket 仍有效；但 Vite 在 `0.0.0.0:5173` 提供同源 `/api`/WebSocket 代理，可把 passwordless loopback 控制面间接暴露给 LAN。 |
| P0-2 env 路径穿越 | **完成** | 安全组件名、cwd 边界、no-follow、大小限制和跨平台 traversal 拒绝仍保留；未发现回归。 |
| P0-3 状态并发与损坏恢复 | **完成** | 原子写、备份校验、marker 与损坏恢复主链仍成立；Telegram checkpoint 的窄并发问题单独列在 P1/P2 状态边界，不推翻核心状态事务。 |
| P0-4 自更新信任链 | **完成** | 固定资产、摘要、发布者校验和 fail-closed 仍存在；未发现新的绕过。 |

### 2.2 P1：4/8 完全解决，4/8 部分解决

| 上次主题 | 当前状态 | 复检结论 |
|---|---|---|
| P1-1 副作用/持久化部分成功 | **部分解决** | 多数 snapshot/补偿存在；生态导入在清理或持久化失败时仍可能留下孤儿/未持久化进程，项目/进程 PATCH 也不能表达显式清空。 |
| P1-2 watcher 所有权 | **部分解决** | 正常启动路径会处理 watcher 创建失败；恢复路径只记日志并保持 `Watching`，可能形成 `file_watcher=None` 的假监督状态。 |
| P1-3 生命周期漂移 | **部分解决 / 重新打开** | `cron_trigger_loop` 仍复制普通 spawn/commit 流程，且退出分支漏掉 retained process tree 终止，已经出现真实语义漂移。 |
| P1-4 LogWriter 泄漏 | **完成** | 明确停止/Drop 所有权仍在；未发现新的 writer 泄漏证据。 |
| P1-5 PID 复用/进程树 | **部分解决 / 重新打开** | Job/pidfd/启动时间基础存在；Cron 正常退出没有释放 retained tree，树所有权创建失败时也只 kill 根进程，身份捕获失败还会被部分路径当作“未运行”。 |
| P1-6 AI/通知/隧道出站 | **完成** | 统一出站策略、大小限制、credential 绑定和私网/回环限制仍在；未发现新绕过。 |
| P1-7 Telegram 空白名单 | **完成** | token 与显式 allowlist 的 fail-closed 仍成立；checkpoint 单调写入竞态影响重复消费，不会把空白名单重新变成放行。 |
| P1-8 发布安全 | **完成** | job 权限、签名、固定指纹和 pinned Actions 仍在；发布 artifact 的安装生命周期验证不足归入 P2 门禁完整性。 |

### 2.3 P2：6/11 完全解决，5/11 部分解决

| 上次主题 | 当前状态 | 复检结论 |
|---|---|---|
| 错误语义 | **部分解决** | 统一 API 错误主链有效，但部分页面仍把 sleeping/starting 错判，日期筛选和不可清空配置会给出错误状态。 |
| 日志/流资源上限 | **完成** | 有界 tail、aggregate、SSE/AI/provider 限制与前端 ring buffer 仍在。 |
| 轮询并发 | **部分解决 / 重新打开** | single-flight/AbortController 存在，但 `enabled=false` 同时禁掉首次 tick，Tunnels 及部分 hooks 可以永远不做初始加载。 |
| CI/质量门禁 | **部分解决 / 重新打开** | 门禁文件存在，但当前 PR 为红；Sonar 配置 job 失败；desktop-shell 未进入 Sonar/覆盖率/独立 fmt-clippy；Linux/正式 release artifact smoke 不完整。 |
| npm 漏洞 | **完成** | 当前 `npm audit --audit-level=high` 为 0。 |
| Rust 直接依赖 | **完成** | 未发现上次已删除依赖重新引入。 |
| 锁文件真相 | **完成** | npm/package-lock/CI 单一路径仍成立。 |
| bundle | **完成** | 当前最大 chunk 290.60 kB，仍低于 500 KiB 预算。 |
| dead/legacy | **完成** | 未发现值得本轮继续处理的新死代码；没有为了清理而清理的必要。 |
| 热点职责第一轮拆分 | **部分解决** | 已提取的边界仍有价值；但 Cron 与普通 spawn 的重复已造成进程树缺陷，ProcessManager 的该段重复需要定点合流。 |
| 测试补齐 | **部分解决** | 测试数量增加且主套件通过，但缺少重复重启、首次 tick、Linux `ss`、Cron tree cleanup 和 Vite LAN 边界测试；desktop-shell 本机套件未通过。 |

## 3. Codemap 第二次结果

### 3.1 总体变化

| 指标 | 上次 | 当前 | 说明 |
|---|---:|---:|---|
| 模块数 | 12 | 13 | 新增 `desktop_shell` 正式模块 |
| 文件 / LOC | 225 / 75,270 | 236 / 83,436 | 桌面壳和后续功能进入地图 |
| 平均健康度 | 80.9 | **68.9** | 下降 12.0；既有范围扩大，也有本次发现的 HIGH 与更严格的调用链审查 |
| 等级分布 | 2A / 8B / 2C | **0A / 6B / 3C / 4D** | D 集中在进程前端、操作页、系统端口和生命周期 |
| Findings | 0 HIGH / 11 MED / 7 LOW | **6 HIGH / 33 MED / 6 LOW** | 报告只展开当前有业务价值的 HIGH 和关键 MED |

### 3.2 当前模块评分

| 模块 | 分数 | 等级 | 当前判断 |
|---|---:|---:|---|
| frontend_shell | 76 | B | App/ServerSwitcher 仍大，但非本轮阻塞 |
| frontend_process_projects | 52 | D | 重复重启、不可清空配置、页面职责过载 |
| frontend_operations | 52 | D | Tunnels 首次加载、Linux 状态兼容、日期/状态语义 |
| frontend_settings_ai_terminal | 78 | B | Terminal 大，但未发现新高风险回归 |
| frontend_transport | 68 | C | `enabled` 与初始加载语义耦合、运行时 schema 边界偏弱 |
| bootstrap_daemon | 78 | B | PID 身份未知/legacy 记录仍有 fail-closed 缺口 |
| api_process_projects | 62 | C | PATCH clear 语义、导入补偿与总超时边界 |
| api_security_os | 56 | D | Linux `ss` 解析漏报、跨平台端口语义漂移 |
| process_lifecycle | 52 | D | Cron 复制路径与进程树清理是当前最高维护风险 |
| state_persistence | 84 | B | 核心状态稳健；checkpoint 单调写入缺少同锁事务 |
| integrations_observability | 84 | B | 未发现新 P0/P1；AI 路由大文件暂缓处理 |
| desktop_shell | 82 | B | 代码边界清晰；错误可观察性和质量门禁仍需补齐 |
| delivery_docs | 72 | C | Vite LAN 代理、Sonar/安装生命周期门禁缺口 |

Codemap 已重新生成：`.codemap/codemap.md` 与 `.codemap/codemap.html`，13/13 模块均为 fresh。

## 4. 当前仍值得继续处理的问题

以下只列会影响安全、运行正确性或门禁可信度的问题；普通大文件、低收益拆分和依赖追新不进入本轮。

### R0-1 Vite 开发代理破坏 loopback-only 假设

- **优先级：P0 / HIGH；预计工作量：S**
- 证据：`web-ui/vite.config.ts:72-80` 将开发服务器绑定 `0.0.0.0`，并把 `/api` 和 WebSocket 代理到 `127.0.0.1:2999`；文档明确说 passwordless 模式只能在 loopback 使用。
- 影响：同一局域网中的客户端可经 `:5173` 间接访问本机 daemon；Vite 代理让 daemon 看到的是本机来源，削弱生产端的 loopback/CORS 防线。
- 建议：默认只绑定 loopback；若确需 LAN 调试，使用显式 opt-in 且强制控制面密码，并加入行为测试。

### R1-1 Cron 与进程树所有权仍会漂移

- **优先级：P1 / HIGH；预计工作量：M**
- 证据：`src/process/manager.rs:2323-2337` 的 Cron 退出路径清空 PID/identity/log writer，却没有调用 retained tree 终止；`src/process/runner.rs:280-285` 和 `src/process/hooks.rs:60-65` 在 tree guard 创建失败时只 kill 根进程。
- 影响：带 descendants 的 Cron/job/hook 在边界失败或根进程提前退出后可能残留，下一次运行覆盖旧 guard，违背上次声称已统一的进程树所有权。
- 建议：把 Cron 的结束/提交路径真正合流到共享生命周期原语；对 guard 创建失败和正常退出都做树级清理，并补 Unix/Windows 定向测试。

### R1-2 “保存并重启”会连续重启两次

- **优先级：P1 / HIGH；预计工作量：S**
- 证据：`web-ui/src/components/EnvFileModal.tsx:142-143` 先直接 `restartProcess` 再调用 `onRestart`；`ProcessDetailPage.tsx:395-400` 的回调又调用相同 API，且第二次没有被当前 handler await。
- 影响：一次用户操作会连续重启两次，可能重复 hooks、通知、日志切换和短暂端口冲突。
- 建议：保留一个 restart 所有者，并以“API 仅调用一次”的组件测试锁定。

### R1-3 条件轮询禁用了首次加载

- **优先级：P1 / HIGH；预计工作量：S**
- 证据：`web-ui/src/pages/TunnelsPage.tsx:513-516` 在初始 tunnels 为空时传入 `enabled=false`；`useSingleFlightPoll` 不再执行首次 tick，页面可一直 loading，只有手动刷新才能恢复。`useProcesses`/`useProjects` 也共享同类语义风险。
- 影响：整改轮询重叠时引入了“初始请求与周期请求被同一开关关闭”的功能回归。
- 建议：把 `initialLoad` 与 `intervalEnabled` 分开；测试首次 mount、关闭自动刷新、服务器切换三种状态。

### R1-4 Linux `ss` 监听端口会被当成 netstat 丢弃

- **优先级：P1 / HIGH；预计工作量：S**
- 证据：`src/api/routes/ports.rs:248` 用 `fields.len() >= 7` 判断 netstat；常见 `ss -Hntlpu` 带 users 字段时同样达到 7 列，于是把 RecvQ 当成本地地址并丢弃整行。`ss` 命令成功后不会回退 netstat。
- 影响：Linux 监听端口页、端口占用判断和 Project WebPort 关联可能漏报。
- 建议：按命令来源使用独立 parser，加入真实 `ss`/`netstat` fixture；前端统一复用已能接受 `LISTEN`/`LISTENING` 的 `processWeb.ts` helper。

### R1-5 状态更新仍缺少“显式清空”和同锁单调提交

- **优先级：P1 / MED；预计工作量：M**
- 证据：`ProcessPatch`/`ProjectPatch` 的 `Option<T>` 无法区分字段缺失与 JSON `null`，cron/cwd/notify/web_port/launch_uri 等已设值不能可靠清空；`telegram_checkpoint::save` 在同一锁外先 load/校验再 write，并发写入可发生较小 offset 后写覆盖。
- 影响：UI 看似保存成功但旧配置残留；Telegram 极端并发下会重复消费更新。
- 建议：使用可表达 absent/null/value 的 patch 类型；把 checkpoint 读-校验-写放进同一个文件级临界区。

### R2-1 质量门禁仍不能证明“当前提交可发布”

- **优先级：P2 / MED；预计工作量：M**
- 当前 PR 为红，本地修复尚未推送验证；Sonar 配置 job 失败。
- `desktop-shell` 未进入 Sonar、覆盖率和独立 fmt/clippy；当前本机 GNU 工具链也不能执行其测试。
- Linux package smoke 未安装 `.deb`/运行 maintainer scripts/systemd 健康检查；正式 Windows release artifact 没有再做安装/升级/卸载 smoke。
- 前端覆盖率相对上次下降约 2–3 个百分点，新增测试没有跟上新增源代码。
- 建议：先恢复 Sonar 认证与远端绿灯，再补 desktop-shell 和最终发布 artifact 的最小可发布性验证；不要求把覆盖率机械抬到 80%，只要求新增高风险路径有行为测试。

## 5. Code Overhaul 复核

### 5.1 架构与范围

“单体 Rust daemon + React 控制台 + Windows 桌面壳 + 本地 JSON 状态”仍符合当前产品规模。没有证据支持拆成微服务、引入数据库或替换技术栈。真正的问题不是单体，而是少数职责在不同路径重复实现：普通启动与 Cron 启动、初始加载与周期轮询、API patch 与 UI 表单状态。

关键链路仍清晰：

```text
Desktop shell / Browser
          │
          ▼
React UI ── transport/auth/polling ── Axum API
                                         │
                      ┌──────────────────┼──────────────────┐
                      ▼                  ▼                  ▼
               Process lifecycle   State/config       OS/Providers
                      │                  │                  │
                 Job/pidfd/PGID      Atomic JSON       Ports/AI/Tunnel
```

### 5.2 复杂度与重复代码

- `src/process/manager.rs` 约 3,431 行，且 Cron 区段复制了普通 spawn/commit 主链；这项重复已经产生实际 HIGH，因此值得定点合流。
- `ProcessesPage`、`ProcessDetailPage`、`PortFinderPage`、`AnalyticsPage`、`TerminalPanel`、`api.ts` 和 `ai.rs` 仍大，但除报告列出的具体语义错误外，**不建议本轮全面拆分**。
- 通知默认值、统计转换和部分页面状态存在重复，尚未造成同等级生产问题，继续优化收益低于回归风险。

### 5.3 依赖、性能与安全

- npm 高危漏洞为 0；Cargo 直接依赖未发现重新膨胀。
- npm/Cargo 有常规 minor/major 更新可用，但没有当前漏洞或故障驱动，不建议本轮追新。
- bundle 预算仍通过；日志/流的有界策略仍有效，没有发现新的性能阻塞证据。
- 生产控制面、env 边界、更新签名和出站策略总体维持上轮加固结果；当前安全重点只保留 Vite LAN 代理这一条。

### 5.4 明确不建议继续处理的项目

- 为了降低单文件行数而全面拆 App/Pages/AI/Terminal。
- 机械清空 Sonar smells、Minor 可访问性告警或所有重复代码。
- 无漏洞驱动的依赖大版本升级。
- 引入微服务、数据库、全新状态框架或重写前端。
- 上次已归入 P3 的本机同用户 TOCTOU、非 Linux POSIX 极窄 PGID 窗口，除非出现真实故障证据。

## 6. SonarQube 前后指标

### 6.1 可比较结果

| 指标 | 上次已认证扫描 | 当前 | 变化结论 |
|---|---:|---:|---|
| NCLOC | 53,281 | N/A | 当前扫描未认证，不能比较 |
| Bugs | 19 Minor | N/A | 不能比较 |
| Vulnerabilities | 0 | N/A | 不能比较 |
| Security Hotspots | 0 | N/A | 不能比较 |
| Code Smells | 571 | N/A | 不能比较 |
| Sonar Coverage | 29.6% | N/A | 不能比较 |
| Duplication | 3.4% | N/A | 不能比较 |
| Quality Gate | ERROR | 配置 job FAILURE / scan 不可执行 | 没有改善证据，门禁可用性反而退化 |

本机 SonarQube `127.0.0.1:9000` 可达，但当前没有可用认证；Scanner 请求 `/api/v2/analysis/version` 返回 `401 Unauthorized`。远端 workflow 的 `SONAR_HOST_URL` 和 `SONAR_TOKEN` 也为空，因此不能生成可信的“整改后第二次 Sonar 指标”。本报告不会把 2026-08-26 的 `.scannerwork/report-task.txt` 旧结果冒充当前结果。

### 6.2 前端 LCOV 的可比较旁证

这不是 Sonar 全项目指标，但使用同一前端测试覆盖率口径，可以作为趋势旁证：

| 前端覆盖率 | 上次 | 当前 | 变化 |
|---|---:|---:|---:|
| Statements | 37.58% | 35.01% | -2.57 pp |
| Branches | 36.43% | 34.12% | -2.31 pp |
| Functions | 34.00% | 32.29% | -1.71 pp |
| Lines | 39.47% | 36.52% | -2.95 pp |

测试从 107 增加到 113，但新增代码增长更快；覆盖率趋势小幅下降。重点应补第 4 节的高风险行为测试，不应为了数字盲目补低价值行覆盖。

## 7. 回归判断

### 已确认的回归/遗漏

1. Tunnels 的首次加载被整改后的条件轮询关闭。
2. Env 保存并重启形成双重 API 调用。
3. Linux 端口 parser 的启发式判断会丢弃常见 `ss` 输出。
4. Cron 仍有复制生命周期路径并遗漏树清理；上次“已统一”的结论不完整。

这些关键代码多数可追溯到上次主整改提交 `c2de2b1`，在 `PROJECT_AUDIT.md` 生成时已经存在，属于 **上次验收漏检的整改回归/未闭合边界**，不是本次复检期间产生的代码变化。

### 后续提交是否引入新回归

- 未发现 `d3b4d36` 之后的品牌、托盘、底栏和 loopback proxy 修改造成新的已证实 P0/P1 生产回归。
- 新增 desktop-shell 扩大了发布面，但当前主要证据是质量门禁覆盖不足和本机 GNU/MSVC 工具链不一致；还不能据此宣称已发生桌面功能回归。
- 当前远端 CI 红、Sonar 不可执行本身就是发布风险；在绿灯恢复前不能说当前分支已达到可发布状态。

## 8. 影响 / 工作量与停止条件

| 顺序 | 问题 | 影响 | 工作量 | 建议 |
|---:|---|---|---|---|
| 1 | Vite LAN 代理旁路 | 高 | S | 立即关闭默认外网绑定并测试 |
| 2 | Cron/guard 进程树所有权 | 高 | M | 合流生命周期，补树级清理测试 |
| 3 | Env 双重重启 | 高 | S | 单一 restart 所有者 + 调用次数测试 |
| 4 | 首次轮询被禁用 | 高 | S | 分离 initial load 与 interval |
| 5 | Linux `ss` 漏报 | 高 | S | 独立 parser + fixture |
| 6 | Patch/checkpoint 一致性 | 中 | M | 支持显式清空；同锁单调提交 |
| 7 | CI/Sonar/desktop/release smoke | 中 | M | 恢复可执行门禁并验证当前 SHA |

达到以下条件后，可以停止本轮优化：

1. 第 4 节 6 个 HIGH 全部关闭，并有对应行为测试；
2. 根 Rust、前端、desktop-shell 的适用门禁全部通过；
3. 当前提交在远端 CI 变绿，Linux package 不再被跳过；
4. Sonar 能对当前 SHA 完成一次认证扫描，并如实记录 gate；
5. 不再开启大文件全面拆分、依赖追新或 smell 清零。

在此之前：**不建议继续做新的功能性优化，也不建议发布或合并；但建议把修复范围严格限制在上述问题。**
