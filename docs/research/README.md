# docs/research：研究与审核材料（随 0.3.2 / gui-gate v0.10.3 归档）

这些材料是 gui-gate 与本桌面版开发过程中的外部审核原文、分析脚本、测试机截图与基线日志，按用户 2026-10-26 的同意从工作区
整理进仓库，供以后核对结论用。**都是原样拷贝**（除下面“打码”一节列出的一处），不是重新跑出来的结果。

验证方式的写法与 docs/test-log.md 一致：
- **真模型** = 在测试机 gui-pi5 上用真实主模型（qwen3.8-flash，经 tokenrhythm.studio）和审核模型（decision-model-preview / System One）跑的；
- **CI 实测** = GitHub runner 上真实安装、启动、截图，但**不用任何模型**，问人由 test-hooks 脚本回答；
- **脚本** = gui-gate 的 fake_driver 测试套件或本地单元测试，不用模型、不碰真实桌面；
- **审核** = 外部 AI 审核的文字意见，不是实验结果。

OSWorld 相关数字都来自基于 OSWorld 任务与评测器的**自定义环境**，不是官方 OSWorld-Verified 成绩，不能与排行榜比较。

## reviews/ — 外部审核原文（审核）

| 文件 | 内容 | 后续 |
|---|---|---|
| review-2026-10-23.md | 较早的一份外部审核（10 KB），对象是 Android 应用 Eta（备份导出仍含请求头密钥、本地回调鉴权等），不是 gui-gate；按用户要求一并归档 | 当时在 Eta 项目处理，不在本仓库 |
| review-2026-10-25.md | 第二次外部审核（24 KB）：gui-gate 安全边界（隐藏的 gate_config 可调、基线策略先于安全规则、session_owned 用 mtime、角色探测里的 printenv、heal 不复查风险、`[gate]` 前缀歧义、密钥经环境继承）+ 桌面客户端上下文 / 问人 | gui-gate v0.10.1、桌面 0.2/0.3.0 修复（CHANGELOG） |
| review-2026-10-26-v2.md | 第三次外部审核（25 KB）：价值模型只能说明“能预测部分执行失败”、未证明增量价值；后台重试绕过最终检查、unknown 提前解锁、审批未绑定业务前提、审核超时退回 direct、FNV-1a 摘要、等待算作主动时间、角色显式禁用失效、任务原话截断、gate_next_step 用虚构 click、处理后特征混入训练等 | gui-gate v0.10.3（v3-sec / v3-res 两部分）与桌面 0.3.2；逐条状态见 docs/review-v3-checklist-desktop.md 和 gui-gate 的 docs/review-v3-checklist-sec.md / -res.md |

## analysis-scripts/ — 分析脚本（脚本，离线）

2026-10-21～22 对 gui-gate `results/` 下已提交日志做的离线分析（只读日志，不改实验数据；路径写死为当时工作区的 `/workspace/work/gui-gate/...`，要重跑需改路径）。
只拷了 .py / .json / .sh 小文件（没有大于 2 MB 的）。

| 文件 | 做什么 |
|---|---|
| load.py | 读 OSWorld 运行 gate jsonl 日志的公共函数 |
| exp.py / exp2.py / exp3.py | 从 OSWorld 运行（results/osworld-2026-10-21*）的 gate 日志抽每步特征，做执行失败预测的离线实验（三版特征） |
| rep.sh | 在 results/ 各次运行上用编好的 gui-gate 二进制批量重算（不同参数、有无审核模型） |
| cmds.py / cmds.json | 抽出 run_command 的命令与其 gate 判定（og04/og06/og07/og10 等文件操作用例） |
| apps.py | pi-ds41 运行前后进程列表的对比（去掉内核线程等噪声，看残留进程） |
| audit.py | 把 cmds.json 里的命令对照任务原话逐条审计 |
| diffprep.py / diffsim.py / diffeval.py | 用 pi-ds41 运行的“动作后窗口”记录离线模拟差量观察（diff）并评估省下的上下文 |
| occl.py / occl2.py | 树里被遮挡 / 属于父菜单的行被点击的情况统计 |
| stale.py | pi-ds41 运行里过期 token / 快照引用的统计 |
| turns.py | pi-ds41 运行每任务的轮数与动作分类 |

