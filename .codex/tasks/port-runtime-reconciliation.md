# 端口扫描与项目运行态校准

## 背景

Windows `netstat -ano` 会为正常的 `TIME_WAIT` 连接返回 PID 0，当前项目页的二次校验错误地将整个端口列表判为无效。与此同时，Niuma Studio（8001/8765）和 Novel Writer（5174）由 Alter 外部进程启动，导致实际运行状态与 Alter 卡片不一致。

## 目标

- 允许端口扫描结果中的 PID 0，但继续禁止其参与项目归属或停止操作。
- 将 Niuma 主服务、Niuma 发布 Worker 和 Novel Writer 安全迁移为 Alter 托管进程。
- 验证项目卡片、监听端口、健康接口和实际进程祖先链一致。

## 允许范围

- 修改 Alter 前端端口校验及相关测试。
- 通过 Alter API 更新 Niuma、Novel Writer 的运行态和项目元数据。
- 在已验证身份后停止仅属于上述项目的外部进程树。
- 备份和恢复 `%APPDATA%\alter-pm2\state.json`、`projects.json`。

## 禁止范围

- 不修改 Niuma Studio、Novel Writer 的生产源码、数据库或 `.env`。
- 不重启 Alter daemon，不启停 CatCare Hub、GroupBrief、RunDock 或其他项目。
- 不按端口、名称或目录前缀盲目接管或结束进程。
- 不读取、复制或记录任何密钥、Token、Cookie 或 `.env` 内容。

## 已确定实现要求

- Niuma 项目包含主服务和发布 Worker 两个 Alter 成员，主网页端口为 8001。
- Novel Writer 使用现有登记项，主网页端口为 5174。
- Worker 直接运行项目现有 Python 入口，并由应用自身读取被忽略提交的配置。
- 任一身份或端口检查发生变化时停止；接管失败时恢复元数据和原运行方式。

## 验收标准

- PID 0/TIME_WAIT 数据通过校验，负 PID 和错误类型仍被拒绝。
- 8001、8765、5174 各有且仅有一个监听者，祖先链均归属 Alter daemon。
- Niuma 显示 `running 2/2`，Novel Writer 显示 `running 1/1`。
- Niuma 健康、调度器和 Worker 可用；Novel Writer 5174 返回 HTTP 200。
- 5173 项目页不再显示“端口扫描返回了无效数据”。

## 测试命令

- `npm exec vitest run src/lib/processWeb.test.ts src/lib/schemas.test.ts src/pages/ProjectsPage.test.tsx`
- `npm test`
- `npm run typecheck`
- `npm run lint`
- `npm run build`

## 返回格式

报告修改文件、测试结果、运行态端口/PID/祖先链、项目卡片状态、备份路径、提交哈希、分支和推送结果。
