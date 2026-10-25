# gui-pi-desktop

桌面版 computer-use agent：Tauri 壳 + Pi 1.4.0（`@earendil-works/pi-coding-agent`，RPC 模式）+ Pi 扩展 gui-pi + [gui-gate](https://github.com/cszf1/gui-gate)（Rust 门控）+ cua-driver 0.38.0（固定版本）。

状态：开发中（Linux 先做）。设计见 [docs/design.md](docs/design.md)，变更见 [CHANGELOG.md](CHANGELOG.md)。

安全边界：门保护的是**合作但会犯错的 agent**，不是恶意 agent；OS 级隔离是后续工作（design.md §11）。

## 目录

| 路径 | 内容 |
|---|---|
| `desktop/core` | crate `gpd-core`：设置与校验、钥匙串槽位、启动环境（Pi / gate 分开）、问人 IPC、Pi RPC 事件、进程监管、测试连接请求。不依赖 GUI，可在任何机器上测试 |
| `desktop/src-tauri` | Tauri 2 壳（窗口、钥匙串、弹窗），只做胶水 |
| `desktop/ui` | 主窗口、设置页、问人弹窗（纯 HTML/JS） |
| `packages/gui-pi` | Pi 扩展（TypeScript）：MCP 2026-07-28 客户端 + MRTR 问人、结构化状态、上下文策略、任务事实 |
| `prompts/`、`skills/` | 系统提示词与 skills |
| `packaging/pi` | Pi 1.4.0 的 package.json + package-lock（固定版本） |
| `scripts/stage-resources.sh` | 布置打包资源（全部按哈希校验） |
| `.github/workflows/linux.yml` | Linux CI：测试 + AppImage / deb |

## 测试

```sh
# 扩展（node >= 22.19，直接跑 .ts；有 gui-gate 二进制时会跑真门 + 假驱动的 e2e，没有则跳过）
cd packages/gui-pi && node --test test/*.test.ts
#   GUI_GATE_SRC=<gui-gate 检出目录>  GUI_GATE_BIN=<gui-gate 二进制>  可指定门的位置

# 壳的核心逻辑（无需 webkit2gtk）
cd desktop && cargo test -p gpd-core
```

## 构建（Linux x86_64）

依赖：Rust stable、node 22、python3、`libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev patchelf libssl-dev libdbus-1-dev at-spi2-core`（Ubuntu 22.04 包名）。

```sh
# 1. 构建 gui-gate（发布的应用固定一个提交，见 linux.yml 的 GATE_REF）
git clone https://github.com/cszf1/gui-gate ../gui-gate
(cd ../gui-gate && git checkout <GATE_REF> && cargo build --release)

# 2. 布置资源到 dist/resources/：node（SHASUMS256 校验）、Pi（package-lock，npm ci）、扩展、提示词、skills、
#    gui-gate 二进制、cua-driver 0.38.0（锁文件哈希；REQUIRE_SIG=1 时还要求签名），最后生成 SHA256SUMS
REQUIRE_SIG=1 scripts/stage-resources.sh ../gui-gate

# 3. 打包（AppImage + deb，产物在 desktop/target/release/bundle/）
cd desktop/src-tauri && npx -y @tauri-apps/cli@2 build
#   测试机用的构建（弹窗由脚本按 GUI_PI_HUMAN_AUTO 规则应答）：加 --features test-hooks，正式包里没有这个开关
```

开发时可以不打包：`GUI_PI_RESOURCES=$PWD/dist/resources cargo run -p gui-pi-desktop`（在 `desktop/` 下）。
无界面跑一个任务：`gui-pi-desktop --run "任务原文"`（用已保存的设置，弹窗照常显示，事件打印到 stdout）。

CI（`.github/workflows/linux.yml`，ubuntu-22.04）：构建并测试固定提交的 gui-gate → 扩展测试 → `cargo test -p gpd-core` → `stage-resources.sh` → `tauri build`，上传 AppImage / deb / SHA256SUMS 为构件，并为 main 的每次推送发布一个预发布（测试机无需登录即可下载）。

## 密钥

只存在 OS 钥匙串里（Secret Service / Windows 凭据管理器 / macOS 钥匙串）；没有钥匙串时只存在本次运行的内存里，设置页会提示，从不写盘。Pi 只拿到主模型密钥；审核模型与角色的密钥只交给 gui-gate。
