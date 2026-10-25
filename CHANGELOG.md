# Changelog

## 0.2.0 — 2026-10-25 (gui-gate v0.10.0)

按外部审阅（桌面版、上下文、问人部分）修订。兼容当前 gui-gate（status executed / blocked / partial / done）和正在改的新 gate（diff 结构、status unknown / input_required、human_outcome、移除 gate_confirm、gate_task 不再授权）。

### 扩展 gui-pi
- 上下文：按 `structuredContent.diff.base_snapshot_id`（旧 gate 用文本 “changed since snapshot X”）保留每个窗口当前观察及其整条基底链；基底不在上下文的 diff 保持完整并标注；`removed` / `reordered` 行写成显式说明；截图每窗口最新一张、最多 3 个窗口。
- 任务事实（用户原话、带来源的事实、人的回答与拒绝、被拒效果、产生的文件）由程序原文保存，`appendEntry` 持久化，压缩摘要原样带上；新工具 `note_fact`。
- 计划提醒只在里程碑（步骤完成 / 失败、被拒之后、连续 8 个动作无进展），不再每轮附加。
- 状态：unknown → 告诉模型结果未知并要求先重读，之后 GUI 动作在读窗口前被拦下，从不自动重试；partial → 写明跑了几步；input_required → 未执行；human_outcome（decline / cancel / timeout / closed / unavailable）一律按拒绝。
- gate 调用前已退出 → 未执行；调用中退出 → unknown；gate 退出会通知壳。
- 不注册 `gate_confirm` / `gate_task` / `gate_config`；拒绝原因随 ElicitResult `_meta` 传给 gate。
- gate 子进程环境按白名单构造：门的设置与密钥从壳的 IPC 一次性取得；不含 Pi 的主模型密钥、问人 token、`GUI_GATE_ALLOW_CONFIG`；`--allow-config` 永远不传。

### 提示词与 skills
- 删除 gate_task / 确认码相关说法；明确模型永远不能批准自己的动作；说明各状态、人的各种拒绝、diff 行删除/重排、窗口相对坐标、note_fact。

### 壳（gpd-core + Tauri）
- 设置：主模型和审核模型必填（审核模型无法关闭，设置文件多出字段整份拒收）；7 个角色移到默认折叠的“高级”，可选，未填字段继承主模型；Base URL 严格校验（https，仅本机允许 http，拒绝前缀伪装和 URL 内凭据）；模型名必填；每个模型的【测试连接】由 Rust 侧发请求，密钥只在请求头里，错误信息遮蔽密钥。
- 密钥：只在钥匙串，或无钥匙串时只在本次运行内存（界面提示）；设置文件写入前检查不含任何密钥（测试扫描文件）；Pi 与 gate 的环境分开，启动器从不设置 `GUI_GATE_ALLOW_CONFIG` / `--allow-config`（有测试）。
- 问人：结果原因（拒绝 / 取消 / 超时 / 窗口关闭 / 无人可问）进主窗口时间线，均按拒绝；关闭弹窗 = 取消；退出应用、停止任务、agent 进程结束时待答问题取消；每个请求 id、每个弹窗只认第一次回答。
- 健壮性：进程监管（Finished / Stopped / Crashed / Disconnected），崩溃时显示退出码与 stderr 并提供重新开始；`agent_settled` 才算结束；Pi 的对话框自动回 cancelled；gate 退出显示。假子进程测试。
- HiDPI：壳不换算 agent 坐标；接管弹窗按显示器 scale factor 摆放。

### 文档
- design.md：Pi 1.4.0、新状态、人的结果、上下文规则、进程生命周期、坐标与 HiDPI、威胁模型（防合作型 agent 误操作，不防恶意 agent；OS 隔离为后续工作）。README：构建步骤（stage-resources.sh、CI）。

## 0.1.0 — 2026-10-25
- 设计 v0.1；扩展原型（MCP 2026-07-28 + MRTR、上下文锚点、问人桥）；Tauri 壳 + gpd-core；Pi 1.4.0 锁定；Linux CI。
