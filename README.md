# LiverGuard（护肝）

> 挂机护肝不费心

LiverGuard（中文名「护肝」）是纯本地的桌面脚本编辑与执行工具：脚本以**节点画布**编辑（拖拽、连线、条件分支、循环、重试），用 OpenCV 在本机截图并匹配模板后执行点击、拖动和输入。脚本、模板素材和运行记录都存在本机，不需要账号，也不联网。

技术栈：Tauri 2 + React / React Flow（界面）+ Rust 核心（脚本模型、匹配、解释器、平台驱动）。macOS 和 Windows 共用同一套核心代码。架构见 [DESIGN.md](DESIGN.md)。

## 目录

| 路径 | 作用 |
| --- | --- |
| `crates/liverguard-ir` | 脚本 IR v3：节点、连线、目标、校验；同时导出 JSON Schema |
| `crates/liverguard-vision` | OpenCV 模板匹配、多尺度、控件候选检测、模板裁剪 |
| `crates/liverguard-engine` | `Driver` 抽象与图解释器（循环、重试、分支、停滞与超时保护） |
| `crates/liverguard-store` | 本地脚本、素材、运行记录、应用设置（原子写入） |
| `crates/liverguard-notify` | 运行通知（失败告警、循环完成通知）：向钉钉 / 企业微信 / 飞书 / 通用 Webhook 发文本消息 |
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

macOS / Linux 上也可以用根目录的 [`Makefile`](Makefile)，它只是把上面的命令转发一遍（Windows 默认没有 `make`，继续用上面的命令）。涉及 Rust 的目标会先检查 `OpenCV_DIR`，没加载 OpenCV 环境就直接报错：

```sh
make dev-web        # 浏览器里的界面开发（模拟后端）
make dev-desktop    # 桌面应用，等价于 cd apps/desktop && tauri dev
make check          # CI 跑的那一套：cargo fmt --check、前端类型检查 / 测试 / 构建、clippy、cargo test --workspace
make fmt            # cargo fmt --all
make build-desktop  # 转发到 scripts/mac-dev-build.sh（见「打包」）
```

### OpenCV 构建环境

Rust 的 `opencv` crate 在编译期需要找到 OpenCV 头文件和 libclang。本项目开发机用的是自行编译的最小静态 OpenCV 4.12（只含 `core`、`imgproc`、`imgcodecs`、`features2d`、`calib3d`、`flann`），通过 `OpenCV_DIR`、`CMAKE_PREFIX_PATH` 指向它。换机器可以用 `scripts/ci/build-opencv.sh <安装目录>` 从官方源码包（校验 SHA-256）重新编译同样的配置，它会打印要设置的环境变量；CI 用的就是这个脚本（macOS 与 Windows 的 Git Bash，Windows 还需要先加载 MSVC 环境）。`.cargo/config.toml` 已为 macOS 加上 Command Line Tools 的 rpath，避免 `tauri dev` 经 node 启动 cargo 时丢失 `DYLD_*` 变量（SIP）而找不到 `libclang.dylib`。

## 命令行

```sh
cargo run -p liverguard-cli -- doctor                  # 检查权限、截图和坐标
cargo run -p liverguard-cli -- windows                 # 列出可控窗口
cargo run -p liverguard-cli -- capture --out shot.png  # 保存一张截图
cargo run -p liverguard-cli -- probe --script <id>     # 只定位模板，不点击
cargo run -p liverguard-cli -- run   --script <id>     # 运行脚本（--repeat N 连续跑 N 轮）
cargo run -p liverguard-cli -- notify-test            # 用已保存的通知设置发一条测试消息
cargo run -p liverguard-cli -- export --script <id|名称> --out q.liverguard.json  # 导出脚本和它的模板图片
cargo run -p liverguard-cli -- import q.liverguard.json  # 导入为一个新脚本
```

`run` / `probe` 也可以用 `--file` 指定 IR JSON，用 `--window <名称或 #编号>` 限定到单个窗口。

## 使用说明

