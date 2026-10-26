# 审核 review-2026-10-26-v2：桌面客户端清单（0.3.2）

来源：外部审核原文 `review-2026-10-26-v2.md`（对 gui-gate v0.10.2 acaba696 + gui-pi-desktop v0.3.1 8d3eb86）。本清单只覆盖**客户端**相关条目：第 5 节全部、第 4 节客户端部分（四种弹窗的展示内容、人的时间计量）、第 7 节桌面打包与配置部分、第 6 节“53.7% 已过时”。门（gate）侧的条目由 gui-gate v0.10.3 的两路改动处理，本仓库只做向前兼容的适配，GATE_REF 不变（仍是 v0.10.2 acaba69），不打 tag、不发版。

状态：**已改** = 代码 / 文档已改并有测试；**部分** = 客户端能做的做了，其余写明原因；**不在客户端** = 属于 gate 或研究工作，写明归属；**待真机** = 需要真实 Windows / macOS / Linux 桌面或真模型，本轮按要求不做。

测试：扩展 `node --test test/*.test.ts` 90/90（新增 test/v032.test.ts 19 个、facts 5 个）；`cargo test -p gpd-core` 66 + 3（新增 4 个）；`cargo check -p gui-pi-desktop`（含 / 不含 test-hooks）通过。没有真实模型、没有在线测试。

## 第 5 节 观察与上下文

| # | 审核意见 | 状态 | 改了什么 / 原因 | 证据 |
|---|---|---|---|---|
| 5.1 | TaskFacts.setRequest 限 4000 字、已有 request 就拒绝更新：用户后续更正跨压缩保不住 | 已改 | `facts.ts`：用户消息序列 `messages[{v, source, text, at, task_v?}]`，首个请求和每条更正都记（来源 initial / task_file / input / session），同文只记一次；权威记录不截短，去掉 4000 上限和“只存第一次”；旧会话数据（只有 request）读成第 1 条 | test/facts.test.ts “every user message is kept verbatim…”、“old saved facts…”；v032 “every user correction is recorded…” |
| 5.2 | 压缩回退只取首条用户消息前 2000 字 | 已改 | `session_before_compact`：分支上所有 user 消息全文补进序列（再读一次任务文件）；摘要按总长 24000 字预算展示，超出时首条和最新三条全文、中间的截到 2000 字并写明“其余保存在任务记录里”（只截展示，不截记录） | v032 “compaction fallback…”；facts “rendering may shorten the middle…” |
| 5.3 | 带版本和来源的用户消息序列；展示可截短，权威记录不截短 | 已改 | 同 5.1/5.2；另外扩展**只读**壳的可信任务文件（gui-pi-task/1）同步版本号，静态检查“扩展从不写任务文件”仍通过 | facts “the shell's task file parses…”；env.test.ts 静态检查 |
| 5.4 | 确定性压缩可能丢过程信息：失败方法、未完成子目标、临时约束 | 已改 | 新 `progress.ts`：从工具结果和门的结构化状态自动提取四类——最近失败方法、未验证假设、未完成子目标 / 操作、临时约束（含 gate v0.10.3 的 `constraints`），存会话、压缩时原文写进摘要；不依赖模型 note_fact | v032 “process state is extracted from tool results…”、“postconditions…” |
| 5.5 | facts 区分模型备注与工具证据；check_done 说没缺口 ≠ 环境证明成功 | 已改 | 事实三类 `tool_evidence` / `agent_note` / `check` 分开列；check_done 的“没缺口”记为 check（措辞写明“不是环境证明”），同时进“未验证”；gate v0.10.3 `postconditions` 里 met:true 才算工具证据；系统提示词加一句“check_done 的 no gaps 是模型的检查，不是证明” | facts “agent notes, tool evidence and checks render apart”；v032 |
| 5.6 | gate_next_step 用虚构 token click 调动作模型，分布不同 | 不在客户端 | 这是 gate 内部（server.rs 的 next-step 决策）的实现；客户端只消费 `{observe, extra_reason}`，仍视为建议、不授予任何东西。由 gate v0.10.3 处理 | — |
| 5.7 | 自动观察与下一轮前观察共用新鲜度判断，避免刚附完树又读一遍 | 已改 | 新 `observe.ts` `ObsTracker`：动作结果自带的观察、模型读的窗口、next-step 附读共用一个判断（之后没有动作、≤10 s、要截图时须带截图）；新鲜就不再读，记 `skipped:"fresh"`；之后有不带观察的动作就变旧 | v032 “the next-step read is skipped…”、“a screenshot request is not satisfied…” |
| 5.8 | diff 链太长时重新生成完整锚点 | 已改 | 某窗口自上次完整树后 diff 超过 6 个 → 下一轮前客户端自己 `get_window_state` 读整棵树（gate 回完整树并重置基底），即使 gate 说 none 或没有 gate_next_step | v032 “end to end: a long diff chain…” |
| 5.9 | 基底缺失时客户端主动补读，比只提示模型更稳 | 已改 | diff 的基底从没见过或已被压缩掉 → 同样主动补读整棵树；压缩之后、人做过一步之后也补读（后者再附窗口列表）；原有的“基底不在上下文”提示保留 | v032 “anchors…”、“a diff whose base is missing…”、“done_by_person…” |
| 5.10 | 图片策略是“3 个窗口最新图＋3 张固定图＋最近召回图”，不是总计 3 张、也不是每窗口 3 张 | 已改（文档） | design.md §4.4 第 4 条写准：上限 3＋3＋1，并说明两种常见误读 | design.md §4.4 |
| 5.11 | 是否过多应看主模型延迟 | 已改 | 新 `latency.ts`：每次主模型请求记首 token 时间、总时间、上下文图片数、input/output/cache_read，写 `<data>/logs/model.jsonl`（gui-pi-latency/1），测不到记 null | v032 “latency: …” 两个测试 |
| 5.12 | JPEG q60 全尺寸保持 | 不改（按审核） | 维持现状，文档写明 | design.md §4.4 |
| 5.13 | 验证：关键约束跨两次压缩、重复观察、召回次数、首 token、成功率 | 待真机 / 真模型 | 本轮不做真实模型测试；所需数据（latency、nextstep skipped/anchor、recall 记录）已能记录 | — |

