# Changelog

## 0.3.1 — 2026-10-26（打包 gui-gate v0.10.2；之后只开发桌面版）

gui-gate v0.10.2 是 gui-gate 作为独立 MCP 的最后一版（三个工作流的 `GATE_REF` 都指向它）；门代码今后随本仓库演进（design.md §5.4）。
本版只做了单元 / 脚本验证，没有真实模型、没有在线测试（docs/test-log.md）。

适配 gate v0.10.2 的协议（扩展 gui-pi + 桌面壳，每项有单元测试：packages/gui-pi/test/gate0102.test.ts、gpd-core rpc / human 测试）：
- ask_help 的 `human_choice: "another_way"`（人选“换个方法 / 给提示”）是回答而不是拒绝：不记为被拒的效果，提示模型不要重发同一动作、换一种明显不同的方法；时间线显示“person: try another way”。
- 新 `human_outcome`：`task_changed`（回答是针对旧任务的，什么都不批准；当前任务仍需要时可重发、会再问人）、`void` / 被 reset 作废后超时、`late`（超时后才回答）、`answer_rejected`（gate 以 `Invalid requestState` 拒收答案，MRTR 客户端转成结构化的 blocked）、`not_asked_again`、`expired`、`channel_unavailable`：都不是批准，只有 declined 是“人说不”，其余显示为“没有回答，步骤未执行”。未知的新词仍按拒绝处理。
- tier `write_args`（读工具带了写文件参数被拒）：提示模型去掉目标参数再调；时间线说明“读工具不能写文件”。
- 冻结（tier `freeze`，包括 Stop 发出 `gate_reset` 后问题仍在人面前）：不算拒绝，状态 `waiting_person`，主窗口显示“等待你回答 / waiting for your answer”，人回答后恢复。
- 拒绝只从结构化结果判断（`gate.decision` / `gate.tier` / `executed`），不再看审核原话（gate 不再返回；`gate_report` 只给 configured / none）；扩展里没有依赖审核模型名或原话的代码。
- gate 把看起来像 gate 行的应用 / 网页 / 终端文本改写成 `(app text) ...`：观察标记只在行首识别（`markerAt`），伪造行不会被当成观察、不会切错上下文。
- 弹窗：键盘默认按钮一律是拒绝 / 不做的那个（Deny、Don't answer、I won't do it、Give a hint；自由文本问题焦点在输入框）；`human::view` 给出 `default: "no"`。
- 发布：推 `v*` tag 时三个工作流把包加到同一个正式 release `vX.Y.Z`，测试钩子版进预发布 `vX.Y.Z-testhooks`（scripts/ci/publish-version.sh；校验和文件带平台后缀 `SHA256SUMS.linux` 等）。main 推送的 `build-<sha>` 预发布照旧。

### 此前未发布的修复（已在 0.3.0 之后的预发布里）

修 0.3.0 在 gui-pi5 实测发现的缺陷（docs/test-log.md）。当时打包的 gui-gate 是 gate main 0cae275 / 6339cf8（未打 tag）。
- B1：`models.json` 的 `apiKey` 改为 `"$GUI_PI_MAIN_KEY"`（Pi 1.4.0 只插值 `$NAME`，裸名字是字面密钥 → 401）；单元测试 + CI 用包内 Pi 的 resolveConfigValue 检查（scripts/check-pi-key.mjs）。
- B2：模型调用失败（`stopReason: "error"`）在时间线显示为错误，任务结束显示“ended with an error … not done”、状态 failed；`--run` 退出码 4。
- B3：AppImage 打包后把 resources 原样放回（linuxdeploy 给其中 ELF 加了 RUNPATH，组件校验失败、Start 被阻）：scripts/appimage-restore-resources.sh，重打 squashfs 并按 SHA256SUMS 校验。
- B4：在 AppImage 里，自检调用的系统工具和所有子进程（Pi → 扩展 → gui-gate → 驱动）用去掉 AppImage 库/模块路径的环境（`launch::system_env`），不再加载包里的旧 libdbus。
- B5：设置页【测试连接】保存时不重绘表单，结果留在页面上。B6：【Save key】不再清掉未保存的 Base URL / 模型名。
- B7（gate）：超时 / 关窗 / 过期的原因进 gate（`human_outcome` timeout / window_closed / expired，不再记成 declined），模型的提示写明“没有回答”。
- B8：弹窗开着时排队过期的问题写进 human.jsonl（`outcome: expired, shown: false`），汇总多 `expired` 计数，时间线也显示；回给 gate 的原因是 `expired`。
- B9：扩展调 `gate_next_step` / `gate_reset` 带客户端标记（`_meta["io.github.cszf1/gui-gate/client"]`）。
- B10：保存设置 / 存密钥后主窗口立即刷新缺项；启动自检完成后不再停在“Still checking…”；`--run` 启动的任务在主窗口显示为运行中。
- B11（gate）：接管 / 求助弹窗正文不再写“press Allow / Deny”。
- gate run_actions：批量里每步（第一步之后）先读窗口，加载中或目标禁用最多等 3 s，仍不行就停下并如实返回；步骤之间新出现的窗口/对话框也停下。