- 左侧「脚本 / 节点 / 目标」：新建、复制（悬停脚本行出现按钮，复制的是已保存的版本，目标模板图按内容寻址，与原脚本共用）与切换脚本、把节点拖进画布、管理模板目标。
- 目标的缩略图点击可放大查看（「目标」面板和节点属性里的目标选择处；画布节点上的缩略图点击仍是选中节点，不放大），弹层显示原始像素尺寸，Esc 或点空白处关闭。
- 脚本的导入导出：「新建脚本」旁的「导入」选一个导出文件，作为**新脚本**加进来（换新 id，从不覆盖已有脚本；重名时名称后加「（导入）」）；悬停脚本行的下载图标是「导出为文件」，弹出系统的保存对话框。文件是一个 JSON（`*.liverguard.json`），脚本用到的模板图片以 base64 内嵌，所以单个文件就能搬到另一台电脑；导出的是已保存的版本，当前脚本有未保存修改时会先问。导入会校验文件格式与版本、IR 版本、每张图片的 SHA-256 与 PNG 头，任何一项不对就整个拒绝，不会留下半截数据。旧 IR（v2 及以前）的脚本不能导入。
- 画布：从节点右侧的输出口拖线到另一个节点；每个输出口只能连一条线，允许成环（循环用 `loop` / `retry` 节点或显式回连，由 `maxVisits` 与整体超时兜底）。
- 顶部栏的「窗口」下拉：选择脚本作用在整个屏幕还是某个窗口（旁边的按钮刷新窗口列表）。**运行、探测、实时预览之前必须先选**：没选就点运行 / 探测 / 开始预览，会提示并让下拉框高亮；所选窗口已关闭（窗口重开后编号会变）也会提示重新选择，不会带着失效的选择去跑。这个选择不随脚本保存，应用重启后需要重新选。
- 右侧「画面」面板：开始实时预览 → 截取画面（冻结）→ 单击让引擎自动圈出控件，或拖动框选 → 创建模板目标，并自动绑定到当前选中的节点。
- 已有的模板目标可以再框选一次：「目标」面板里鼠标移到目标上，点「框选图片」图标，在放大的图片里拖动框出要保留的部分（可拖小方块微调），确定后这个目标的图片就换成框内部分，名称、阈值、搜索范围、点击位置比例都不变，用到它的节点直接用新图片匹配；可以撤销（⌘Z），旧图片文件仍留在本地素材目录里。坐标点目标没有图片，不支持。
- 也可以不连屏幕：在「画面」或「目标」面板点「上传图片」（PNG / JPEG / WebP / BMP，最大 30 MB、单边不超过 10000 像素），用一张截图代替冻结画面，框选方式相同。要上传整个屏幕或整个窗口的完整截图，不要先裁剪；上传图片没有真实窗口可查，单击自动圈出不会用到系统辅助功能的控件框。
- 右上角「运行」跑一次；旁边的「循环」点开后填次数、点「开始循环 N 次」，整张图从开始节点连续走 N 轮（次数是这次运行的参数，不存进脚本，窗口里会记住上次填的数）。每轮变量清空，「结束」节点只结束当前一轮，某轮失败或手动停止则整个终止；运行没有总时长限制，要停请手动停止。CLI 用 `liverguard run --repeat N`。
- 顶部铃铛「运行通知」：配置失败告警和循环完成通知（整个应用一份设置，对所有脚本生效，不随脚本导出）。选接收方（钉钉 / 企业微信 / 飞书群机器人，或通用 Webhook），填 Webhook 地址；机器人开了「加签」就填密钥，设了「自定义关键词」就填关键词（消息里没有时自动加在开头）。用「发送测试通知」确认收到后保存。没有单独的开关：填了 Webhook 地址就是开启（铃铛变绿），清空地址再保存就是关闭。循环执行（多于一轮）每一轮都成功时会发一条「循环执行完成」，整个循环只发一条；单次运行成功、被手动停止都不会发，失败按失败告警发。不兼容变更：设置里原来的「脚本失败时发送通知」「循环执行全部完成时也通知」两个开关已删除，旧设置文件里的 `enabled` / `onLoopDone` 会被忽略，只要里面有 Webhook 地址就会开启通知（包括原来关着开关但留着地址的情况）。
  - **什么时候发**：脚本因为没有被处理的失败而中止时发一条，内容有脚本名、出错节点、原因（带中文提示）、第几轮 / 共几轮、已执行步数与时长。包括：节点失败且没接「失败 / 超时」出口、截图卡住、驱动错误、步数或访问次数上限。
  - **什么时候不发**：接了「失败 / 超时」出口的失败算脚本自己处理了的，不通知；手动停止不通知；运行开始前的准备失败（缺素材、校验不通过）也不通知，因为那时人在场。
  - **循环执行**：任何一轮里发生上面那种失败，整个循环立即停止，不再跑剩下的轮次，通知里会写「已停止循环，剩余 N 轮不会再执行」。
  - **失败了怎么办**：发送失败（网络、关键词不对、签名不对）只会在运行日志里记一条 `alert_failed`，不影响运行结果；网络类错误和 5xx 会自动重试 2 次。Webhook 地址是机密：明文存在本机 `~/.liverguard/settings.json`，不会写进日志，也不在导出文件里。
  - **命令行**：`liverguard run` 同样读这份设置并发通知，`--no-notify` 可以关掉；`liverguard notify-test` 发测试消息。