## screenshots-gui-pi5/ — 测试机与 CI 截图（按原子目录保留）

原目录 `gp5/`（约 19 MB）里的截图和随附的小文本（summary / 日志 / 测试脚本），保持原子目录；**没拷**压缩包与二进制
（*.zip、*.tgz、*.b64、无扩展名的二进制），共约 14 MB。截图里设置页只显示密钥的末 4 位（应用自己的遮蔽，`set (...xxxx)`），没有完整密钥。

| 子目录 / 文件 | 来自哪次 | 方式 |
|---|---|---|
| 根目录 s.png、pop.png、pops.png、pc.png、pt.png、ai.png | 2026-10-25 晚（约 23:40 CST）0.3.0 在 gui-pi5 上的第一次现场测试（设置页、弹窗、主窗口） | 真模型 |
| 根目录 *.sh / *.py | 那次测试用的脚本：装包、起应用、截图、脚本化回答问人（ipc.py）、gatelab 测试应用、泄露检查等 | 测试工具 |
| r031/ | 2026-10-26 凌晨 B1–B11 修复后的复测（build-0c12ff8 / 49b621a / 4f661d5，gate 0cae275）：r1-* 第一轮、r2-* 第二轮（无回答措辞、弹窗高度、B5/B6/B10）；results.md 是逐项结论；scripts/ 是复测脚本 | 真模型（部分问人用 test-hooks 脚本回答） |
| final/{linux,macos,windows}/ | 0.3.0 三平台 CI 冒烟的最终一轮截图（主窗口、设置、四种弹窗）与 summary.txt | CI 实测 |
| mac/、win/ | macOS universal 与 Windows 的 CI 冒烟（含修设置页前后的几轮）；testlog-win.md / testlog-win.merged.md 是对应记录 | CI 实测 |
| w1/ | Windows / macOS runner 上 gui-gate fake_driver 套件的日志与失败清单（Windows 路径修复前） | 脚本 |

## baseline-logs/ — 修复前基线日志（脚本）

证明“测试在修复前确实失败”的日志（只拷文本日志）。

| 文件 | 内容 |
|---|---|
| prefix_run.log | gui-gate v2-sec 修复前（基于 c3893bc）新回归测试的运行：应失败的都失败 |
| postfix_run.log | 同一组测试在修复后的运行 |
| cargo_test_after.log | v2-sec 修复后的 cargo test |
| gg-v2-hum-baseline-fail.txt | v2-hum（问人协议）新测试在修复前基线上的失败清单 |
| gg-v3-sec-prefix-tests.txt | v3-sec（v0.10.3 安全部分）20 个新测试在 v0.10.2（acaba69）二进制上的结果：19 个失败，第 20 个当时已被“同一动作刚失败 → look_full”规则挡住 |

## summary/ — 实验总结

| 文件 | 内容 |
|---|---|
| experiments-summary.md | 研究实验总结（Markdown） |
| gui-agent-实验结果总结.docx | 同一总结的 Word 版（2026-10-26 交给用户）：185/361 = 51.25 %（计划口径）、185/332 = 55.72 %（有效口径）、147 个失败的分类、GUI 预测器 AUC / ECE、无对照组因而不能证明门控增益等结论 |

两份里的 OSWorld 数字来自 v0.9.9 那次**真模型**运行（主模型 deepseek-v4.1-flash，审核 decision-model-preview），自定义环境。

## 打码与密钥检查

拷贝前对全部文本文件 grep 了 `sk-`、`ghp_` / `gho_` / `github_pat_`、`token=`、`apiKey` / `api_key`、`password`、`Bearer …`、
`AKIA…`、`gpdtest` 以及 32 位以上的长串，并对 docx 的正文做了同样检查：

- **打码 1 处**：`screenshots-gui-pi5/kr.sh` 里解锁测试机 gui-pi5 钥匙环的测试口令，换成了 `<REDACTED-test-keyring-password>`。
- `sk-should-not-leak` 出现在 w1/、mac/ 的测试日志里：它是 gui-gate 泄露测试故意写入的假密钥，不是真密钥，保留。
- probe.sh、r031/scripts/kprep.sh 等只引用测试机上的密钥**文件路径**（`/data/workspace/keys/...`），不含密钥内容。
- 截图只含应用遮蔽后的密钥末 4 位。
