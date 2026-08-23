# Alter 项目主网页端口任务

## 背景

Alter 当前将项目进程及其子进程的全部监听 TCP 端口作为网页候选。AI JobPilot 因此同时显示 `6866`、`8888`、`35729`、`61135` 和 `63119`，但其中只有 `6866` 是日常前端入口。

## 目标

- 为项目元数据增加明确的主网页端口。
- AI JobPilot 的“打开网页”按钮只直达 `http://127.0.0.1:6866/`，不再弹出端口选择菜单。
- 全部监听端口继续保留在展开后的技术组件详情中，方便排错。

## 允许修改范围

- `src/models/project.rs`、`src/config/project_store.rs`、`src/api/routes/projects.rs`。
- `web-ui/src/types.ts`、`web-ui/src/pages/ProjectsPage.tsx`。
- `web-ui/src/components/WebPortButton.tsx` 及对应测试。
- 本任务文件、构建产物、本机 Alter 安装备份及项目元数据备份。

## 禁止修改范围

- 不修改监听端口发现和进程归属规则。
- 不修改或重命名 AI JobPilot 的 Backend/Frontend 进程。
- 不重置、stash、覆盖现有工作区修改；不提交、推送、发布或部署。
- 不读取或输出 secrets，不改动其他项目的名称、备注、分类和启停状态。

## 已确定实现要求

- `ProjectRecord` 兼容旧数据地保存可选 `web_port`，`ProjectInfo` 返回该字段，`ProjectPatch` 可设置有效的非零 TCP 端口。
- 项目配置了 `web_port` 时，只有该端口正在监听才显示主按钮；按钮文字显示“打开网页：端口”，点击直接新开页面。
- 未配置 `web_port` 的项目继续保持现有单端口直开、多端口选择行为，避免改变其他项目。
- 展开的技术组件按现有 PID/祖先进程归属显示去重、排序后的监听端口；主按钮不得把技术端口当网页入口。
- 备份 `%APPDATA%/alter-pm2/projects.json` 后，通过正式项目更新 API 仅将 AI JobPilot 的 `web_port` 设置为 `6866`。

## 验收标准

- AI JobPilot 顶部只显示“打开网页：6866”，其链接为 `http://127.0.0.1:6866/`，不存在端口选择菜单。
- `8888`、`35729`、`61135`、`63119` 只出现在展开后的技术详情/端口页面，不出现在主网页按钮中。
- `web_port` 缺失的旧项目元数据可正常加载；其他项目行为和元数据不变。
- API 拒绝端口 `0`；项目保存后重启仍保留 `6866`。

## 测试命令

- `npm test -- --run`（`web-ui`）
- `npm run build`（`web-ui`）
- `cargo test --target x86_64-pc-windows-gnu`
- `cargo build --release --target x86_64-pc-windows-gnu`
- 当前本机 `/processes` 页面浏览器烟雾检查与点击目标核对。

## 返回格式

- 修改摘要、测试结果、安装与元数据备份、实机验收结果、剩余风险。
- 明确区分本次失败与仓库既有基线问题。
