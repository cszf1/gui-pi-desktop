# gui-pi-desktop 设计文档

版本 0.3（2026-10-25，按外部审阅 §4/§5/§7 修订；对应 gui-gate v0.10.1 的新接口，兼容 v0.10.0；变更见 CHANGELOG.md）。范围：桌面版 computer-use agent，Linux 先做，然后 Windows，最后 macOS。

一句话：**Tauri 壳 + Pi 1.4.0（@earendil-works/pi-coding-agent，RPC 模式，packaging/pi 锁定）+ Pi 扩展 gui-pi（TypeScript）+ gui-gate（Rust 门控）+ cua-driver 0.38.0（固定版本）**，一个安装包装好就能用。

---

## 1. 目标与不做的事

要做到：

1. 用户装一个包，打开应用，在设置页填好主模型和审核模型，输入任务，agent 在本机桌面上操作应用。
2. 每一步 GUI 动作都先过 gui-gate。门的安全核心（风险规则、授权、独立审核模型、执行）只在 Rust 里，agent 无法放行自己。防护对象是**合作但会犯错的 agent**，不是恶意 agent（见 §12 威胁模型）。
3. 需要人的时候弹原生窗口：批准/拒绝、回答问题、接管（你来做完再点继续）、卡住求助。拒绝、取消、超时、关窗、无人可问一律按拒绝，并记录原因。
4. 门的每一种返回动作都能真实触发，都有端到端测试。
5. 和基线比：v0.9.9 在 gui-pi3 上 OSWorld-Verified 部分结果 137/255 有效题成功（53.7%）。

不做：

- 不改 gui-gate 的风险规则和预测器本身（command.rs、authz.rs、decide.rs 的判断逻辑不动），本项目只改**人机通道和协议**（human.rs、MCP 2026-07-28 的问人格式）。
- 不自动升级（应用和驱动都不）。cua-driver 固定 0.38.0，打包时用 SHA256SUMS 和 scripts/cua-driver.lock 的哈希校验，签名文件原样随包；换版本只能随应用发版。应用只显示“有新版本”提示（§7.2）。
- Pi 自带的 bash / read / edit / write 工具在桌面 agent 里关掉（见 §4.6），命令只能走门里的 run_command。

---

## 2. 总体架构

```
┌────────────────────────── Tauri 应用（Rust） ──────────────────────────┐
│  主窗口：任务输入、过程流、停止按钮                                       │
│  设置页：主模型 / 审核模型（必填）/ 高级：7 个可选角色 / 问人超时         │
│  问人弹窗：一个组件、四种结果 approve / clarify / takeover / help          │
│  可信任务通道：用户原话 → 任务文件（0600）→ gate --task-file              │
│  启动自检（Linux）、资源 SHA256SUMS 校验、停止（gate_reset + 杀进程组）   │
│  钥匙串：keyring（Linux Secret Service / Windows 凭据管理器 / macOS 钥匙串）│
│  human IPC 服务：Unix socket（Windows 127.0.0.1），一次一 token          │
│  进程监管：Pi 崩溃 / 断连 / 停止 → 取消待答问题、显示、可重启             │
└───────────┬────────────────────────────────────────────────────────────┘
            │ 子进程 stdin/stdout：pi --mode rpc（JSON 行，只按 LF 分帧）
            │ Pi 的环境：白名单会话变量 + 主模型密钥；没有任何 GUI_GATE_* 变量
            │ 门的设置与密钥：经 human IPC 一次性交给扩展，只进 gate 子进程环境
┌───────────▼──────────── Pi 1.4.0（内置 node） ──────────────────────────┐
│  系统提示词 prompts/system.md，skills/*                                 │
│  扩展 gui-pi（TS）：                                                     │
│    · MCP 客户端（2026-07-28：逐请求 _meta + MRTR 问人）                  │
│    · 把门的工具注册成 Pi 工具                                            │
│    · 每轮前问 gate_next_step：附树 / 树+截图 / 不附，必要时加固定提示   │
│    · 上下文策略：按 base_snapshot_id 保留 diff 链、旧观察摘要、任务事实   │
│      跨压缩保留；截图每窗口最新 + 固定证据图，其余存盘可取回             │
│    · 一份计划状态（plan.ts），只在里程碑 / 压缩后 / 偏航时提醒           │
│    · skills 只列名字，load_skill 按需加载；每次调用带真实 token 用量     │
│    · 结构化状态：executed/blocked/partial/unknown/input_required + 人的结果│
│    · 问人桥：MRTR inputRequests → human IPC → 弹窗 → inputResponses      │
└───────────┬────────────────────────────────────────────────────────────┘
            │ stdio MCP 2026-07-28
┌───────────▼──────────── gui-gate（Rust） ──────────────────────────────┐
│  风险规则 command.rs / features.rs · 授权 authz.rs · 审核 reviewer.rs   │
│  决策 decide.rs（hard_rules → choose）· 风险门 server.rs risk_gate       │
│  循环打断 loopbreak.rs · 执行与自愈 · 问人 human.rs（本项目修改）         │
└───────────┬────────────────────────────────────────────────────────────┘
            │ stdio MCP（老式，gate 自己握手）
┌───────────▼──────────── cua-driver 0.38.0（固定） ─────────────────────┐
│  AT-SPI / UIA / AX 读树、截图、输入                                       │
└────────────────────────────────────────────────────────────────────────┘
```

进程关系：Tauri 启动 Pi；Pi 的扩展启动 gui-gate；gui-gate 启动 cua-driver。每层退出时带走下一层（gate 已有 shutdown 清理：杀后台任务、关驱动）。

为什么扩展自己做 MCP 客户端，而不是用第三方 pi-mcp 适配器：问人走 MRTR，需要客户端在 `input_required` 时拉起弹窗、带 `inputResponses` 和 `requestState` 重试；这一步必须在我们自己的代码里，模型碰不到。

---

## 3. 门在 Pi 内外的分工

原则：**能放行动作的东西只在 gui-gate 里。** Pi 侧只管“看得更好、问得更好”，不做任何安全判断。