- 底部：运行日志、校验问题、探测结果。运行时节点按状态高亮，失败节点标红。
- 「探测」只截一帧并定位所有目标，不点击；`threshold`（默认 0.85；已存脚本里显式写了的值不受影响，没写的会随默认值变严）以下的匹配一律按未找到处理，`tap` 等动作不会点出去。
- 左右两侧面板和底部日志面板都可以拖动边缘调整大小，双击边缘还原默认；焦点在边缘时可用方向键微调（Shift 加大步长）。大小会记在本机，下次启动沿用。
- 快捷键：⌘S 保存，⌘Z / ⇧⌘Z 撤销与重做，⌘Enter 运行。

### 本地数据

默认在 `~/.liverguard/`，设置环境变量 `LIVERGUARD_HOME` 可改到别处：

- `scripts/`：一个脚本一个 JSON 文件（IR v3）。
- `assets/`：模板素材，按 SHA-256 命名的无损 PNG。
- `runs/`：运行记录，含汇总指标与 JSONL 日志。

**不兼容旧版本。** 本版直接覆盖旧的 Python 实现，不做迁移：旧的 IR v1 脚本无法解析，列表里不会出现它们（文件原样留在磁盘上，不会被删除）。

脚本不再有多个流程（画布上方的流程标签页已取消），「调用流程」节点及其底层逻辑（`call` 节点、`entry` / `flows` 字段、`call_depth` / `unknown_flow` 错误码、运行事件里的 `flow` 字段）全部移除，IR 升为 v3：图直接放在顶层 `flow` 字段里。含 `flows` 的 IR v2 脚本同样无法解析，列表里不会出现（文件留在磁盘上），需要手工把某个流程的 `nodes` / `edges` 搬到 `flow` 下并把 `irVersion` 改成 3；含 `call` 节点的要先去掉。

点击 / 长按节点不再带延时字段（`delayMs` / `delayRangeMs` 已移除，需要等待请接「等待时间」节点）。已存脚本里的旧延时字段会被忽略（不报错，等于没有延时）。

「等待时间」节点的 `ms` 改为 `minMs` / `maxMs`：每次运行在这个范围内随机取一个整数毫秒（两者相同就是固定等待），界面上按秒输入。旧脚本里的 `ms` 会被忽略，该节点按默认的 1 秒加载，需要重新填写。

点击 / 长按节点的「点位随机偏移」（`offsetRatio`）先改为 `randomPoint` 开关，现在进一步并入 `human`（「操作方式：拟人」），点击、长按、滑动三类节点都有。关闭时（默认）直接点锚点、精确注入；打开后：落点围绕锚点呈正态分布并限制在匹配框内，鼠标沿曲线从当前位置移到目标（带停顿，偶尔冲过头再修正），按下时长与滑动时长在设定值上下浮动，滑动的起终点也带抖动，细节见 `DESIGN.md`。拟人打开后属性面板里多一个「分散度」（`spread`，默认 1，推荐值，范围 0.3～2）：越小落点越贴近锚点，越大越散向整个目标框，始终不出匹配框；1 与此前固定的分布一致，旧脚本没有这个字段时按 1 读取。这个数值是按人机交互文献取的经验值，不是用真人数据标定的。旧脚本里的 `randomPoint` 会按 `human` 读取（打开着的仍是打开）；注意旧的「框内均匀随机」已被替换，落点更集中，且打开拟人后每次点击会多出移动与停顿的耗时（通常几百毫秒）。`offsetRatio` 仍被忽略。

