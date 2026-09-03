# RunDock 第二轮定点修复执行任务

## 背景

`PROJECT_REAUDIT.md` 复检发现 6 个 HIGH，以及 PATCH/checkpoint 和质量门禁缺口。用户已批准按既定计划实施；只关闭这些问题，不做大文件全面拆分、依赖追新或 Sonar smell 清零。

## 目标

1. 关闭 Vite LAN 暴露、双重重启、首次加载、Linux 端口解析回归。
2. 修复 Cron retained process tree 与 ProcessTreeGuard 附加失败的清理所有权。
3. 为进程/项目 PATCH 提供 absent/null/value 语义；保证 Telegram checkpoint 单调提交原子性。
4. 恢复本地 Sonar、desktop-shell、Windows 安装包和 Linux deb 的可信门禁。
5. 重新验证 Codemap，生成 `PROJECT_REAUDIT_REMEDIATION.md`。

## 允许修改范围

- `web-ui/vite.config.ts`、相关前端组件/hooks/types/tests。
- `src/api/routes/ports.rs`、`src/process/{manager,runner,tree,hooks}.rs`。
- `src/models/{api_types,project}.rs`、相关进程/项目路由和前端 API 类型。
- `src/config/{atomic_file,telegram_checkpoint}.rs` 及对应测试。
- `.github/workflows/{quality,release}.yml`、`scripts/` 下新增的本地 Sonar/安装 smoke 脚本。
- `.gitattributes`、`sonar-project.properties`、`.codemap/` 和整改关闭报告。
- 为上述行为新增或扩展的测试文件。

## 禁止修改范围

- 不做微服务、数据库、状态格式、端口或外部 Provider 迁移。
- 不全面拆分 ProcessManager/App/Pages/AI/Terminal。
- 不执行强推、rebase 已推送历史、自动合并、部署或系统级安装。
- 不读取、写入或提交 Token、密码、Cookie、`.env` 或浏览器数据。
- 不重启当前项目运行时，不修改生产数据库或远程服务器。

## 已确定实现要求

- Vite 默认仅 `127.0.0.1`；API/WS 代理也保持 loopback。
- Env modal 自己 await 唯一 restart，父组件只接收成功通知并刷新。
- 不改变 `useSingleFlightPoll.enabled` 语义；调用方负责一次初始加载。
- Unix 端口输出携带 `Ss`/`Netstat` 格式标记，使用独立 parser。
- Cron 只有在完整树清理成功后才能 Sleeping/重排；失败则 Errored 并停 scheduler。
- PATCH：缺失保留、null 清空、具体值覆盖；文件/URL/方法/响应结构不变。
- checkpoint 的读、单调校验、写入使用同一文件操作锁。
- Sonar 使用本机服务和会话 Token，GitHub-hosted runner 不再强制访问本机。
- desktop-shell 在 Windows MSVC CI 跑 fmt/clippy/test/build；正式安装包和 deb 做真实生命周期 smoke。

## 验收标准

- 6 个 HIGH 关闭，R1-5 完成，受影响 Codemap 模块无新 HIGH。
- 根 Rust fmt/check/test/clippy/audit/coverage 通过。
- 前端 format/lint/typecheck/test/coverage/build/audit 通过，覆盖率不低于复检基线。
- desktop-shell MSVC 门禁和 Windows 安装包 smoke 通过。
- Linux deb 安装、健康、升级、卸载 smoke 通过。
- 本地 Sonar 完成认证扫描并如实记录 gate；若缺 Token，明确阻塞且不伪造指标。
- `git diff --check` 通过，无范围外修改、秘密、TODO/debug 或临时文件。

## 测试命令

- `cargo fmt --all -- --check`
- `cargo check --all-targets --locked`
- `cargo test --all-targets --locked`
- `cargo clippy --all-targets --all-features --locked -- -D warnings`
- `npm run format:check && npm run lint && npm run typecheck && npm run test:coverage && npm run build && npm audit --audit-level=high`（在 `web-ui`）
- desktop-shell 的 fmt/test/clippy/release build（Windows MSVC）。
- Codemap 官方 scan/apply-audit/render/stamp 流程。

## 返回格式

最终报告：问题关闭矩阵、关键文件、测试/覆盖率、Sonar/Codemap、Git 初始与最终状态、提交 SHA、远端 SHA、Push、PR、CI、是否合并及剩余阻塞。
