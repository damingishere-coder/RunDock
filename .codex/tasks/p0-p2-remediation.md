# RunDock / Alter P0-P2 整改任务

## 背景

依据仓库根目录 `PROJECT_AUDIT.md`，本轮把报告中的 P0、P1、P2 全部整改到可验证状态。目标不是机械清零 Sonar，而是让控制面、文件边界、持久化、生命周期、恢复、外部集成、错误语义、资源上限和质量门禁形成可执行的不变量。

## 目标

- 完成报告 P0-1 至 P0-4。
- 完成报告 P1-1 至 P1-8。
- 完成报告 P2 的错误语义、资源上限、质量门禁、依赖/Dead Code、热点职责拆分和关键测试补齐。
- 保留 `alter` CLI、现有数据目录、JSON 兼容读取和本地单体架构。
- 用自动化测试、静态检查、隔离运行态和更新后的 Sonar/Codemap 证明整改结果。

## 允许修改范围

- Rust：`src/**`、`tests/**`、`Cargo.toml`、`Cargo.lock`。
- 前端：`web-ui/src/**`、`web-ui/package.json`、`web-ui/package-lock.json`、必要的前端工具配置。
- 工程门禁：`.github/workflows/**`、`lefthook.yml`、`Justfile`、必要的安全/覆盖率配置。
- 文档与审计：`PROJECT_AUDIT.md`、`.codemap/**`、`.codex/tasks/p0-p2-remediation.md`。
- 已证实未引用的 dead/legacy 文件和依赖；删除前必须有引用扫描与全量验证证据。

## 禁止修改范围

- 不写入、迁移或删除 `%APPDATA%\alter-pm2` 中的真实 `state.json`、`projects.json`、认证配置或日志。
- 不停止、替换或接管当前安装版 Alter 及其管理的其他项目。
- 不执行真实 AI、Webhook、Telegram、Tunnel、自更新、安装、发布、部署或远程写操作。
- 不改变 Codex/OpenAI 登录、认证或模型提供商。
- 不引入微服务、消息队列、Kubernetes、CQRS、Event Sourcing 或无证据的大型架构。
- 不提交 API Key、Token、密码、Cookie、`.env`、日志、缓存、构建产物或扫描临时文件。

## 已确定实现要求

### P0

1. 非回环监听必须启用认证；CORS 只允许明确受信来源；默认本机 UI/CLI 兼容。
2. env 文件 API 只接受单个安全文件名组件，规范化后不得越过进程 cwd，并覆盖 Windows/Unix traversal 测试。
3. state 保存采用单写者/串行化、唯一临时文件、原子替换、last-known-good 和写后校验；保留旧 JSON 读取兼容。
4. 自更新只接受固定仓库、正确版本/平台资产，限制重定向和大小，校验 SHA-256 后才允许执行/替换，任何不完整证据 fail-closed。

### P1

1. 进程/项目变更必须明确 commit/compensate；不得返回“失败”却留下不可见副作用。
2. watcher、LogWriter、health、hooks、cron 由明确 handle 所有，start/restart/crash/stop 行为一致。
3. 恢复和 kill 校验 PID 身份；不匹配时不接管、不误杀，恢复可重复执行。
4. AI/Webhook/Tunnel 出站目标有协议、地址和私网边界；秘密读取只返回掩码；诊断内容脱敏并限制大小；Telegram whitelist fail-closed。
5. 发布工作流使用 job 级最小权限、关键 action pin SHA、签名缺失 fail-closed，并加入 PR/main 质量门禁。

### P2

1. UI/API 明确区分 loading、empty、stale、failed、partial；不得失败后仍显示成功。
2. 日志读取有行数/字节上限并避免整文件载入；前端实时日志有有界 buffer；轮询 single-flight、可取消、退避。
3. 在行为测试保护下拆分最危险的 App/ProcessManager/AI 职责，不做无收益全面重写。
4. 删除已证实无引用的模板/占位/旧实现和依赖，统一 npm 锁文件真相；每项删除可独立证明。
5. 修复现有 fmt/lint 门禁，并补 auth/CORS/path/state/lifecycle/恢复/外部目标/错误语义/资源上限测试。

## 验收标准

- `PROJECT_AUDIT.md` 中所有 P0、P1、P2 条目都有对应实现、测试或明确证明“不适用/已由更强不变量覆盖”的证据。
- Rust fmt、check、test、clippy 在受支持工具链上通过；若当前 GNU 环境仍缺工具，必须使用仓库支持的替代工具链完成同等验证，而不是跳过。
- 前端 format、lint、typecheck、unit、coverage、build 全部通过。
- 依赖审计和 secrets 扫描无未解释的高风险项；Sonar 重新扫描并记录有意义的变化。
- 隔离数据目录与非生产端口完成 daemon/API smoke；当前 Alter 管理的 2999/5173 路由不被替换或遗留手动进程。
- `git diff --check` 通过；最终只有本任务范围内文件；没有 TODO/debug、硬编码秘密或构建临时产物。

## 计划验证命令

- `cargo fmt --all -- --check`
- `cargo check --all-targets --locked`
- `cargo test --all-targets --locked`
- `cargo clippy --all-targets --all-features --locked -- -D warnings`
- `npm run format:check`、`npm run lint`、`npx tsc -b --pretty false`、`npm test`、覆盖率、`npm run build`
- 依赖/secret 扫描、Codemap scan/render、SonarScanner/API 查询
- 隔离 `ALTER_DATA_DIR_SUFFIX`、日志目录和备用端口的 daemon/API smoke
- `git diff --check`、`git diff --stat`、`git status --short`

## 返回格式

- P0/P1/P2 完成矩阵与关键实现说明。
- 实际测试命令、结果和剩余限制。
- 运行态、Git 分支、提交 SHA、远端和推送结果。