| 职责 | 位置 | 说明 |
|---|---|---|
| 风险规则（危险命令、删除、发送、付款、密码框、验证码） | gate：command.rs、features.rs assess_risk | 不动 |
| 授权（用户原话是否明确要求） | gate：authz.rs | **可信任务通道**（§6.7）：壳把用户自己输入的话（首个请求和运行中每条追加消息）写进应用私有运行目录里的任务文件（0600，带版本），gate 用 `--task-file` 启动并在文件变化时重读，`task_source=operator`。扩展只转交路径、从不写；模型的文字永远进不了这个文件。agent 不能提交或改写任务（`gate_task` 不授权任何东西，扩展也不注册它） |
| 独立审核模型 | gate：reviewer.rs | 必配；没有审核模型时风险步骤一律拒绝（fail closed） |
| 价值门（direct / look / check / wait / heal / review） | gate：decide.rs | 不动 |
| 循环打断（loop 提示、stop_loop） | gate：loopbreak.rs | 不动 |
| 问人通道 | gate：human.rs（改） | 桌面版固定 `--human elicit`，不退到聊天；结果写入 `structuredContent.human_outcome` |
| 问人界面 | 壳：原生弹窗 | 扩展只转发，不能代答 |
| 下一轮看什么（gate_next_step） | gate 决定（建议性，不授予任何东西），扩展执行 | 扩展每轮前问一次，按 observe 附树 / 树+截图 / 不附，extra_reason 时加固定提示 |
| 上下文（观察摘要、diff 基底、任务事实、截图、压缩、计划提醒） | Pi 扩展 | 只改模型看到的消息，会话文件保留原文；树 diff 只由门生成（唯一的 diff 负责人） |
| 系统提示词、skills | Pi | 教模型怎么配合门，不含放行权 |

agent 放行不了自己的理由（逐条可测）：

1. 工具 schema 里没有 `gate_ack`；传了也被忽略。agent 侧的确认码路径（`gate_confirm`）已从门里移除；扩展也不注册旧门可能还列出的 `gate_confirm` / `gate_task` / `gate_config`（`HIDDEN_GATE_TOOLS`），并从 `arguments` 里删掉这些键。
2. 桌面版 gate 跑 `--human elicit`：要问人时 gate 自己发 MRTR 问人请求，不给模型任何确认码。
3. MRTR 的 `inputResponses`、`requestState` 在 `tools/call` 的 params 顶层，由扩展填写；模型只能写 `arguments`。扩展发请求前把 `arguments` 里的 `inputResponses` / `requestState` / `_meta`（以及旧门的 `gate_confirm` / `gate_ack` / `gate_task`）删掉。
4. `requestState` 是 gate 进程内随机 id，映射到 gate 内存里的待答记录（工具名 + 参数摘要 + 种类 + 截止时间），**一次性**，过期作废；重试时工具名或参数摘要对不上就作废。
5. 问人弹窗由壳画，答案从壳经 IPC 回到扩展；IPC 每次启动一个随机 token。每个请求 id 只答一次：重复的 id 不弹窗直接拒绝，同一弹窗只认第一次点击。模型没有 bash 工具，也拿不到 token。
6. 超时按拒绝：弹窗到截止时间自动拒绝；gate 也按自己的时钟判，过期的答案按超时处理。
7. 授权只来自可信通道：用户原话由壳写进任务文件（`--task-file`），扩展的 `GUI_PI_GATE_ARGS` 里的 `--task` / `--task-file` 被丢弃，gate 环境里也不传 `GUI_GATE_TASK*`；模型重写任务、在参数或界面文字里写“用户同意了”都不产生授权。系统提示词明确告诉模型：它永远不能批准自己的动作。
9. 客户端专用工具 `gate_next_step`、`gate_reset` 永远不注册成 Pi 工具（`CLIENT_ONLY_TOOLS`，或 gate 在 tools/list 里标成 client_only）；`gate_next_step` 只给建议，不授予任何东西。
8. `gate_config` 只在可信启动参数 `--allow-config` / `GUI_GATE_ALLOW_CONFIG=1` 下可用；桌面启动器**从不**设置它们，扩展也会从 gate 参数和环境里剔除（有测试）。

---

## 4. Pi 侧设计

### 4.1 启动方式

壳启动：

```
node <res>/pi/dist/cli.js --mode rpc \
  --no-skills --skill <res>/skills/gui-basics ... \
  --extension <res>/gui-pi/index.ts \
  --tools <门的工具清单 + 扩展自己的 4 个工具，见 4.6> \
  --system-prompt "$(cat <res>/prompts/system.md)" \
  --model gui-main/<模型id> --thinking medium
```

- 模型配置：壳在应用私有目录（权限 0700）生成 `models.json`，`apiKey` 写环境变量引用 `"$GUI_PI_MAIN_KEY"`（Pi 1.4.0 只把 `"$NAME"` / `"${NAME}"` 当作环境变量插值，裸名字是字面密钥，0.3.0 因此全部 401；docs/models-configuration.md），真实密钥只放在 Pi 进程的环境里。CI 用包内 Pi 自己的 resolveConfigValue 检查（scripts/check-pi-key.mjs）。
- Pi 进程的环境是显式构造的（`env_clear` + 会话变量白名单 + 自己的变量），壳自身环境里的 `GUI_GATE_*`、`GUI_PI_*`、`PI_*` 和任何像密钥的变量都不会继承（`launch::child_env`）。
- `PI_CODING_AGENT_DIR` 指向应用私有目录，不碰用户自己的 `~/.pi`。
- thinking 默认 medium（之前 bench 的规则：low/off 不可用）。
- 扩展的环境另有 `GUI_PI_TASK_FILE`（任务文件路径）、`GUI_PI_SKILLS`（skills 根目录，Pi 自己的 skill 列表拿不到时的后备）。

### 4.2 指令只写一处：系统提示词、skills、门的简报

