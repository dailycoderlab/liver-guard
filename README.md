# LiverGuard（护肝）

> 挂机护肝不费心

LiverGuard（中文名「护肝」）是纯本地的桌面脚本编辑与执行工具：脚本以**节点画布**编辑（拖拽、连线、条件分支、循环、重试），用 OpenCV 在本机截图并匹配模板后执行点击、拖动和输入。脚本、模板素材和运行记录都存在本机，不需要账号，也不联网。

技术栈：Tauri 2 + React / React Flow（界面）+ Rust 核心（脚本模型、匹配、解释器、平台驱动）。macOS 和 Windows 共用同一套核心代码。架构见 [DESIGN.md](DESIGN.md)。

## 目录

| 路径 | 作用 |
| --- | --- |
| `crates/liverguard-ir` | 脚本 IR v2：节点、连线、目标、校验；同时导出 JSON Schema |
| `crates/liverguard-vision` | OpenCV 模板匹配、多尺度、控件候选检测、模板裁剪 |
| `crates/liverguard-engine` | `Driver` 抽象与图解释器（循环、重试、分支、停滞与超时保护） |
| `crates/liverguard-store` | 本地脚本、素材、运行记录（原子写入） |
| `crates/liverguard-driver-mac` | macOS：ScreenCaptureKit 截图、CGEvent 输入、权限、窗口枚举 |
| `crates/liverguard-driver-win` | Windows：Windows Graphics Capture、SendInput、窗口枚举 |
| `crates/liverguard-cli` | 命令行 `liverguard`：`doctor` / `windows` / `capture` / `probe` / `run` |
| `apps/desktop/src-tauri` | Tauri 外壳，只负责把核心能力暴露成命令 |
| `apps/desktop/web` | 画布编辑器（React 19、`@xyflow/react`、Zustand、Tailwind 4） |

## 本地开发

需要：Rust 1.88+、Node 20+、pnpm、OpenCV 4.x 和 libclang（`opencv` crate 的绑定生成需要）。

```sh
# 前端依赖与类型
cd apps/desktop/web
pnpm install
pnpm gen:types        # 由 Rust 的 JSON Schema 生成 src/ir/generated.ts
pnpm test             # 画布状态与图操作测试
pnpm build            # 类型检查并构建

# 桌面应用（在 apps/desktop 下运行，Tauri CLI 来自 web 的 devDependencies）
cd ..
./web/node_modules/.bin/tauri dev
```

只改界面时，`pnpm dev` 可以在浏览器里运行：此时使用内置的模拟后端，不会截图或点击。

### OpenCV 构建环境

Rust 的 `opencv` crate 在编译期需要找到 OpenCV 头文件和 libclang。本项目开发机用的是自行编译的最小静态 OpenCV 4.12（只含 `core`、`imgproc`、`imgcodecs`、`features2d`、`calib3d`、`flann`），通过 `OpenCV_DIR`、`CMAKE_PREFIX_PATH` 指向它；这一步没有脚本化，换机器需要自行准备。`.cargo/config.toml` 已为 macOS 加上 Command Line Tools 的 rpath，避免 `tauri dev` 经 node 启动 cargo 时丢失 `DYLD_*` 变量（SIP）而找不到 `libclang.dylib`。

## 命令行

```sh
cargo run -p liverguard-cli -- doctor                  # 检查权限、截图和坐标
cargo run -p liverguard-cli -- windows                 # 列出可控窗口
cargo run -p liverguard-cli -- capture --out shot.png  # 保存一张截图
cargo run -p liverguard-cli -- probe --script <id>     # 只定位模板，不点击
cargo run -p liverguard-cli -- run   --script <id>     # 运行脚本
```

`run` / `probe` 也可以用 `--file` 指定 IR JSON，用 `--window <名称或 #编号>` 限定到单个窗口。

## 使用说明