## 0.3.0 — 2026-10-25（打包 gui-gate v0.10.1，27c479f；兼容 v0.10.0）

按外部审阅 §4（问人）、§5（agent 侧）、§7（桌面产品化）补齐剩下的桌面 / 扩展项。新 gate 接口按名字编码，缺失时退回 0.2 行为（design.md §11 列出全部假设）。

### 扩展 gui-pi
- 每次模型请求前调用客户端专用工具 `gate_next_step {window_id}`：observe=tree / tree+screenshot 时扩展自己读聚焦窗口，作为会话里的 custom message 附在下一轮前（参与 diff 链）；extra_reason 时附固定提示；decision_id 写进会话。没有该工具时试调一次后不再调。
- blocked + `next_action: "rethink"`：告诉模型重新生成候选、不要重发；`done_by_person`（及旧门的 `gate.done_by_person`）：同一工具 + 参数在本任务内被拦下，读窗口后继续。
- 截图：每张图有 id、原字节（JPEG q60，不缩放）存进会话目录（0600）；上下文保留每窗口最新一张 + 最多 3 张固定证据图（新工具 `pin_evidence {id, label}`，可 unpin，跨压缩保存）+ 最近一次 `recall_image {id}`。
- 提示词卫生：系统提示词、gate 简报、skills 每条指令只写一处（有重叠检测测试）；skills 只列名字和描述，用新工具 `load_skill` 按需加载；新增 platform-linux / platform-windows / platform-macos。
- 一份计划状态（plan.ts）：删掉 gate 每个结果末尾的 `[plan]` 行；提醒只在里程碑、压缩之后、偏航时。
- 每个 tools/call 的 `_meta` 带 `gui_pi/usage: {input, output, cache_read}`（模型真实用量，同一消息的其余调用为 0 + same_message）。
- 任务通道：有 `GUI_PI_TASK_FILE` 时 gate 用 `--task-file` 启动；`GUI_PI_GATE_ARGS` 里的 `--task` / `--task-file` 和 gate 环境里的 `GUI_GATE_TASK*` 一律丢弃；扩展从不写任务文件。
- 停止：经问人 IPC 的 `watch_stop` 连接收到停止 → 拦下一切调用 → `gate_reset`（有的话）→ 杀掉 gate → 回确认。
- 密钥擦除：扩展经手的密钥在工具结果、details、会话条目、压缩摘要、通知和日志里换成 `***`。
- 测试：假 gate（fake_gate.mjs）+ 模拟 Pi 的端到端测试（next_step、rethink / done_by_person、截图、用量、停止、假密钥泄漏、任务文件）。

### 壳（gpd-core + Tauri）
- 可信任务通道：用户原话（开始任务和运行中每条“Send”消息）写进私有运行目录的任务文件（0600、原子替换、带版本），只能由 `UserWords` 写入；测试证明只有用户输入进文件。
- 停止：通知扩展并等确认（最多 2 秒），然后杀掉 Pi 的整个进程组（Pi 以独立进程组启动）；假子进程测试：停止后不再有动作，新任务是新进程。
- 问人弹窗：一个组件、四种严格结果类型 approve（对象 / 范围 / 后果）、clarify（选项及各自后果）、takeover（最少步骤、输入暂停、完成不重做）、help（已试办法、代做或给提示）；gate 的 answer / ask_help 映射到 clarify / help。
- 问人指标：每题与每任务的询问次数、重复问题、人主动 / 等待秒数、接管步骤、取消 / 超时，写进 `logs/human.jsonl`（schema gui-pi-human/1），界面只多一行时间线。
- Linux 启动自检：X11 / Wayland 说明、AT-SPI 总线、DBus 会话、Secret Service（否则提示仅内存）、WebKitGTK 4.1、glibc ≥ 2.35；结果在设置页，fail 阻止开始。
- 发布卫生：启动时按包内 SHA256SUMS 校验所有组件（驱动签名原样），不符阻止开始；不自动更新，只提示新的正式版本（可关）；`appimage-update.sh`（校验后安装、保留上一版、rollback）和带守卫的 `uninstall.sh`（只删应用自己的目录，从不碰另装的 Cua Driver 和用户数据），均有测试；CI 发布 `SHA256SUMS` 与 `resources.SHA256SUMS`。
- 日志和时间线里的密钥值一律遮蔽（`launch::Redactor`）；假密钥端到端测试：Pi 环境只有主模型密钥，gate 密钥只经一次性 IPC 交接，日志与界面无密钥。

### 文档
- design.md 0.3：指令归属表、skills 按需加载、next_step、截图存取、计划单一来源、四种问人结果与指标、可信任务通道、停止流程、坐标原点与缩放约定、平台说明表（Linux / Windows / macOS）、卸载范围、发布与更新、对新 gate 接口的假设；威胁模型改为“审核模型是一道必要的防线，但不是充分的防线”。

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