「脚本设置」弹窗已删除，对应的两项能力一并移除：脚本里的「变量初始值」（IR 的 `vars` 字段）和「最长运行时间」（`meta.maxDurationMs`，引擎不再有运行总时长上限）。变量现在只由「设置变量」节点在运行中产生，每轮开始时清空，没设过的变量在条件里按未定义处理（`==` 与大小比较为假，`!=` 为真）。已存脚本里的 `vars` 与 `maxDurationMs` 会被忽略，下次保存时丢弃；依赖初始值的脚本需要在开头加一个「设置变量」节点。

运行画面的选择从右侧「画面」面板挪到了顶部栏，并且不再默认「整个屏幕」：必须先选才能运行、探测或预览（之前默认整个屏幕，容易在没留意的情况下对全屏点击）。

「整理」换成了 ELK 布局，节点卡片的外观有两处小变化：卡片头和缩略图行改成固定高度，输入口从卡片垂直居中移到第一个输出行上。旧脚本不受影响（位置字段不变），需要重新点一次「整理」才会按新规则排。依赖上用 `elkjs` 换掉了 `@dagrejs/dagre`。

属性面板里所有时长输入（按下 / 长按 / 滑动时长、等待出现与消失的超时和检测间隔、等待画面稳定的保持时间与超时、重试间隔、等待时间）统一以秒为单位输入，脚本里仍存毫秒（`durationMs`、`timeoutMs`、`intervalMs`、`ms`、`backoffMs`、`minMs`、`maxMs`），引擎按毫秒执行，旧脚本不受影响；输入会四舍五入到整毫秒。探测表里的耗时、运行日志里的 `*Ms` 字段是程序输出，仍以毫秒显示。

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

上面这条命令的产物是 ad-hoc 签名：macOS 按二进制哈希认应用，**每次重新打包都会被当成新应用，屏幕录制 / 辅助功能的授权随之失效**。本机反复打包调试时用：

```sh
cd /Users/advancer-sunkai/Documents/workspace/code/sky/liverguard   # 仓库根目录，换机器时改成自己的路径
source /tmp/ocvenv.sh             # 先加载 OpenCV 环境，不加载编译会找不到 OpenCV（文件不存在时见 AGENTS.md 重建）
scripts/mac-dev-build.sh          # 首次会在登录钥匙串里创建自签名证书 "LiverGuard Dev"，之后用它给每次打包签名
scripts/mac-dev-build.sh setup    # 只创建证书，不打包
```

本机开发测试的日常流程就是前三行：进仓库、加载 OpenCV 环境、跑 `scripts/mac-dev-build.sh`，产物在 `target/release/bundle/macos/LiverGuard.app`。

签名身份变成「`com.sky.liverguard` + 这张证书」，重新打包后不变，授权一次即可保留。脚本结束时会打印 `designated =>` 一行，里面不应出现 `cdhash`。切换到这张证书后的第一次仍要重新授权；旧的 “LiverGuard” 条目可以在系统设置里删掉，或执行 `tccutil reset ScreenCapture com.sky.liverguard`、`tccutil reset Accessibility com.sky.liverguard`。用 `LIVERGUARD_SIGN_IDENTITY` 可以换证书名。这是自签名，只适合本机；产物不会被公证，别人电脑上 Gatekeeper 仍会拦截。**未验证**：多次重新打包后授权确实保留，需要在真机上连续打包两次确认。

对外分发前还需要：

- macOS：Developer ID 签名、`notarytool` 公证、装订（`stapler`）。`src-tauri/entitlements.plist` 已按 hardened runtime 准备，不需要额外 entitlement。设置 `APPLE_SIGNING_IDENTITY`、`APPLE_ID`、`APPLE_PASSWORD`、`APPLE_TEAM_ID` 后 `tauri build` 会自动签名并公证。
- Windows：代码签名证书（`bundle.windows.certificateThumbprint`）；WebView2 运行时在 Windows 11 自带，Windows 10 需要随安装包引导安装。

## 持续集成与发布

### 每次提交的检查（CI）

[`.github/workflows/ci.yml`](.github/workflows/ci.yml) 在每个 Pull Request 和推送到 `main` 时运行，新提交会取消同一分支上还在跑的旧运行：