审阅指出三处重复（系统提示词、gate 简报、skills/*）。0.3 起每条指令只有一个归属，`test/prompts.test.ts` 用词对重叠（≥0.6）检查系统提示词、每个 skill 和 gui-gate 的 `src/skills.md` 之间没有重复（并验证这个检查能抓到 0.2 的重复）：

| 归属 | 内容 |
|---|---|
| prompts/system.md（< 4000 字符） | 身份；读-做-看循环；**状态含义的唯一清单**（executed / blocked / rethink / partial / unknown / done by a person / 人的各种拒绝）；不能自我批准；没有聊天 → request_human；计划、note_fact、pin_evidence、check_done；不要做的事 |
| gate 的任务简报（gui-gate `src/skills.md`，按任务词选段） | 各应用的快捷键、菜单路径、文件保存约定（Calc/Writer/Impress/Chrome/VLC/GIMP/Thunderbird/VS Code/终端与文件） |
| skills/*（按需加载） | gui-basics（token、diff、截图 id 与固定、坐标、批量）；gate-replies（每种具体回复与客户端说明）；ask-human（四种问法与各自含义）；platform-linux / platform-windows / platform-macos；office-libreoffice、browser、files、terminal 只写简报没有的部分 |

gate 在 initialize 里附的 instructions（INSTRUCTIONS_NOTE、0.9.7 说明）扩展不转给模型。

### 4.3 Skills 按需加载

Pi 只有在自带 read/bash 工具启用时才把 skill 列进系统提示词，而桌面 agent 不给这两个工具（文件访问只能经过门）。所以：壳仍用 `--no-skills --skill <dir>` 让 Pi 加载 skill（解析 frontmatter）；扩展在 `before_agent_start` 里把 Pi 的 skill 列表（只有名字和一句描述）写成系统提示词的一个段落 `gui_skills`，并注册 `load_skill {name}`：只返回列表里那个 skill 的 SKILL.md 正文，别的文件一律拒绝。平台与应用 skill 都不进系统提示词。

### 4.4 上下文策略（扩展 `context` 事件，非破坏）

会话文件保留全部原文；只改送给模型的消息。树 diff 只有一个负责人：**gate 生成，扩展只决定保留哪些**。

1. **观察注入**：gate 在动作结果里附 “Window right after the action”，在被拒结果里附 “Window now (nothing was executed)”。另外**每次模型请求之前**（`before_agent_start` 和每个有工具结果的 `turn_end`），扩展对当前聚焦窗口调用客户端专用工具 `gate_next_step {window_id?}`，得到 `{observe: none|tree|tree+screenshot, extra_reason, reason, decision_id}`：
   - `tree` / `tree+screenshot`：扩展自己调 `get_window_state`（后者带截图），作为一条会话里的 custom message（`gui-pi-observe`，和工具结果里的读窗口一样参与 diff 链）附在下一轮前；
   - `extra_reason: true`：再附一条固定文字，请模型重新生成下一步（不授予任何东西）；
   - `decision_id` 等写进会话（`pi.appendEntry("gui-pi-nextstep")`），便于和 gate 日志对齐；
   - 没有这个工具（gate v0.10.0）：tools/list 里没列就试调一次，回复里没有合法 `observe` 就永不再调，行为和 0.2 相同。
2. **diff 与基底**：gate 发完整树，或相对某个早先快照的 diff。新 gate：`structuredContent.diff = {base_snapshot_id, snapshot_id, changed, removed, reordered}`；旧 gate：文本 “Only rows that changed since snapshot X” / “Nothing in the tree changed since snapshot X”。两种都支持（扩展把解析结果存进 `details.obs`）。
   - 每个窗口（pid:window_id）最新的观察是当前观察；沿 `base_snapshot_id` 往回走直到一棵完整树，这条链上的观察**原样保留**（基底可以早于更新的完整读，按 id 找，不按位置猜）。
   - diff 的基底已不在上下文里（例如被压缩掉）：该 diff 保持原样并追加说明 “基底已不在上下文，未列出的行未知，用其他 token 前先重新读窗口”。
   - 没有可解析基底的旧式 diff：保留到该窗口上一次完整读为止。
   - `removed` / `reordered` 在结果到达时就写成显式说明（哪些行没了、哪些行重新编号、旧 token 不再可靠）。
3. **旧观察摘要**：不在任何窗口当前链上的旧观察，换成一行摘要（标题、snapshot、行数），去掉截图；动作本身的文字（和客户端说明）保留，限长。
4. **截图**：每张图到达时得到一个 id（`img-N`，紧跟在图后的一行文字里），原始字节（JPEG q60，不缩放、不重编码）写进会话目录下的私有目录（`gui-pi-images/<session>/`，目录 0700、文件 0600）。上下文里保留：每个窗口最新一张（最多 3 个窗口）+ 模型用 `pin_evidence {id, label}` 固定的证据图（最多 3 张，可 `unpin`；固定列表用 `appendEntry("gui-pi-pins")` 保存，压缩和重启后恢复）+ 最近一次 `recall_image` 的结果。其余图片换成一行“img-N 已移出上下文，recall_image 可取回”。
5. **任务事实**（`src/facts.ts`）：程序原文保存，不由模型重新概括——用户原话、带来源的事实（`note_fact` 记的，或工具确认的，如 check_done 没有缺口）、人的回答和每次拒绝结果、gate 拒绝过的效果、本次产生/修改的文件（run_command 的 `changed_files` 等）。每次变化用 `pi.appendEntry("gui-pi-facts")` 存进会话，`session_start` 时从当前分支恢复，所以压缩不会丢。
6. **压缩**：`session_before_compact` 直接给出确定性摘要：任务事实原文 + 计划与里程碑状态 + 最后窗口标题（提示先重读）。门自己的状态（denied_effects、计划、历史）在 gate 进程里，不受压缩影响。
7. **一份计划状态**（`src/plan.ts`）：由模型的 `set_plan` 建立（同样发给 gate）。gate 每个结果末尾机械附加的 `[plan] k/n done` 行从结果里删掉，模型只从一处听到计划。提醒只在：里程碑完成或失败、压缩之后的第一个结果、偏航（刚被拒绝、或连续 8 个动作没有进展）。

### 4.5 MCP 客户端（2026-07-28）

- 每个请求带 `_meta`：`io.modelcontextprotocol/protocolVersion = "2026-07-28"`、`clientCapabilities = {"elicitation": {"form": {}}}`、`clientInfo = {name: "gui-pi", version}`。
- 启动：`server/discover` → 版本必须含 2026-07-28 → `tools/list`。
- `tools/call`：
  - 结果 `resultType: "complete"` → 转成 Pi 工具结果（text / image 原样），并按 `structuredContent.status` 加客户端说明：
    | status | 含义 | 客户端做什么 |
    |---|---|---|
    | executed | 已执行（isError 表示驱动报错） | 无 |
    | blocked | 门什么都没执行 | 无（门的文字说明原因） |
    | partial | run_actions 中途停下 | 写明跑了 k/n 步（`batch.executed` 或 `completed_steps`），后面的没跑 |
    | unknown | 可能执行了也可能没有 | 告诉模型结果未知、先重读；**此后的 GUI 动作在读窗口之前一律拦下**；从不自动重试 |
    | input_required | 需要人，什么都没执行 | 写明未执行 |
    | blocked + `next_action: "rethink"` | 门要求重新生成候选（extra_reason 的拦截版） | 告诉模型别重发、别换个说法重发，从当前窗口重新想下一步 |
    | done_by_person（旧门：`gate.done_by_person`） | 接管后人做完了这一步 | 告诉模型别重发；**同一工具 + 同一参数在本任务内被拦下**；结果没附新读时，读窗口前其他动作被拦下 |
    | done | 非 GUI 工具的回答 | 无 |
  - `structuredContent.human_outcome`（decline / cancel / timeout / closed / unavailable，旧门从文字识别）→ 一律按拒绝，写进任务事实，并在壳的时间线上显示原因。
  - gate 在调用前已退出 → status blocked（没执行）；调用途中退出或超时 → status unknown（不重试）；同时通过 `ui.notify` 和 stderr 报告 “gui-gate exited”，壳显示。
  - 结果 `resultType: "input_required"` → 对每个 `inputRequests[k]`（只接受 `elicitation/create`，mode form）交给问人桥，得到 ElicitResult；用**新 id** 重发同一 `tools/call`，带 `inputResponses` 和原样 `requestState`；最多 3 轮。
  - 问人桥不可用（例如 Pi 不在壳里跑）→ 回 `{"action": "decline"}`，gate 按拒绝处理。
  - 拒绝时 ElicitResult 带 `_meta["io.github.cszf1/gui-gate"].reason`（declined / cancelled / timed_out / closed / unavailable），gate 记录到 human_outcome；接受只有表单内容。
- 扩展不缓存、不重放 `requestState`，不在别的请求里用它（规范要求）。
- **真实 token 用量**：每个 `tools/call` 的请求 `_meta` 带 `"gui_pi/usage": {input, output, cache_read}`，取自产生这次调用的那条 assistant 消息（Pi `message_end` 的 usage）。一条消息里有多个调用时，第一个带全部用量，其余带 0 和 `same_message: true`，按调用求和就是真实总量。用量只在 `_meta`，不进 `arguments`；协议键总是覆盖它。
- **密钥擦除**：扩展经手的密钥值（Pi 的模型密钥、问人 token、从壳拿到的 gate 密钥）在任何工具结果、details、会话条目、压缩摘要、通知和日志行里都换成 `***`。

### 4.6 工具清单

Pi 的 `--tools` 只放门的工具和扩展自己的四个工具：list_apps, list_windows, get_window_state, launch_app, bring_to_front, click, double_click, right_click, type_text, press_key, hotkey, scroll, set_value, drag, invoke_menu, run_actions, run_command, request_human, set_plan, check_done, gate_report, inspect（0.9.7 开关打开时出现），以及 note_fact、pin_evidence、recall_image、load_skill。Pi 内置 bash/read/edit/write 不放；`gate_task` / `gate_confirm` / `gate_config` 和客户端专用的 `gate_next_step` / `gate_reset` 永远不放。所有门工具 `executionMode: "sequential"`；有人在答题时其他门工具不跑，答“不”之后本轮剩下的调用跳过（PersonFreeze）。

---

## 5. gui-gate 的修改（分支 desktop-human）

### 5.1 MRTR 问人（修 docs/mcp-2026-07-28.md 的“未做”）

现在：gate 向客户端发服务端请求 `elicitation/create`（老格式）。2026-07-28 规定服务端请求必须走 MRTR，老格式不再支持。

改成：

- 客户端是新式（请求带 2026-07-28 `_meta`）且声明了 `elicitation`：gate 需要人时，`tools/call` 直接返回
  ```json
  {"resultType": "input_required",
   "inputRequests": {"human": {"method": "elicitation/create", "params": {
       "mode": "form", "message": "...", "requestedSchema": {...},
       "_meta": {"io.github.cszf1/gui-gate": {"kind": "approve|answer|takeover|ask_help",
                 "timeout_s": 120, "deadline_ms": 1761400000000, "tool": "click", "target": "Delete file",
                 "reason": "high-risk action: ..."}}}}},
   "requestState": "<随机 id>"}
  ```
  并在内存登记 `{id → 工具名, 参数摘要, 种类, 签名, 截止时间}`。
- 客户端带 `inputResponses` + `requestState` 重试：校验 id 存在、未用过、未过期、工具名和参数摘要一致 → 取出答案，作为这一步“人的回答”交给原来的问人分支；之后流程和原来完全一样（allow → 执行；deny/超时 → 拒绝并记住效果）。
- 老式客户端（initialize 声明了 elicitation）仍走老的服务端请求，保持兼容。
- `request_human` 工具同理。

四种表单（requestedSchema）：

| kind | 来源 | schema | 答案映射 |
|---|---|---|---|
| approve | ask_human（高风险 / 审核模型不答）、request_human approve | `{decision: enum[allow, deny]}` | allow → 执行；其它 → 拒绝 |
| answer | request_human answer | `{answer: string}` | 文本给模型 |
| takeover | handoff（密码框、验证码）、request_human takeover | `{done: boolean}` | true → “人已做完，别重复，从当前状态继续”并附新树；false → 拒绝 |
| ask_help | 连续失败（consec_fail ≥ 2）且有人可问 | `{choice: enum[did_it, try_other], note: string}` | did_it → 同 takeover 完成；try_other → “没人代做，换个办法”，note 转给模型 |

`action` 为 decline / cancel、或超过截止时间 → 一律按拒绝。原因写进 `structuredContent.human_outcome`（decline / cancel / timeout / closed / unavailable）。超时只拒绝这一次动作，不等于用户永久反对，也不立即循环再问。

### 5.2 原生弹窗（修 macOS/Windows 退到聊天）

gate 独立运行（不在桌面壳里，例如命令行 Pi 或别的 MCP 客户端）时的 `desktop` 通道：

- Linux：zenity → kdialog → python3 GTK（已有），新增 takeover / ask_help 的按钮文字。
- macOS：`osascript` 的 `display dialog`（系统自带，无依赖），`giving up after <timeout>`，超时返回 gave up → 拒绝；answer 用 `default answer ""`。
- Windows：`powershell -NoProfile -STA` 内联 WinForms 窗口（系统自带），TopMost，计时器到点关闭 → 拒绝；answer 带文本框。
- `resolve_channel()`：DISPLAY 不再是唯一条件；macOS / Windows 默认有 desktop 通道。

桌面应用里不走 desktop 通道（走 MRTR → 壳的弹窗），但 desktop 通道作为 gate 的独立能力修好并有单元测试（命令行构造测试；真实弹窗在 Linux 测试机上用脚本点按钮验证）。

### 5.3 不变的部分

decide.rs、command.rs、authz.rs、reviewer.rs、loopbreak.rs 的逻辑不改。

---

## 6. 桌面壳（Tauri 2）

### 6.1 窗口

1. **主窗口**：任务输入框、开始/停止、过程流（Pi RPC 事件：思考摘要、工具调用、`[gate]` 回复高亮、截图缩略图）、结束结果。
2. **设置页**：见 6.2。
3. **问人弹窗**（独立小窗口，置顶、获取焦点、任务栏闪烁，倒计时条）。**一个共用组件，四种严格结果类型**（`human::view` 把 gate 的请求整理成显示内容，`popup.js` 按类型画；gate 的旧名 answer / ask_help 分别就是 clarify / help 的表单）：

   | 类型 | 显示 | 按钮 → 结果（只有这一种形状） |
   |---|---|---|
   | approve | gate 确认的具体对象、范围、允许后的后果、gate 为什么问 | 【拒绝】【允许】→ `{decision}` |
   | clarify | 问题；每个选项及“选它之后会怎样”；没有选项时可自由回答 | 选项 / 文本 → `{answer}`；【不回答】→ 拒绝。回答不是批准 |
   | takeover | 人要做的最少步骤；“agent 的输入已暂停”；“完成”不会让 agent 重做原动作 | 【我不做】【完成】→ `{done}` |
   | help | agent 已经试过的办法、卡住的原因 | 【给个提示】（文本）→ `{choice: try_other, note}`；【我替你做了】→ `{choice: did_it}` |

   - 读字段时宽容（`object`/`target`/`effect.resources`、`scope`、`consequence`、`options[{label,value,effect}]`、`steps`、`tried`），缺什么就不显示，从不编造。
   - 倒计时到 0 → 超时（拒绝）；关闭弹窗 → 取消（拒绝）；应用退出、点停止、agent 进程结束时还开着的问题 → 取消（拒绝）。
   - 每个请求 id 只答一次；同一弹窗只认第一次点击（`human::Pending`）。同时只显示一个弹窗；队列按先来后到。
   - 主窗口时间线每个问题只有一行结果（允许 / 拒绝 / 取消 / 超时 / 窗口关闭 / 无人可问；非“是/回答”标“按拒绝处理”）。
4. **问人指标**（`human::Metrics`，只写日志，不额外显示）：每个问题一行 `{"ev":"human_q","schema":"gui-pi-human/1","task","type","outcome","refused","secs","repeated","takeover_steps"}`，任务结束一行 `human_summary`：询问次数、按类型计数、重复问题数与比例（同类型、规范化后文字相同）、`person_active_s`（弹窗出现到人回答，含接管时人动手的时间）、`person_wait_s`（任务中没有问题开着、人只是在看的时间）、`agent_blocked_s`（问题开着的总秒数）、接管步骤数、取消 / 超时 / 拒绝 / 无人可问次数、任务秒数。写到 `<data>/logs/human.jsonl`，供 bench/human_pilot.py 读。

### 6.2 设置页

| 区块 | 字段 | 规则 |
|---|---|---|
| 主模型 | 协议（openai-completions / openai-responses / anthropic-messages / google-generative-ai）、Base URL、模型名、密钥、thinking（默认 medium，不低于 medium）、是否支持图片 | 必填 |
| 审核模型 | 协议（chat / responses / anthropic / gemini / systemone）、URL、模型名、密钥、超时 | **必填，不能关**（设置文件里也没有关闭它的字段，多出来的字段整份拒收） |
| 问人 | 超时秒数（默认 120，最小 30） | 超时 = 拒绝 |
| 高级（默认折叠） | 7 个可选角色 feasibility、planner、verifier、judge、search、compact、grounder；每个：关闭 / 主模型 / 同审核模型 / 自定义端点 | 默认关闭；自定义端点的空字段（含密钥）继承主模型；映射到 `GUI_GATE_ROLE_<NAME>_*` 或 `GUI_GATE_ROLE_<NAME>=reviewer` |

校验（`config::check_base_url` 等，Rust 侧判定，页面只做提前提示）：Base URL 必须 https，只有 localhost / 127.0.0.0/8 / [::1] 允许 http；URL 里不许带用户名密码；`http://localhost.evil.com` 之类前缀伪装不通过；主模型和审核模型的模型名不能为空。

每个模型（含启用的角色）有【测试连接】：请求由 Rust 侧构造和发送（`probe::request`，密钥从钥匙串读出只放进请求头），页面只拿到 ok / 延迟 / 已遮蔽密钥的错误。

### 6.3 密钥

- 只用 OS 钥匙串（`keyring` crate）：Linux Secret Service（GNOME Keyring / KWallet），Windows Credential Manager，macOS Keychain。服务名 `gui-pi-desktop`，账户名 `main` / `reviewer` / `role.<name>`。
- 钥匙串不可用（Linux 没有 Secret Service）时：**不退到明文**，密钥只存在本次运行的内存里，设置页明确提示“仅本次运行有效，从不写盘”。
- 设置文件 `config.json`（0600，目录 0700）只存非机密字段；写入前检查：任何已存密钥的值出现在设置内容里（比如把密钥粘进了 URL 或模型名）就拒绝保存（有测试扫描写出的文件）。
- 谁拿到什么密钥：
  - Pi 进程：只有 `GUI_PI_MAIN_KEY`（主模型）和问人 IPC 的路径与 token；没有任何 `GUI_GATE_*`。
  - gui-gate：审核模型与角色的 `GUI_GATE_*`（含密钥）由壳暂存在 IPC 服务里，扩展启动 gate 前用 token 取一次（`{"op":"gate_env"}`，一次性，取完即清空），只放进 gate 子进程的环境；gate 的环境同样按白名单构造，**不含** `GUI_PI_MAIN_KEY`、问人 token、`GUI_GATE_ALLOW_CONFIG`。“主模型”角色需要主模型密钥，这时它只出现在 gate 的环境里。
  - gate 再往下（驱动、run_command、探测命令）按 gui-gate 自己的白名单给环境，不继承任何模型密钥。
  - 没有 IPC 的开发运行（不是桌面应用）才把 gate 变量放进 Pi 环境，`Launch.gate_env_in_pi` 标明。
- 密钥不写文件、不进参数、不进日志；`redacted_env()` 给日志和界面用。

### 6.4 human IPC

- 壳启动时监听 `$XDG_RUNTIME_DIR/gui-pi-desktop/<pid>.sock`（0600，目录 0700；Windows 目前是 `127.0.0.1` 上的随机端口，地址写成 `tcp:127.0.0.1:PORT`，命名管道留到 M5），生成随机 token，通过环境变量 `GUI_PI_HUMAN_IPC`、`GUI_PI_HUMAN_TOKEN` 交给 Pi 进程。
- 协议：每行一个 JSON。请求 `{token, id, kind, message, schema, timeout_s, deadline_ms, meta}`，回复 `{id, action: accept|decline|cancel, content, reason?}`；`reason` ∈ declined / cancelled / timed_out / closed / unavailable。
- 一次性交接：`{token, op: "gate_env"}` → `{env: {...}}`，每个任务只给一次（见 6.3）。
- 停止通道：扩展启动后发 `{token, op: "watch_stop"}` 并保持连接；用户点停止时壳回 `{"stop":true}`，扩展拦下一切动作、调 `gate_reset`（有的话）、杀掉 gate，再回 `{"ack":"stopped","reset":bool}`（§6.5）。
- 每个请求 id 只处理一次；应用退出时服务进入 closing，新请求直接按取消回复。
- 测试钩子：环境变量 `GUI_PI_HUMAN_AUTO=<规则文件>` 时由脚本按规则自动回答（只在测试构建和 e2e 测试中使用，正式包里该开关被编译掉）。

### 6.5 进程生命周期与崩溃（`gpd_core::proc`）

- Pi 用显式环境启动，stdout 持续读取、只按 LF 分帧；任务结束以 `agent_settled` 为准（`agent_end` 之后 Pi 仍可能重试或压缩）。结束后壳关闭 Pi 的 stdin，Pi 关掉扩展，扩展关掉 gate，gate 关掉驱动；5 秒不退就杀掉。
- 结束原因只判一次：Finished（已 settled 后正常结束）、Stopped（用户停止 / 退出应用）、Crashed（未 settled 就退出，任何退出码；或写不进 stdin）、Disconnected（关闭了 stdout 但进程还在，宽限 3 秒后杀掉）。
- Crashed / Disconnected：所有待答问题取消，主窗口显示退出码和 stderr 末尾几行，并提供【重新开始任务】。
- gate 退出：扩展通过 `ui.notify`（RPC 的 `extension_ui_request` notify）和 stderr 写 “[gui-pi] gui-gate exited …”，壳显示；之后的门工具调用返回 “没有执行”，正在进行的那一步返回 unknown。
- Pi 自己的对话框（select / confirm / input / editor）在桌面版不用：壳立即回 `cancelled`，扩展按拒绝处理，Pi 不会卡住。
- **停止**（主窗口的停止按钮、退出应用）：① 取消所有待答问题；② 经停止通道通知扩展：扩展立刻拦下一切工具调用（之后到达的调用都返回“没有执行”），gate 有 `gate_reset` 就调用它（作废已有批准和观察），然后杀掉 gate；壳最多等 2 秒确认；③ 不管有没有确认，杀掉 Pi 的整个进程组（Pi 以独立进程组启动，gate 和驱动都在组里）。停止之后不会再有任何动作发出；新任务启动全新的 Pi、gate 和驱动。
- 这些行为都在 `desktop/core` 里用假子进程（`/bin/sh -c …`）测试：正常结束、崩溃（退出码、stderr、取消待答问题）、未 settled 的 exit 0、断连、停止（假 Pi 带假 gate 每 50 ms “发一个动作”：停止后进程都不在、动作不再增加，新任务是新进程）、环境只含传入变量；扩展侧用假 gate 测停止后 gate 收不到任何调用。

### 6.6 屏幕坐标与 HiDPI

- 坐标由 gate / 驱动负责：gate 报告的 x/y 是**相对窗口的逻辑坐标**，与它为该窗口给出的截图所在坐标空间一致，换算到设备像素（缩放、显示器原点）由 gate / 驱动完成。
- **原点与缩放要明确，不能默认一致**：不缩小截图只保证图片像素没变，不保证输入坐标和截图坐标天然对齐。约定：原点 = 该窗口（客户区）左上角；单位 = 截图的像素；缩放 = 截图像素 ÷ 窗口逻辑尺寸，由驱动在截图元数据里给出；显示器 = 窗口所在显示器，多显示器下按驱动报告的窗口位置。gate / 驱动负责把 (x, y) 按这三个量换成屏幕设备坐标。三平台的差别见 §6.8。
- 壳和扩展**从不换算坐标**，也不缩放截图：扩展原样转发 `x`、`y`（有测试），截图 JPEG 原尺寸。系统提示词和 gui-basics skill 告诉模型：坐标按最新截图、相对窗口给出，不要自己缩放。
- 壳里唯一的屏幕计算是摆放自己的弹窗：接管弹窗放在当前显示器右上角，用显示器的物理位置、物理尺寸和 scale factor 计算（Tauri `PhysicalPosition`），HiDPI 下不会跑出屏幕。
- 已知限制：多显示器且缩放不同时，窗口跨屏的情况由驱动报告为准；Windows 的 DPI 感知、macOS Retina 的换算在各自平台阶段单独验证（§10 M5/M7）。

### 6.7 可信任务通道（`gpd_core::task`）

- 壳在开始任务时把用户在输入框里打的字写进 `$XDG_RUNTIME_DIR/gui-pi-desktop/task-<id>.txt`（没有 XDG_RUNTIME_DIR 时用 `<data>/run/`；目录 0700，文件 0600，先写临时文件再原子改名），环境变量 `GUI_PI_TASK_FILE` 告诉扩展，扩展用 `--task-file` 启动 gate。
- 运行中用户在主窗口发的每条消息（“Send”）：**先**追加进任务文件（版本 +1），**再**作为 steer 提示发给 Pi。gate 在文件变化时重读。
- 格式（纯文本，旧 gate 把整个文件当任务原文读也能看到每个字）：
  ```
  gui-pi-task/1 version=<n>
  --- user <i> <UTC 时间> initial request|later message
  <原话>
  ```
- 只有两个 UI 命令（开始任务、发送消息）能写：唯一的入口类型 `UserWords` 只从用户输入构造。扩展从不写（静态检查 + 运行测试）；模型的回复、工具结果、对话框、stderr 都到不了这个文件（假 Pi 子进程测试）。任务结束删除文件。

### 6.8 平台说明

| 平台 | 状态 | 要点 |
|---|---|---|
| Linux | 第一阶段 | 只承诺 X11；Wayland 会话启动自检明确提示（读不到别的应用窗口、输入和截图受限），建议换 X11/Xorg 会话。自检 AT-SPI 总线、DBus 会话、Secret Service（没有就提示“密钥只存本次运行内存”）、WebKitGTK 4.1、glibc ≥ 2.35；失败显示在设置页，fail 项阻止开始 |
| Windows | 第二阶段 | 应用没有 UIA 时只能看截图；完整性级别：普通权限的 agent 不能给管理员窗口发输入（要人接管）；UAC 在安全桌面上，agent 看不到也点不到（接管）；DPI 感知与多屏缩放；RDP 断开时没有可交互桌面；run_command 不是 bash，风险规则不能直接套 bash 规则 |
| macOS | 第三阶段 | “辅助功能”和“屏幕录制”两项权限分别授权，撤销后要重启应用才生效；签名身份要稳定，否则权限失效；Retina 下截图像素 = 2× 逻辑点，坐标换算按 §6.6 由驱动给出缩放 |

---

## 7. 打包（Linux 先）

产物：AppImage + .deb（Tauri bundler）。资源目录：

```
resources/
  node/            node 22 LTS 官方二进制（构建时按官方 SHASUMS256 校验）
  pi/              @earendil-works/pi-coding-agent@1.4.0 + 依赖（package-lock 固定）
  gui-pi/          扩展编译产物
  prompts/ skills/
  gui-gate         gui-gate 二进制（同仓库 CI 用 desktop-human 分支构建）
  cua-driver/      cua-driver 0.38.0（scripts/cua-driver.lock 哈希 + SHA256SUMS 校验）
```

- 不改 PATH，不写 shell profile；数据只在 `~/.config/io.github.cszf1.gui-pi-desktop`、`~/.local/share/io.github.cszf1.gui-pi-desktop`、`~/.cache/io.github.cszf1.gui-pi-desktop`、`$XDG_RUNTIME_DIR/gui-pi-desktop`（Tauri 按 identifier 命名）。
- 用户已装的 Cua Driver 不动；gate 用 `--driver` 指向包里的驱动。

### 7.1 卸载范围（`packaging/linux/uninstall.sh`）

只删应用自己的目录：上面四个 + `~/Applications/gui-pi-desktop`（appimage-update.sh 装的当前版和上一版）+ `~/.local/share/applications/io.github.cszf1.gui-pi-desktop.desktop`；`--keys` 时再删钥匙串里服务名 `gui-pi-desktop` 的条目。脚本里有守卫：每个路径必须是绝对路径、在 `$HOME` 或 `$XDG_RUNTIME_DIR` 下、最后一段必须是应用自己的名字，否则整体拒绝（例如把 XDG_CONFIG_HOME 设成 /etc）。**从不删除**：此前或另外安装的 Cua Driver（`~/.local/bin/cua-driver`、`~/.cua`、`/usr/local/bin/cua-driver` 等）、用户文档、Pi 自己的 `~/.pi`、gui-gate 的 `~/.config/gui-gate`、/usr 和 /opt 下的任何东西（.deb 用 apt 卸载，postrm 不删用户数据）。`--print-paths` / `--dry-run` 先看清单。有测试（core/tests/scripts.rs）：路径清单、真实运行后假的 Cua Driver 和用户文件都还在。

### 7.2 发布与更新

- **完整性**：stage-resources.sh 给 resources 里每个文件写 SHA256SUMS（随包，也作为 `resources.SHA256SUMS` 发布）；CI 再给 AppImage / .deb 写 `SHA256SUMS` 一起发布。应用每次启动（后台线程）按包内 SHA256SUMS 校验所有组件（node、Pi、扩展、提示词、skills、gui-gate、Cua Driver 及其签名文件）：有改动或缺失就在设置页说明并**阻止开始任务**；不修复、不重签，驱动签名保持原样。开发运行没有 SHA256SUMS 时只提示“未校验”。
- **不自动更新**：设置里可关的“更新提示”只读 GitHub 上最新的正式 release（不含预发布 / CI 构建），版本更新时在主窗口和设置页显示一行提示和下载页，什么都不下载、不安装。
- **回滚**：`packaging/linux/appimage-update.sh install NEW.AppImage SHA256SUMS` 先按发布的 SHA256SUMS 校验（不在清单里或哈希不对就拒绝），再把当前版改名为 `gui-pi-desktop.previous.AppImage`、新版成为当前版；`rollback` 交换两者。设置和数据目录两版共用。有测试。
- 依赖：AT-SPI（at-spi2-core）、webkit2gtk-4.1；.deb 声明依赖，AppImage 运行时检查并提示。
- 布置资源：`scripts/stage-resources.sh <已构建的 gui-gate 检出目录>` 生成 `dist/resources/`（node 按 SHASUMS256 校验、Pi 按 package-lock `npm ci`、扩展/提示词/skills、gate 二进制、驱动按锁文件哈希和签名校验），最后写 SHA256SUMS。
- CI：`.github/workflows/linux.yml`，GitHub Actions ubuntu-22.04：构建并测试固定提交的 gui-gate（`GATE_REF`）→ 扩展测试 → `cargo test -p gpd-core` → stage-resources → `tauri build`（AppImage + deb）。Windows：NSIS 安装包 + 便携 zip（第二阶段）。

---

## 8. 门的每种动作：触发条件与测试矩阵

层次：
- **E2E-fake**：真实 gate 二进制 + `tests/fake_driver.py` + 假审核模型 HTTP 服务 + 新式 MRTR 客户端（gui-gate 仓库 `tests/test_actions_e2e.py`）。脚本验证，不涉及真模型。
- **E2E-ext**：gui-pi 扩展的 MCP 客户端 + 真实 gate + fake_driver + 自动应答的 human IPC（本仓库 `tests/e2e`）。脚本验证。
- **LIVE**：测试机 gui-pi4，真模型（主模型 + 审核模型）+ 真驱动 + GTK 夹具应用（gui-gate `scripts/fixture_app.py`），弹窗由脚本按规则应答。真模型验证。

| 动作 | 触发条件（代码位置） | E2E-fake 场景 | LIVE 场景 |
|---|---|---|---|
| direct | 预测成功率高，期望代价最低（choose） | 读窗口后点 token | 夹具：点“Increment” |
| look_light | 无观察 + token/index 目标；新窗口出现；heal 失败（hard_rules） | 旧 token 指向重名“OK” | 夹具弹出新对话框后直接点旧目标 |
| look_full | 无观察 + 像素目标；同一动作刚失败又重发；像素目标但窗口/弹窗变了 | 弹窗后像素点击 | 不读窗口直接 x/y 点击 |
| check | 灰区，交回模型前先探一次树（choose 的 grey tier） | 背后改标签后点旧 token → check → 重绑 | 夹具按钮重命名后点旧 token |
| wait | 有加载提示或弹窗（loading_cue / last_popup） | 窗口标题含 “Loading” 后点击 | 夹具“加载中”状态下点击 |
| heal | token 过期且角色+标签唯一（hard_rules） | 再读一次后用旧 token | 读两次后用第一次的 token |
| review | 同一动作重复 ≥2 次无确认效果；或公式选中 review（hard_rules / choose） | 假审核模型 allow → 执行；deny → “reviewer doubts” | 让模型反复点无效按钮 |
| risk_review | 中风险，审核模型决定（hard_rules） | 假审核 allow → 执行；deny → deny | 夹具“Delete file”中风险，真审核模型裁决 |
| ask_human | 高风险，或 risk_mode=human，或审核模型不答（hard_rules / risk_gate） | MRTR：allow → 执行；deny → 拒绝；超时 / 取消 / 关窗 / 无人 → 拒绝并记录 human_outcome；老式 elicitation 兼容 | run_command `rm -rf ~/Documents/x`，弹窗允许/拒绝各一次 |
| ask_help | consec_fail ≥ 2 且人可达（choose；review_step 规则 tier） | did_it → 不执行并附新树；try_other → 换办法 | 夹具一个永远失败的按钮 |
| handoff | 往密码框打字；窗口有验证码（features.rs assess_risk） | 密码框 type_text → takeover done/拒绝 | 夹具密码框 |
| deny | 审核模型拒绝；无审核模型 fail closed；人拒绝/超时 | 各一例 | 真审核模型拒绝一次 |
| deny（same effect already refused） | 同一效果已被拒（risk_gate denied_effects） | 被拒后换参数打同一效果 → 直接 deny，不再问 | 拒绝后模型重试 |
| deny 升级 | 连续拒绝 ≥ deny_consecutive 或累计 ≥ deny_total（deny_text） | 连续三次 → “stop here and tell the user” | — |
| stop_loop | 循环打断级别 ≥ 2，同一步在当前循环里已无进展（server.rs） | 同一点击反复无效果 | 夹具无效按钮 |
| loop 提示 | 循环打断级别上升时附的提示（loopbreak note） | 同上，检查提示文本 | 同上 |
| request_human approve/answer/takeover | 模型主动调用 | 三种 kind × 接受/拒绝/超时 | 任务里要求“问我要哪个文件名” |
| next_step（建议，v0.10.1） | 每轮模型请求前（gate_next_step） | observe 三种 × extra_reason（扩展：假 gate） | 观察附加次数、decision_id 与 gate 日志对齐 |
| rethink（v0.10.1） | blocked + next_action rethink | 扩展：说明 + 不重发 | — |
| done_by_person（v0.10.1） | 接管完成 | 扩展：同一步被拦、读窗口后继续 | 夹具密码框接管 |
| 无人可问（deny 通道） | `--human deny` | ask_human → 拒绝；ask_help 不出现 | — |

每个 E2E-fake 用例断言三件事：返回给模型的文本（`[gate]` 前缀与要点）、事件日志里的 `decision` / `tier` / `executed`、问人记录（kind、channel、answer）。

---

## 9. 和基线对比

- 基线：v0.9.9 + 7 个角色（deepseek-v4.1-flash）+ 审核 System One，OSWorld-Verified 部分结果 137/255 有效题（53.7%）。
- 新 agent 用同一主模型、同一审核模型、同一角色配置、同一任务子集和评分脚本（gui-gate bench/osworld_native.py），区别只在：Pi 系统提示词与 skills、上下文策略、MRTR 问人（bench 里 `--human deny` 等价：IPC 自动拒绝）。
- 只在用户下命令后跑；跑在新测试机，不碰 gui-pi3。
- 报告：成功率、每题回合数、截图数、门干预分布、耗时；逐题对比列出差异题。

---

## 10. 里程碑

| 阶段 | 内容 | 完成标准 |
|---|---|---|
| M0 | 设计文档、仓库骨架 | 本文档；仓库推送 |
| M1 | gui-gate desktop-human：MRTR 问人、四种表单、mac/win 原生弹窗、e2e 全动作测试 | gui-gate 全部测试通过（含新增） |
| M2 | gui-pi 扩展：MCP 客户端、工具注册、上下文策略、问人桥；提示词与 skills | 扩展单测 + E2E-ext 通过 |
| M3 | Tauri 壳 Linux：设置页、钥匙串、弹窗、Pi RPC；AppImage/deb | CI 构建出包 |
| M4 | 测试机 gui-pi4 LIVE：每种门动作和四种问人都真实触发一次 | docs/test-log.md 有逐项记录 |
| M5 | Windows：命名管道、凭据管理器、NSIS 包、Windows 原生弹窗 | Windows CI 构建 + 测试 |
| M6 | 与基线对比（用户下命令后） | 对比报告 |
| M7 | macOS | — |

---

## 11. 对新 gate 接口的假设（v0.10.1，按名字编码，缺失时退回 0.2 行为）

| 接口 | 假设 | 没有时 |
|---|---|---|
| `gate_next_step {window_id?}` | 客户端专用；`structuredContent: {observe, extra_reason, reason, decision_id}`；可能列在 tools/list（可带 `_meta["io.github.cszf1/gui-gate"].client_only`），也可能隐藏 | 试调一次；回复里没有合法 observe 就不再调 |
| `gate_reset {}` | 客户端专用；作废批准和观察；成功时 `structuredContent.reset: true`（列在 tools/list 时只看 isError） | 试调一次（1.5 秒超时）；反正随后杀掉 gate |
| status `blocked` + `next_action: "rethink"` | 什么都没执行 | — |
| status `done_by_person` | 也接受旧门的 `gate.done_by_person: true` | — |
| `--task-file <path>` | 文件变化时重读；格式见 §6.7（纯文本，带版本头） | v0.10.0 启动时读一次（同一参数，已支持） |
| 问人 `_meta` | clarify：`options[{label,value,effect}]`、`allow_other`；help：`tried[]`；takeover：`steps[]`；approve：`object`/`scope`/`consequence`，或从 `target` + `effect{tool,hazard,resources,app,window}` 推出 | 只显示有的字段 |
| 每次调用 `_meta["gui_pi/usage"]` | `{input, output, cache_read}`（+ 可选 `same_message`），gate 按调用记录 | gate 忽略未知 `_meta` 键 |

## 12. 威胁模型

保护的对象：**合作但会犯错的 agent**——模型理解错任务、点错目标、重复提交、把界面里的提示当成用户的话、在压缩后忘了被拒过的效果。门、问人通道、结构化状态、上下文规则都是为这个设计的，并且逐条有测试。

不保护的对象：**恶意 agent** 或被完全劫持的模型。理由：

- gate、Pi、驱动和被操作的应用都以同一个用户身份运行。能以该用户身份执行任意代码的东西，原则上可以读取同用户进程的内存或环境、模拟输入、操作问人弹窗所在的桌面。
- `run_command` 的风险规则是关键词与结构规则，不能证明任意脚本安全（换解释器、分步写入、从 GUI 终端输入都超出规则能可靠覆盖的范围）。
- **审核模型是一道必要的防线，但不是充分的防线**：它不能凭空授予用户没给的权限，也不能推翻明确的拒绝；独立调用也不代表出错相互独立——界面树里的提示注入可能同时误导主模型和审核模型。

已经做的收紧（降低合作型 agent 出错时的伤害面）：Pi 没有 bash/read/edit/write；模型只能写 `arguments`；授权只来自壳写的任务文件里的用户原话、审核模型和人；停止后不再发出任何动作；扩展经手的密钥从模型可见内容和日志里擦除；`gate_config` 只对可信启动参数开放且桌面启动器从不开启；密钥按进程分开、按白名单传环境；问人答案一次性、带 token、可追溯原因。

未来工作：OS 级隔离——由 Rust 壳以受限身份 / 沙箱（Linux：独立用户或 namespaces + seccomp / Landlock；macOS：App Sandbox；Windows：AppContainer / 低完整性级别）运行 agent 和命令执行，问人弹窗放在 agent 无法注入输入的会话里。在那之前，产品说明只承诺“防止合作型 agent 的误操作”。
