# RunDock / Alter 全项目工程审计任务

## 背景

本轮联合使用 Codemap、Code Overhaul Review 与 SonarQube，对当前仓库做一次完整但只读的工程体检。目标是解释系统为何能运行、哪些部分可靠、哪些风险依赖偶然条件，并给出分轮整改路线图。

## 目标

- 建立功能模块级架构地图、依赖关系、核心请求与进程生命周期主线。
- 审计架构、业务逻辑、数据与状态、稳定性、测试、安全、性能、依赖和可维护性。
- 运行现有的安全验证命令并收集 SonarQube 客观指标。
- 交叉验证三类证据，最终生成仓库根目录 `PROJECT_AUDIT.md`。

## 允许修改范围

- `.codemap/**`：Codemap 配置、状态与生成报告。
- `.codex/tasks/full-project-audit.md`：本任务边界与验收标准。
- `PROJECT_AUDIT.md`：最终联合审计报告。
- SonarScanner 所需的临时参数或仓库外临时文件；不得把令牌或密钥写入仓库。

## 禁止修改范围

- `src/**`、`web-ui/src/**`、`tests/**` 及任何生产代码、测试代码与业务配置。
- `Cargo.toml`、`Cargo.lock`、`web-ui/package.json`、锁文件和依赖版本。
- 数据库、用户状态目录、进程注册信息、远程服务与生产环境。
- 不修复、不重构、不删除 Dead/Legacy Code，不自动处理 Sonar 问题。

## 已确定实现要求

- Codemap 使用中文，输出到 `.codemap/`，`modules.json` 为唯一事实源，HTML/Markdown 由脚本生成。
- 每个 Codemap 功能模块必须由独立只读子任务按固定标准评分；主控只做结构划分与跨模块综合。
- Code Overhaul 采用 FULL AUDIT 模式；不创建 Beads、不进入整改。
- SonarQube 使用本机共享实例（若实时验证可用），扫描令牌只在运行时使用并在结束后撤销。
- 任何测试或扫描不得启动/停止 Alter 管理的项目，不得读写真实用户数据或项目外密钥。

## 验收标准

- `.codemap/modules.json`、`.codemap/codemap.html`、`.codemap/codemap.md` 一致且可重新生成。
- 已记录构建、测试、lint、typecheck、coverage、依赖检查和 SonarQube 的实际结果或明确阻塞。
- `PROJECT_AUDIT.md` 包含用户要求的全部章节、文件行号证据、优先级、Top 10、删除候选、暂不应动区域和分轮路线图。
- 报告明确区分客观扫描、源码审查、推断、未验证项，不把 Quality Gate 或测试通过等同于生产就绪。
- 完成报告后停止，不实施任何整改。

## 计划验证命令

- `cargo fmt --all -- --check`
- `cargo check --all-targets`
- `cargo test --all-targets`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `npm run lint`（`web-ui`）
- `npm run format:check`（`web-ui`）
- `npm run test -- --coverage` 或现有覆盖率命令（`web-ui`）
- `npm run build`（`web-ui`）
- SonarScanner + SonarQube API 指标查询

## 返回格式

- 主产物：`PROJECT_AUDIT.md`
- 辅助产物：`.codemap/modules.json`、`.codemap/codemap.html`、`.codemap/codemap.md`
- 最终回复仅汇报结论、验证证据、产物路径和 Git 同步状态，不附带整改实现。