| 任务 | 内容 |
| --- | --- |
| Rust format | `cargo fmt --all -- --check`，**强制**；本地用 `make fmt` 格式化 |
| Web checks | `pnpm typecheck`、`pnpm test`，以及 `scripts/ci/` 里更新清单脚本的测试 |
| Rust macos-arm64 / windows-x64 | 构建 OpenCV 与前端后跑 `cargo clippy --workspace --all-targets` 和 `cargo test --workspace --all-targets`。**clippy 目前只显示警告、不会让 CI 失败**：仓库里已有约 12 条（`liverguard-vision` 6、`liverguard-engine` 4、桌面 crate 1、`liverguard-driver-win` 1），修它们会改动匹配与引擎代码，所以没有顺手改；清掉之后在命令末尾加 `-- -D warnings` |

Rust 任务的环境步骤（libclang、MSVC、OpenCV 缓存与构建、Windows 的 `-L native=` 处理）是从 `release.yml` 的 `build` 任务抄出来的，**改一处要同步另一处**。OpenCV 缓存的 key 与发布相同，CI 和发布共用同一份。缓存规则：Pull Request 写入的缓存只有它自己能读，`main` 上写入的缓存所有 PR 都能读，所以合入 `main` 后的第一次运行最慢。

### 发布

[`.github/workflows/release.yml`](.github/workflows/release.yml) 在 GitHub Actions 上打包 macOS（Apple 芯片，`macos-26`，产物最低支持 macOS 14；必须用 macOS 26 的 SDK 编译，因为 `screencapturekit` 依赖的 `apple-metal` 需要它，`macos-15` 会编译失败；`macos-14` 镜像已于 2026-11-02 下线）和 Windows（x64，`windows-2022`）。仓库是公开的，标准 runner 不计费。只做这两个目标：**不支持 Intel Mac**（GitHub 的 Intel 镜像只保证到 2027 年 8 月，决定不做）、也没有 Windows ARM 的包。

| 触发 | 结果 |
| --- | --- |
| 推送 `v*` 标签 | 先检查标签与版本号一致，再打两个平台的包，最后在 Releases 里创建**草稿**（含 `SHA256SUMS.txt` 和安装说明），确认后手动 Publish |
| Actions 页面手动运行（Run workflow） | 只构建，安装包留在该次运行的 Artifacts 里，保留 14 天，不创建 Release |

发版步骤：

```sh
# 1. 同步改这三处版本号：apps/desktop/src-tauri/tauri.conf.json、根 Cargo.toml 的 [workspace.package]、apps/desktop/web/package.json
node scripts/ci/check-version.mjs v0.2.1   # 本地先自检（版本号一致才通过）
git tag v0.2.1 && git push origin v0.2.1
```

产物：macOS 是 `*_aarch64.dmg`，Windows 是 NSIS 的 `*-setup.exe`；另外有应用内更新用的 `LiverGuard.app.tar.gz`、`*-setup.exe.sig` 等签名文件和 `latest.json`，见下面「应用内更新」。发布前先确认 Actions Secret `TAURI_SIGNING_PRIVATE_KEY` 已设置，没有的话第一个任务就会失败。

说明与限制：

- **没有代码签名**：macOS 是 ad-hoc 签名、没有公证，Windows 没有证书。用户首次打开要手动放行，步骤写在 `.github/release-notes.md`（会进入每个 Release 的正文）。接 Developer ID 与证书的做法见上面「对外分发前还需要」，需要时把证书放进仓库的 Secrets 再扩展 workflow。
- **首次构建很慢**：要先从源码编译 OpenCV 和整个 Rust 依赖，几十分钟；OpenCV 与 `target` 会被缓存。缓存的作用域规则是：标签触发的运行写入的缓存只有同一个标签能读，所以先在 `main` 上手动运行一次预热，之后的标签才能用上。
- 发布前会跑前端类型检查和测试，**不会跑 Rust 测试**（`cargo test --workspace` 请在本机发版前跑）。
- Windows 动态链接 C 运行库（`/MD`），用户机器需要 VC++ 运行库。
- **验证状况**：OpenCV 构建脚本在本机 macOS 上跑通，并用它的产物编译、测试过 `liverguard-vision`；workflow 通过了 `actionlint`。**这句话写于早期，已经过时**：维护者（2026-11-11）说明 Windows 的构建在 GitHub 上已经跑通，但这次整理没有去核对运行记录，具体结论以 GitHub Actions 上的实际运行为准；`DESIGN.md` 里的详细经过同样可能落后。**新增的 `ci.yml`、签名产物与 `latest.json` 的生成都还没有在 GitHub 上跑过**。