## 第 4 节 真人交接（客户端部分）

| # | 审核意见 | 状态 | 改了什么 / 原因 | 证据 |
|---|---|---|---|---|
| 4.1 | 批准：显示具体对象、范围、后果；只批准这个效果 | 已改 | `human::view` approve 增加 `only`：“允许只批准这一个效果：<对象>（<范围>），不批准别的目标、内容、之后步骤或重复”；gate 没给的对象 / 范围 / 后果列在 `missing`，弹窗写“gate 没说：…，不确定就拒绝”，不编造 | gpd-core `popup_contents_per_type_review_v2` |
| 4.2 | 澄清：少量有实际差别的选项 | 已改 | 最多 4 个选项，按值去重（大小写 / 空格不同算同一个）；多出来的允许自由输入；每个选项仍显示“选它之后会怎样” | 同上 |
| 4.3 | 接管：最少步骤，完成后重读受影响状态，禁止自动重放原动作 | 已改 | 弹窗最多列 5 步（多的计数）；说明“完成后 agent 先重读，不重复不重放”；扩展：done_by_person 后同工具同参数被拦（原有），下一轮前**客户端自己**完整重读受影响窗口并附窗口列表（新），模型提示写明“别以别的方式重放” | gpd-core 同上；v032 “done_by_person: the affected window…” |
| 4.4 | 求助：展示已尝试过什么，让用户选提示或代做 | 已改 | `tried` 用 gate 的清单；gate 没给时扩展把自己记录的最近失败方法作为 `client_tried` 带上，弹窗标明“来自 agent 日志”；说明两个选择的后果 | gpd-core 同上；v032 “a help request carries the client's record…” |
| 4.5 | 人的负担：等待时间不能记成主动操作时间；测不到记缺失或上界 | 已改 | human.jsonl 升到 gui-pi-human/2：每问 `wait_s`（弹窗时间）与 `active_s`（只来自弹窗里的交互证据：按键 / 点击 / 输入 / 滚动 / 指针移动；只有回答那一下、没有交互、接管在别的窗口做时记 null）+ `evidence`；汇总 `person_active_s`（有证据之和）、`person_active_missing`、`person_active_upper_s`（上界）、`question_wait_s`、`no_question_s`；保留 `secs`（= wait_s）兼容旧读者 | gpd-core `waiting_is_not_operating_time`、`metrics_go_to_the_log_not_the_screen` |
| 4.6 | 每个请求统一记录（请求 ID、任务版本、效果、类型、截止、唯一终态），过期 / 断连 / 迟到 / reset 同一收尾 | 部分 | 客户端已有：每个请求 id 只答一次、过期未显示的也记一行（B8）、所有非回答都有原因；“唯一终态 + 任务版本 + 效果”的账本属于 gate（human.rs 的请求生命周期），由 gate v0.10.3 处理 | 既有 human 测试 |
| 4.7 | 安全必需的批准不能因“问人收益低”跳过；可选澄清 / 求助做价值判断 | 不在客户端 | 是否发起问人由 gate 决定，客户端只展示 | — |
| 4.8 | 验证：打断次数、重复率、无回答率、等待、主动时间、接管完成率、交还后重复动作率、主观打扰；交还测试包括切窗口、改文件、改焦点 | 待真机 | 计数和时间已写进 human.jsonl；交还后的重复动作被客户端拦下（单元测试）；真人试点本轮不做 | — |

## 第 7 节 辅助角色与桌面产品（打包与配置部分）

