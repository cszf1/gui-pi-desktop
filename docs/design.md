# gui-pi-desktop 设计文档

版本 0.1（2026-10-25）。范围：桌面版 computer-use agent，Linux 先做，然后 Windows，最后 macOS。

一句话：**Tauri 壳 + Pi（RPC 模式）+ Pi 扩展 gui-pi（TypeScript）+ gui-gate（Rust 门控）+ cua-driver 0.38.0（固定版本）**，一个安装包装好就能用。

---

## 1. 目标与不做的事

要做到：

1. 用户装一个包，打开应用，在设置页填好主模型和审核模型，输入任务，agent 在本机桌面上操作应用。
2. 每一步 GUI 动作都先过 gui-gate。门的安全核心（风险规则、授权、独立审核模型、执行）只在 Rust 里，agent 无法放行自己。
3. 需要人的时候弹原生窗口：批准/拒绝、回答问题、接管（你来做完再点继续）、卡住求助。超时一律按拒绝。
4. 门的每一种返回动作都能真实触发，都有端到端测试。
5. 和基线比：v0.9.9 在 gui-pi3 上 OSWorld-Verified 部分结果 137/255 有效题成功（53.7%）。

不做：

- 不改 gui-gate 的风险规则和预测器本身（command.rs、authz.rs、decide.rs 的判断逻辑不动），本项目只改**人机通道和协议**（human.rs、MCP 2026-07-28 的问人格式）。
- 驱动不自动升级。cua-driver 固定 0.38.0，打包时用 SHA256SUMS 和 scripts/cua-driver.lock 的哈希校验，换版本只能随应用发版。
- Pi 自带的 bash / read / edit / write 工具在桌面 agent 里关掉（见 §4.6），命令只能走门里的 run_command。

---

## 2. 总体架构

```
┌────────────────────────── Tauri 应用（Rust） ──────────────────────────┐
│  主窗口：任务输入、过程流、停止按钮                                       │
│  设置页：主模型 / 审核模型（必填）/ 7 个可选角色 / 问人超时               │
│  问人弹窗：approve / answer / takeover / ask_help（置顶、倒计时）          │
│  钥匙串：keyring（Linux Secret Service / Windows 凭据管理器 / macOS 钥匙串）│
│  human IPC 服务：Unix socket（Windows 命名管道），一次一 token            │
└───────────┬────────────────────────────────────────────────────────────┘
            │ 子进程 stdin/stdout：pi --mode rpc（JSON 行）
            │ 环境变量：模型密钥（只在内存和子进程环境里，不落盘）
┌───────────▼──────────── Pi 1.4.0（内置 node） ──────────────────────────┐
│  系统提示词 prompts/system.md，skills/*                                 │
│  扩展 gui-pi（TS）：                                                     │
│    · MCP 客户端（2026-07-28：逐请求 _meta + MRTR 问人）                  │
│    · 把门的工具注册成 Pi 工具                                            │
│    · 上下文策略：观察注入、树 diff、旧观察摘要、压缩、计划提醒            │
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
| 授权（用户原话是否明确要求） | gate：authz.rs | 任务原文由壳通过 `gate_task` 传入，`task_source=operator`；agent 转述的请求不能授权 OPERATOR_ONLY 类动作 |
| 独立审核模型 | gate：reviewer.rs | 必配；没有审核模型时风险步骤一律拒绝（fail closed） |
| 价值门（direct / look / check / wait / heal / review） | gate：decide.rs | 不动 |
| 循环打断（loop 提示、stop_loop） | gate：loopbreak.rs | 不动 |
| 问人通道 | gate：human.rs（改） | 桌面版固定 `--human elicit`，不退到聊天 |
| 问人界面 | 壳：原生弹窗 | 扩展只转发，不能代答 |
| 上下文（观察摘要、树 diff、压缩、计划提醒） | Pi 扩展 | 只改模型看到的消息，会话文件保留原文 |
| 系统提示词、skills | Pi | 教模型怎么配合门，不含放行权 |

agent 放行不了自己的理由（逐条可测）：

1. 工具 schema 里没有 `gate_ack`（已有测试 `test_tools_list_augmented`）；`gate_ack` 传了也被忽略。
2. 桌面版 gate 跑 `--human elicit`：ask_human 时 gate 自己发 MRTR 问人请求，**不给模型 confirm_code**；chat 通道（让模型去问用户再带 gate_confirm 回来）在桌面版不可用。
3. MRTR 的 `inputResponses`、`requestState` 在 `tools/call` 的 params 顶层，由扩展填写；模型只能写 `arguments`。扩展发请求前把 `arguments` 里任何名为 `inputResponses` / `requestState` / `gate_confirm` 的键删掉。
4. `requestState` 是 gate 进程内随机 128 位 id，映射到 gate 内存里的待答记录（工具名 + 参数摘要 + 种类 + 截止时间），**一次性**，过期作废；重试时工具名或参数摘要对不上就作废。
5. 问人弹窗由壳画，答案从壳经 IPC 回到扩展；IPC 每次启动一个随机 token，连接要带 token。模型没有 bash 工具，也接触不到 token。
6. 超时按拒绝：弹窗到截止时间自动拒绝；gate 也按自己的时钟判，超过 `human_timeout + 5s` 才回来的答案按超时处理。

---

## 4. Pi 侧设计

### 4.1 启动方式

壳启动：

```
node <res>/pi/dist/cli.js --mode rpc \
  --no-skills --skill <res>/skills/gui-basics ... \
  -e <res>/gui-pi/index.js \
  --tools <门的工具清单，见 4.6> \
  --system-prompt "$(cat <res>/prompts/system.md)" \
  --model gui-main/<模型id> --thinking medium