- 左侧「脚本 / 节点 / 目标」：新建与切换脚本、把节点拖进画布、管理模板目标。
- 画布：从节点右侧的输出口拖线到另一个节点；每个输出口只能连一条线，允许成环（循环用 `loop` / `retry` 节点或显式回连，由 `maxVisits` 与整体超时兜底）。
- 右侧「画面」：开始实时预览 → 截取画面（冻结）→ 单击让引擎自动圈出控件，或拖动框选 → 创建模板目标，并自动绑定到当前选中的节点。
- 底部：运行日志、校验问题、探测结果。运行时节点按状态高亮，失败节点标红。
- 「探测」只截一帧并定位所有目标，不点击；`threshold`（默认 0.82）以下的匹配一律按未找到处理，`tap` 等动作不会点出去。
- 左右两侧面板和底部日志面板都可以拖动边缘调整大小，双击边缘还原默认；焦点在边缘时可用方向键微调（Shift 加大步长）。大小会记在本机，下次启动沿用。
- 快捷键：⌘S 保存，⌘Z / ⇧⌘Z 撤销与重做，⌘Enter 运行。

### 本地数据

默认在 `~/.liverguard/`，设置环境变量 `LIVERGUARD_HOME` 可改到别处：

- `scripts/`：一个脚本一个 JSON 文件（IR v2）。
- `assets/`：模板素材，按 SHA-256 命名的无损 PNG。
- `runs/`：运行记录，含汇总指标与 JSONL 日志。

**不兼容旧版本。** 本版直接覆盖旧的 Python 实现，不做迁移：旧的 IR v1 脚本无法解析，列表里不会出现它们（文件原样留在磁盘上，不会被删除）。

### 权限

- macOS 需要「屏幕录制」和「辅助功能」。权限记在**负责进程**上：打包后的 `LiverGuard.app`（标识 `com.sky.liverguard`）会作为 “LiverGuard” 出现在系统设置里；用 `tauri dev` 或直接运行二进制时，权限记在启动它的终端或 IDE 上。授予屏幕录制后需要完全退出并重启程序才生效。应用启动后若缺权限，界面会显示权限引导。
- Windows 使用 Windows Graphics Capture（Windows 10 1803+）。目标程序以管理员身份运行时，LiverGuard 也需要以管理员身份运行，否则输入会被系统拒绝。
- 请勿在存在反作弊或禁止自动化条款的应用中使用。

## 测试

```sh
cargo test --workspace     # Rust：IR、匹配、解释器、存储
cd apps/desktop/web && pnpm test
```

`crates/liverguard-vision/tests/golden/` 里是匹配对拍夹具：它由已删除的 Python 实现（OpenCV 5.0.0）生成，当前 Rust（OpenCV 4.12）要在得分 ±0.01、点击点 ±1 个逻辑点内复现。夹具现在是冻结的，没有再生成脚本。

自动化测试使用模拟驱动，不能替代真机验证：截图、输入、窗口焦点和权限需要在真实的 macOS 与 Windows 桌面上分别检查。

## 打包

在目标系统上分别执行（不能在 macOS 上生成 Windows 安装包）：

```sh
cd apps/desktop
./web/node_modules/.bin/tauri build --bundles app    # macOS，产物在 target/release/bundle/macos/LiverGuard.app
```

当前产物是 ad-hoc 签名，未公证；对外分发前还需要：

- macOS：Developer ID 签名、`notarytool` 公证、装订（`stapler`）。`src-tauri/entitlements.plist` 已按 hardened runtime 准备，不需要额外 entitlement。设置 `APPLE_SIGNING_IDENTITY`、`APPLE_ID`、`APPLE_PASSWORD`、`APPLE_TEAM_ID` 后 `tauri build` 会自动签名并公证。
- Windows：代码签名证书（`bundle.windows.certificateThumbprint`）；WebView2 运行时在 Windows 11 自带，Windows 10 需要随安装包引导安装。