| # | 审核意见 | 状态 | 改了什么 / 原因 | 证据 |
|---|---|---|---|---|
| 7.1 | role_enabled 对 feasibility / verifier / judge / grounder 直接 true；配置“显式关闭高于任何组开关和端点存在” | 已改（客户端配置） | `Settings::role_enabled`：关闭最高优先；`launch::gate_env` 对每个角色都发显式 `GUI_GATE_ROLE_<NAME>_ENABLE=0|1`，关闭的角色**不发任何端点变量**，从不发组开关 `GUI_GATE_ROLES_ENABLE`；所以在 v0.10.2 上关闭也成立（没有端点角色不存在），v0.10.3 的 gate 侧修正另由 gate 完成 | gpd-core `explicit_off_wins_and_client_side_roles_are_off_in_the_gate_by_default`、`keys_only_in_env…` |
| 7.2 | planner、search、compact 的门内版本在证明净收益前关闭 | 已改 | 新设置 `in_gate_roles`（默认空）：这三个角色即使配置了端点，没勾“在门内运行”也发 `_ENABLE=0` 且不发端点；设置页高级区每个有勾选框和说明 | 同上；settings.js / settings.html |
| 7.3 | 工具返回以结构化状态为准：统一执行状态、续接 ID、已完成批量索引、后置条件证据 | 已改（向前兼容） | `status.ts` 解析 gate v0.10.3：`done` 视同已执行；`continuation_id`（别名 resume_id / continue_id / state.*）；`completed`（0 起，`index_base:1` 支持；别名 completed_indices / batch.completed）→ 写明哪些步骤没跑；`postconditions`（met true/false/null）；任何字段缺失按旧行为；unknown 时模型看到“RESULT UNKNOWN … Do not resend it directly”，时间线显示“结果未知 - result unknown … not resent” | v032 “v0.10.3: …” 三个测试；rpc `statuses_and_person_outcomes_in_the_timeline` |
| 7.4 | Linux 明确 X11 支持范围，测试 AT-SPI / DBus 丢失后的行为 | 已改 | design §6.8.0 写明范围（X11/Xorg，Wayland 不承诺）；自检把失效的 `unix:path=` 总线地址判为不可达、dbus-send 对给定地址 5 s 超时；任务中途丢失：`platform.ts` 识别、附固定说明、连续两次后拦下 GUI 动作直到读窗口成功，时间线显示 | gpd-core `a_lost_dbus_or_atspi_bus_reads_as_unreachable_and_blocks_start`；v032 “platform: …” 三个测试 |
| 7.5 | Windows 普通权限，UAC、锁屏、RDP 断开转真人；不提供 run_command 合理 | 已改（客户端）/ 待真机 | NSIS currentUser、普通权限（原有），管理员运行 warn；`platform.ts` 识别 UAC 安全桌面 / 锁屏 / 无交互桌面 / 管理员窗口 → 提示请人接管，重复时拦下；run_command 在 Windows 不提供（原有）。驱动在真机上的实际错误措辞未验证 | v032；design §6.8.1 |
| 7.6 | macOS 验证发布包签名、权限授予、升级后权限保持 | 待真机 | design §6.8.2 列为待真机验证项（CI 只做了构建和冒烟） | design §6.8.2 |
| 7.7 | 清楚列出缓存、图片、日志、钥匙串、用户文档的卸载边界 | 已改（文档） | design §7.1 表格：每类数据的位置和卸载时去留（Linux 脚本删应用目录，钥匙串只有 `--keys` 才删，用户文档从不删）；Windows / macOS 的数据位置与“卸载器对应用数据的处理未在真机验证”写明 | design §7.1；既有 scripts.rs 测试 |
| 7.8 | 包内校验和解决完整性，来源真实性需要签名或可信发布链 | 已改（文档） | design §7.2 首条：完整性 ≠ 来源真实性，目前包未签名，签名发布是未来工作 | design §7.2 |
| 7.9 | 四层结构保留；角色消融、check_done 总截止时间 | 不在客户端 | 结构不变；角色消融和 verifier / check_done 总截止属于 gate 与评测 | — |
| 7.10 | 打包测试用安装后的产物（停止、崩溃、断连、升级、回滚、卸载），CI 不能代替真机 | 待真机 | 现有 CI 冒烟不变；本轮不做真机 | — |

## 第 6 节 评测口径

| # | 审核意见 | 状态 | 改了什么 | 证据 |
|---|---|---|---|---|
| 6.1 | 桌面设计文档里 53.7% 部分结果已过时 | 已改 | design §1、§9 改为：185/361 = 51.25%（计划口径）、185/332 = 55.72%（有效口径）、28 个 infra、1 个未运行；自定义原生环境、不能和官方排行榜比较；这次没有对照，不能证明门控贡献。数字与 gui-gate README / docs/failure-analysis-v099.md 核对一致 | design §1、§9（`grep 53.7` 只剩“已过时”的说明） |
| 6.2 | 其余离线 / 在线评测建议（分组、跨 run、基线、试点） | 不在客户端 | 属于 gui-gate bench 与研究工作 | — |

## 其他

- 版本号 0.3.2：desktop/core、src-tauri 的 Cargo.toml 与 Cargo.lock、tauri.conf.json、packages/gui-pi/package.json、扩展 `VERSION`。
- 需要 gate 侧配合的一处：gui-gate `bench/human_pilot.py` 目前只读 `schema == "gui-pi-human/1"`；0.3.2 写 gui-pi-human/2（仍带 `secs` = wait_s）。gate 侧需要接受 /2 并改用 `wait_s` / `active_s`，否则它会跳过 0.3.2 的人员记录。
