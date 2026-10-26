
## 2026-10-26 Windows x86_64 与 macOS universal：CI 构建与冒烟（未上真机）

- desktop main **7ba3501**（合并 win-dev；版本号仍 0.3.0）；gui-gate main **dafb126**（0cae275 + Windows 修复 + 测试不弹真实对话框），随后 **025f9ba**（macOS 终端规则用 ps/lsof；驱动崩溃后的调用不再等满超时）。三个工作流的 `GATE_REF` 一致。gate 未打 tag、未发 release。
- CI 不用任何真模型、不碰测试机；冒烟在 GitHub runner 的桌面上跑（Windows Server 2025 / macOS runner），**不等于真机**。
- 验证方式：**CI实测** = GitHub runner 上真实安装、启动、截图；**脚本(fake_driver)** = gui-gate 的 fake_driver 测试套件（含包内二进制）；**单元** = cargo / node 单元测试；**未测** = 没有验证。
- 截图：`/workspace/work/gp5/win/main/{normal,testhooks}/`（main 7ba3501 的运行），`/workspace/work/gp5/win/{normal,testhooks}/`（win-dev dc681dc 的运行，修设置页之前）。

### Windows（windows-latest，Windows Server 2025 Datacenter 24H2 build 26100）

运行：main 7ba3501 → [run 40231125970](https://github.com/cszf1/gui-pi-desktop/actions/runs/40231125970) 全绿（test、bundle normal、bundle testhooks）；Linux 工作流同一提交 [run 40231125977](https://github.com/cszf1/gui-pi-desktop/actions/runs/40231125977) 绿（新 GATE_REF 下 Linux 仍通过）。

| 项 | 结果 | 验证方式 | 证据 | 日期 |
|---|---|---|---|---|
| gui-gate 构建（静态 CRT）+ `cargo test --release` | 过（120 测试中 save_button_over_existing_export 在 5bdb152 前失败：Windows 文件夹路径含反斜杠；5bdb152 修） | 单元 | run 40231125970 test 作业 | 2026-10-26 |
| gate fake_driver e2e + 安全 + meta（58 个） | 过（OK，skipped=6：run_command / 假终端是 bash-only） | 脚本(fake_driver) | 同上 | 2026-10-26 |
| 第 4/5 次 CI“e2e 卡住 >10 分钟”的原因 | 已修 | 脚本(fake_driver) | 不是死锁：没有 elicitation 的用例在 Windows 上退到 gate 自己的 PowerShell 对话框，每个等满 120 s（第 5 次日志：`test_no_elicitation_capability_never_executes` 123 s、`test_why_a_request_ended_is_recorded` 120 s）；另有 5 个 bash-only 用例失败。修：gate 测试默认 `GUI_GATE_DIALOG=none`、工作流同设；bash-only 用例在 Windows 跳过（dafb126） | 2026-10-26 |
| gate 全量 python（311 个） | **信息性，不阻塞**：29 失败 / 5 错误 / 14 跳过 | 脚本(fake_driver) | win-dev dc681dc run 40228893928。失败分三类：① run_command / 终端行规则 / jobs（bash-only，Windows 不提供 run_command）；② bench 工具（Linux 专用 shell 脚本）；③ **真实的 Windows 路径问题**：`run_command` 的 cwd 把 `C:\Users\...` 拼在 home 后面、`inspect env` / 计划检查 / check_done 对 `C:\...\x/notes.txt` 这类混合分隔符和 8.3 短名（`RUNNER~1`）判断“不存在”。③未修，见剩余问题 W1 | 2026-10-26 |
| 扩展测试（58 个，含对真实 gui-gate.exe 的 E2E-ext） | 过（第一次失败 1 个：测试用 `split("/")` 取 skill 目录名，改 `path.basename`，c3a0f88） | 单元 + 脚本(fake_driver) | run 40231125970 | 2026-10-26 |
| gpd-core 测试（Job Object 停止、环境白名单不区分大小写、自检） | 过（49 个） | 单元 | 同上 | 2026-10-26 |
| 资源布置（node win-x64、Pi 锁、gui-gate.exe、Cua Driver 0.38.0 windows-x86_64；SHA256 + Sigstore） | 过 | CI实测 | bundle 作业 | 2026-10-26 |
| NSIS + MSI 打包 | 过（setup.exe 57.7 MB，msi 95.3 MB） | CI实测 | 预发布资产 | 2026-10-26 |
| 静默安装 `/S` → `%LOCALAPPDATA%\gui-pi-desktop\gui-pi-desktop.exe`，组件齐全（node.exe、gui-gate.exe、cua-driver.exe、SHA256SUMS） | 过 | CI实测 | `install.txt` | 2026-10-26 |
| 自检 | windows ok、webview2 ok（155.0.4283.62）、uia ok（6 个顶层窗口）、keychain ok（凭据管理器）；session warn（runner 没有 SESSIONNAME）、elevation warn（runner 以管理员运行）——两条 warn 是 runner 环境，符合预期 | CI实测 | `app.stderr.txt` | 2026-10-26 |
| 组件校验 | 过：`All 15072 bundled files match the release's SHA256SUMS` | CI实测 | 同上 | 2026-10-26 |
| 无模型配置时 `--run` 拒绝开始 | 过（exit 3） | CI实测 | 冒烟日志 | 2026-10-26 |
| 主窗口截图 | 过：无截断、无乱码 | CI实测 | `main/normal/01-main-window.png` | 2026-10-26 |
| 设置页在启动检查完成后刷新 | **修后过**：dc681dc 上设置页一直停在“Start is blocked: Still checking…”（设置页没监听 `checks_done`，主窗口有）；799226d 修，main 截图显示全部检查项 | CI实测 | `win/testhooks/01-main-and-settings.png`（修前）、`win/main/testhooks/01-main-and-settings.png`（修后） | 2026-10-26 |
| 四种弹窗（测试钩子版 `--demo-popups`，经真实问人 IPC） | 过：approve 480×227、clarify 480×298、takeover 480×282、help 480×290，全部 `fit ... ok true`；文字完整、倒计时行完整、无乱码 | CI实测 | `main/testhooks/02..05-popup-*.png` | 2026-10-26 |
| 弹窗回复检查 | 过：approve `accept/allow`、clarify `accept/"Shuffle labels"`、takeover `accept/done:true`、help 超时 `decline/timed_out`（不回答 = 未执行） | CI实测 | `main/testhooks/app.stdout.txt` | 2026-10-26 |
| 包内 gui-gate.exe 跑 fake_driver e2e | 过：30 个 OK（skipped=1） | 脚本(fake_driver) | `main/normal/bundled-gate-e2e.txt` | 2026-10-26 |
| 静默卸载 | 过：卸载器 exit 0，exe 已删除 | CI实测 | 冒烟日志 | 2026-10-26 |
| 真实任务（真模型）、UIA 读真实应用、UAC / 管理员窗口接管、DPI 多屏、RDP 断开、SmartScreen 提示 | 未测 | 未测 | 需要 Windows 真机 | — |