## 应用内更新

顶栏右侧的「更新」按钮（`apps/desktop/web/src/components/UpdateButton.tsx`）在应用启动时静默检查一次：有新版本就在按钮上显示绿点，点击后弹窗确认，再下载、安装并重启。没有新版本或网络不通时不打扰用户；手动点击会提示「已经是最新版本」或失败原因。脚本正在运行时不会安装。浏览器里的 `pnpm dev`（模拟后端）没有这个按钮。

实现用的是官方的 `tauri-plugin-updater` 与 `tauri-plugin-process`（重启），前端直接调用插件，没有新增 Rust 命令。检查地址是 `tauri.conf.json` 里的 `plugins.updater.endpoints`：`https://github.com/dailycoderlab/liver-guard/releases/latest/download/latest.json`。这个地址**只有 Release 被 Publish 之后才可访问**（草稿对客户端不可见），也只指向最新的非预发布版本，所以发布一个 `-beta` 标签并 Publish 后，所有用户都会收到它。

### 签名密钥

updater 会用 `plugins.updater.pubkey` 校验每个更新包的签名，这一步无法关闭。

```text
私钥  → 你自己的备份 + 仓库的 Actions Secret TAURI_SIGNING_PRIVATE_KEY
公钥  → tauri.conf.json 的 plugins.updater.pubkey（minisign 公钥，ID E4313EDFB51A9F07）
```

- 密钥对无密码，所以**不要**建 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 这个 Secret。
- **丢失私钥，或换一对新密钥，会让所有已安装的版本再也无法自动更新**，只能手动重装。换密钥前先确认有这个代价。
- 生成（只在需要新密钥时做）：`cd apps/desktop/web && ./node_modules/.bin/tauri signer generate --ci -w ../../../.tauri/liverguard.key`。`.tauri/` 已被 `.gitignore` 忽略。把 `.pub` 文件的内容原样（它本身就是 base64 文本，不要再编码一次）写进 `pubkey`，把私钥文件全文粘贴进 Secret，备份后删掉工作目录里的两个文件。
- 本机 `tauri build`（包括 `scripts/mac-dev-build.sh`）不生成更新包，所以不需要私钥；`createUpdaterArtifacts` 只在 `release.yml` 里通过 `--config` 打开。

### 发布时生成什么

`release.yml` 在打包时用私钥给更新包签名（macOS 必须用 `--bundles app,dmg`：只写 `dmg` 时 Tauri 会警告“没有启用更新的目标”，不生成 `.app.tar.gz`，这一点已在本机实测）：macOS 是 `LiverGuard.app.tar.gz` 加 `.sig`，Windows 的 NSIS 安装包本身就是更新包，旁边是 `*-setup.exe.sig`。`release` 任务再用 [`scripts/ci/generate-update-manifest.mjs`](scripts/ci/generate-update-manifest.mjs) 生成 `latest.json`（`darwin-aarch64` 与 `windows-x86_64` 两个平台，缺任何一个更新包或签名都会失败），和安装包一起上传到草稿 Release。

### 限制与未验证

- **已安装的 v0.2.0 不含更新功能**，需要手动装一次带更新功能的版本，之后才会自动更新。
- macOS 是 ad-hoc 签名：更新后的应用在系统看来可能是一个新应用，**屏幕录制和辅助功能的授权可能要重新给一次**（和重新打包时的现象相同）。Windows 上以管理员身份运行的 LiverGuard 更新时可能需要提权，安装模式是 `passive`（显示进度、不需要点击）。
- **没有验证**：从旧版本更新到新版本的完整流程（至少要发布两个版本才能验证）、`latest.json` 在真实 Release 上的内容、Windows 的更新包与重启、更新后的权限授权是否保留。已验证的只有：前端状态机与清单脚本的单元测试、Rust 侧能编译，以及本机（macOS arm64）带私钥执行 `tauri build --bundles app,dmg --config '{"bundle":{"createUpdaterArtifacts":true}}'` 确实产出 `LiverGuard.app.tar.gz` 和 `.sig`，签名里的密钥 ID 与 `pubkey` 对应；Windows 的更新包产物没有在本机验证。