```

- 模型配置：壳在应用私有目录（权限 0700）生成 `models.json`，`apiKey` 写环境变量名（如 `GUI_PI_MAIN_KEY`），真实密钥只放在子进程环境里。Pi 支持 apiKey 填环境变量名（docs/models.md）。
- `PI_CODING_AGENT_DIR` 指向应用私有目录，不碰用户自己的 `~/.pi`。
- thinking 默认 medium（之前 bench 的规则：low/off 不可用）。

### 4.2 系统提示词（prompts/system.md）

要点（全文见文件）：

1. 身份：你在用户本机桌面上替他操作应用。只做他要求的事。
2. 循环：读窗口 → 做一步或一批 → 看结果（动作结果已附带新窗口状态，不要再读一遍）。
3. 门：任何以 `[gate]` 开头的结果表示**没有执行**，照它说的做。逐条对应：
   - 附了新树 → 从新树重新选目标再发；
   - 附了截图 → 在截图上重新瞄准；
   - stop_loop / loop 提示 → 换一种做法（快捷键、菜单、别的元素），不要重发；
   - 风险拒绝 → 走它给的安全办法完成任务，不要换个说法再试同一效果（gate 记得 “same effect already refused”）；
   - 连续被拒 → 停下，告诉用户你想做什么、为什么。
4. 人：需要人做的事（登录、验证码、付款确认、你拿不准的选择）用 `request_human`，kind 选 approve / answer / takeover。不要在回复里问用户然后等，桌面版没有聊天回路。
5. 开始先用 `gate_task` 原样传用户原话（壳已经传过，模型调用会被 gate 拒绝“只能设一次”，无害）。
6. 计划：多步任务先 `set_plan`（可行性 + 里程碑），每到一个里程碑更新；结束前 `check_done`。
7. 输出：完成时一句话说结果；做不到时说 FAIL 和原因。
8. 不要：猜坐标而不看截图；在没读过的窗口里点；重复刚失败的同一步。

### 4.3 Skills

放在 `skills/`，Pi 的 SKILL.md 格式（frontmatter：name、description），按需加载：

| skill | 内容 |
|---|---|
| gui-basics | 观察-动作-验证循环；element_token 与 x/y 的取舍；run_actions 批量；滚动和菜单 |
| gate-replies | 每种 `[gate]` 回复的含义和正确反应（与 §8 的动作表一一对应） |
| ask-human | 什么时候问人、选哪种 kind、问题怎么写（一两句、可直接回答） |
| office-libreoffice | Calc/Writer/Impress 常用路径（名称框跳格、公式、另存为格式） |
| browser | 地址栏、标签页、下载、表单 |
| terminal | 在终端里跑命令 vs run_command；危险命令会被门拦 |
| files | 文件管理器、保存对话框、路径输入 |

skills 只写“怎么做”，不写任何能绕过门的东西。

### 4.4 上下文策略（扩展 `context` 事件，非破坏）

会话文件保留全部原文；只改送给模型的消息。

1. **观察注入**：gate 已经在动作结果里附 “Window right after the action”，在被拒结果里附 “Window now (nothing was executed)”。扩展不另外读窗口，避免多一轮。
2. **锚点 + 树 diff**：
   - 每个窗口（pid:window_id）最新一次“完整树”是锚点，原样保留，带截图（若有）。
   - 锚点之后同一窗口的动作结果树，若和锚点相比改动行数 ≤ 50% 且距锚点 ≤ 3 次，就替换成“相对锚点的变化”：新增 `+ 行`、消失 `- 行`、标题变化；token 以最新那份为准（diff 里保留新行的完整 token）。
   - 超过阈值的那份成为新锚点。
3. **旧观察摘要**：不是任何窗口当前锚点的旧观察，换成一行摘要（标题、snapshot、行数），去掉截图（沿用 bench/pi_ext/obs_prune.ts 的规则，已有离线回放和对齐测试）。
4. **截图只留最新**：整个上下文里最多保留最新 1 张截图（look_full 后的那张）。
5. **压缩**：`session_before_compact` 给自定义指令：保留用户原话、计划与里程碑状态、已确认的事实、被门拒绝过的效果（避免重试）、人给过的答案、当前窗口标题；丢弃旧树。门自己的状态（denied_effects、计划、历史）在 gate 进程里，不受压缩影响。
6. **计划提醒**：`set_plan` 的里程碑在扩展里也记一份，每轮在上下文末尾附一行“当前计划：第 k/n 步 …”（只读提示，不影响门）。

### 4.5 MCP 客户端（2026-07-28）

- 每个请求带 `_meta`：`io.modelcontextprotocol/protocolVersion = "2026-07-28"`、`clientCapabilities = {"elicitation": {"form": {}}}`、`clientInfo = {name: "gui-pi", version}`。
- 启动：`server/discover` → 版本必须含 2026-07-28 → `tools/list`。
- `tools/call`：
  - 结果 `resultType: "complete"` → 转成 Pi 工具结果（text / image 原样）。
  - 结果 `resultType: "input_required"` → 对每个 `inputRequests[k]`（只接受 `elicitation/create`，mode form）交给问人桥，得到 ElicitResult；用**新 id** 重发同一 `tools/call`，带 `inputResponses` 和原样 `requestState`；最多 3 轮。
  - 问人桥不可用（例如 Pi 不在壳里跑）→ 回 `{"action": "decline"}`，gate 按拒绝处理。
- 扩展不缓存、不重放 `requestState`，不在别的请求里用它（规范要求）。

### 4.6 工具清单

Pi 的 `--tools` 只放门的工具：list_apps, list_windows, get_window_state, launch_app, bring_to_front, click, double_click, right_click, type_text, press_key, hotkey, scroll, set_value, drag, invoke_menu, run_actions, run_command, request_human, set_plan, check_done, gate_report（inspect 在 0.9.7 开关打开时出现）。Pi 内置 bash/read/edit/write 不放。

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

`action` 为 decline / cancel、或超过截止时间 → 一律按拒绝。

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
3. **问人弹窗**（独立小窗口，置顶、获取焦点、任务栏闪烁，倒计时条）：
   - approve：“agent 想 <动作> <目标>。门拦下的原因：<reason>。” 按钮【允许】【拒绝】。
   - answer：问题 + 文本框，【发送】【不回答】。
   - takeover：“请你在应用里完成：<事情>。做完点【我做完了，继续】；不做点【不做】。” 弹窗缩到屏幕角落，不挡操作。
   - ask_help：“agent 卡住了：<最近失败>。它想 <动作>。” 【我来做这一步（做完点继续）】【让它换个办法】+ 可选一句提示。
   - 倒计时到 0 自动按拒绝（answer 视为不回答），弹窗关闭也按拒绝。
   - 同时只显示一个弹窗；队列按先来后到。

### 6.2 设置页

| 区块 | 字段 | 规则 |
|---|---|---|
| 主模型 | 协议（openai-chat / openai-responses / anthropic / gemini）、Base URL、模型 id、密钥、thinking（默认 medium）、是否支持图片 | 必填 |
| 审核模型 | 协议（chat / responses / anthropic / gemini / systemone）、URL、模型、密钥、超时 | **必填，不能关**；没测通不能开始任务 |
| 可选角色 ×7 | feasibility、planner、verifier、judge、search、compact、grounder；每个：关闭 / 同审核模型 / 自定义端点 | 默认关闭；映射到 `GUI_GATE_ROLE_<NAME>_*` 或 `GUI_GATE_ROLE_<NAME>=reviewer` |
| 问人 | 超时秒数（默认 120，最小 30） | 超时 = 拒绝 |
| 门 | lambda（默认）、0.9.7 功能开关（默认按 v0.9.9 bench 的配置） | 高级 |

每个模型有【测试连接】按钮（发一句极短请求，显示延迟和错误）。

### 6.3 密钥

- 用 `keyring` crate：Linux Secret Service（GNOME Keyring / KWallet），Windows Credential Manager，macOS Keychain。服务名 `gui-pi-desktop`，账户名 `main` / `reviewer` / `role.<name>`。
- 设置文件 `config.json`（应用配置目录）只存非机密字段和“该槽位有密钥”的标记。
- 启动任务时壳读钥匙串，只通过子进程环境变量传下去（`GUI_PI_MAIN_KEY`、`GUI_GATE_REVIEWER_KEY`、`GUI_GATE_ROLE_<NAME>_KEY`），不写文件，不进日志。gate 的事件日志不记录环境变量。
- Linux 没有 Secret Service（无桌面会话、无 keyring 守护）时：**不退到明文**，设置页提示安装 gnome-keyring 或选择“仅本次运行有效”（密钥只存在内存）。

### 6.4 human IPC

- 壳启动时监听 `$XDG_RUNTIME_DIR/gui-pi-desktop/<pid>.sock`（0600；Windows：`\\.\pipe\gui-pi-desktop-<pid>-<rand>`），生成随机 token，通过环境变量 `GUI_PI_HUMAN_IPC`、`GUI_PI_HUMAN_TOKEN` 交给 Pi 进程。
- 协议：每行一个 JSON。请求 `{token, id, kind, message, schema, timeout_s, deadline_ms, meta}`，回复 `{id, action: accept|decline|cancel, content}`。
- 测试钩子：环境变量 `GUI_PI_HUMAN_AUTO=<规则文件>` 时由脚本按规则自动回答（只在测试构建和 e2e 测试中使用，正式包里该开关被编译掉）。

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

- 不改 PATH，不写 shell profile；数据只在 `~/.local/share/gui-pi-desktop`、`~/.config/gui-pi-desktop`、`$XDG_RUNTIME_DIR`。卸载即删这两个目录（.deb 的 postrm 不删用户数据，应用内提供“清除所有数据”）。
- 用户已装的 Cua Driver 不动；gate 用 `GUI_GATE_DRIVER` 指向内置驱动。
- 依赖：AT-SPI（at-spi2-core）、webkit2gtk-4.1；.deb 声明依赖，AppImage 运行时检查并提示。
- CI：GitHub Actions ubuntu-22.04 构建（webkit2gtk 在沙箱里没有）。Windows：NSIS 安装包 + 便携 zip（第二阶段）。

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
| ask_human | 高风险，或 risk_mode=human，或审核模型不答（hard_rules / risk_gate） | MRTR：allow → 执行；deny → 拒绝；超时 → 拒绝；老式 elicitation 兼容 | run_command `rm -rf ~/Documents/x`，弹窗允许/拒绝各一次 |
| ask_help | consec_fail ≥ 2 且人可达（choose；review_step 规则 tier） | did_it → 不执行并附新树；try_other → 换办法 | 夹具一个永远失败的按钮 |
| handoff | 往密码框打字；窗口有验证码（features.rs assess_risk） | 密码框 type_text → takeover done/拒绝 | 夹具密码框 |
| deny | 审核模型拒绝；无审核模型 fail closed；人拒绝/超时 | 各一例 | 真审核模型拒绝一次 |
| deny（same effect already refused） | 同一效果已被拒（risk_gate denied_effects） | 被拒后换参数打同一效果 → 直接 deny，不再问 | 拒绝后模型重试 |
| deny 升级 | 连续拒绝 ≥ deny_consecutive 或累计 ≥ deny_total（deny_text） | 连续三次 → “stop here and tell the user” | — |
| stop_loop | 循环打断级别 ≥ 2，同一步在当前循环里已无进展（server.rs） | 同一点击反复无效果 | 夹具无效按钮 |
| loop 提示 | 循环打断级别上升时附的提示（loopbreak note） | 同上，检查提示文本 | 同上 |
| request_human approve/answer/takeover | 模型主动调用 | 三种 kind × 接受/拒绝/超时 | 任务里要求“问我要哪个文件名” |
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
