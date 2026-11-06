# Sky 架构

Sky 是纯本地工具：画布编辑、截图、图像识别、脚本执行和全部数据都在桌面客户端内完成，没有服务端、账号或云同步。启动、打包和使用说明见 [README.md](README.md)。

## 分层

```
apps/desktop/web        React 19 + @xyflow/react：画布、属性面板、实时画面、日志
        │  invoke / Channel（JSON 命令；二进制画面帧）
apps/desktop/src-tauri  Tauri 2 外壳：命令、实时画面线程、运行线程、冻结帧
        │
crates/sky-engine ── Driver 抽象 + 图解释器
crates/sky-vision ── 模板匹配、控件检测、模板裁剪（OpenCV）
crates/sky-ir     ── IR v2 数据模型与校验（也是前端类型的来源）
crates/sky-store  ── ~/.sky 本地存储
crates/sky-driver-mac / sky-driver-win ── 各平台的截图、输入、窗口、权限
crates/sky-cli    ── 与桌面应用共用同一套核心的命令行
```

平台相关代码只存在于两个 `sky-driver-*` crate 和外壳里的 `platform.rs`、CLI 里的 `platform.rs`；匹配、等待、分支、重试、报告都在平台无关的 `sky-engine` / `sky-vision` 里，两个平台跑的是同一份代码。

## 脚本 IR v2

一个脚本是若干命名**流程**（`flows`），每个流程是一张有向图：

- 节点 `{ id, type, pos, label?, maxVisits?, data }`，连线 `{ id, source, sourceHandle, target }`；每个输出口至多一条线，允许成环。
- 输出口按节点类型固定：`tap` / `longPress` / `swipe` 为 `next`、`fail`；`waitFor` 为 `found`、`timeout`；`waitGone` / `waitStable` 为 `next`、`timeout`；`if` 为 `true`、`false`；`loop` 为 `body`、`done`；`retry` 为 `body`、`done`、`failed`；`switch` 为 `case:<目标>` 与 `default`；`end` 没有输出；其余为 `next`。
- 未接线的 `fail` / `timeout` / `failed` 输出口不是静默继续，而是让运行以对应原因中止（日志里带节点 id）。
- 目标（`targets`）分模板目标和坐标点目标。模板目标记录素材 SHA-256、素材像素尺寸、录制时的缩放与逻辑宽度、搜索范围（归一化）、锚点、匹配策略与阈值；坐标点目标只记录屏幕比例位置，不做图像匹配。
- 条件可以引用目标可见性、变量比较，并可用 `all` / `any` / `not` 组合。

`sky-ir` 用 `schemars` 导出 JSON Schema，`apps/desktop/web/scripts/gen-types.mjs` 把它转成 `src/ir/generated.ts`，前后端对脚本的理解来自同一处定义。

## 解释器

`sky-engine` 的 `Interpreter` 沿连线走图，每个节点按 `Driver` 的几个原语（`bounds` / `capture` / `tap` / `swipe` / `key` / `text`，另有可选的 `ax_at`）执行。保护机制：

- 每个节点可设 `maxVisits`；`loop` 无限循环有迭代上限；`call` 递归有深度上限；整体有 `maxDurationMs`。
- 连续多个动作而画面没有变化会触发**停滞保护**。
- 截图耗时异常（macOS 在权限丢失时会卡住而不是报错）会被识别并以可操作的原因中止。
- `sleep` 和等待会响应停止请求与整体超时，不会睡过头。
- 事件订阅者 panic 不会中断运行。
- 每次运行产生 JSON Lines 日志与按（步骤、目标）聚合的匹配得分，供界面和 `runs/` 使用。

## 匹配

模板匹配的流程是：把当前画面和模板都归一化到同一个规范宽度（480 逻辑单位），在目标的搜索范围内做模板匹配（`tpl`，失败再用多尺度 `mstpl`，可选 `sift`），再做 RGB 颜色校验，最后按 `multi` 规则选一个结果。低于目标 `threshold` 的结果按未找到处理，`tap` / `longPress` / `swipe` 以 `not_found` 中止且不注入输入；`waitGone` 与条件判断不受阈值拦截——低分说明画面在变化，判成“已消失”会得出错误结论。

已知特性：因为先归一化到 480 宽，在很高分辨率的画面上，哪怕模板就是从同一帧里裁出来的，得分也可能只有 0.8 左右（在 5000 像素宽的窗口截图上实测 0.787，位置正确）。这是沿用旧实现的算法，在这个样例上 Rust 与旧 Python 的结果完全一致；若要提高区分度，需要调整规范宽度，这会改变所有已存脚本的匹配行为，因此没有改。

## 桌面外壳

- **实时画面**：`live_start` 起一个线程循环截图，缩成不超过 1280 宽的 JPEG，通过 `ipc::Channel` 以二进制发给前端。每帧前有 48 字节头：`seq u64` + `x, y, w, h, scale`（`f64`，小端）。
- **冻结与裁剪**：`snapshot` 重新截一张全分辨率 PNG 并在后端固定住，返回它的 `seq`；之后的 `detect_candidates`、`create_template` 都凭 `seq` 使用这张固定的帧，帧过期会明确报错。`Frame` 内含缓存、不是 `Send`，所以跨线程传的是像素 `Mat` 和边界，需要时在目标线程重建。
- **运行**：`run_start` 在独立线程里跑 `run_local`，事件通过 `Channel` 推给前端驱动节点高亮；结束时落盘运行记录并发送 `done`。同一时间只允许一个运行。
- 耗时命令都不在主线程执行。

## 前端

- 画布使用 `@xyflow/react`，状态在 Zustand 的 `editor` store；撤销/重做、脏标记、保存、关闭拦截都在 store 与 `App.tsx` 里，图操作（加点、连线规则、整理）是纯函数，有单元测试。
- 前端命令层 `src/api/`：`tauri.ts`（真实后端）与 `mock.ts`（浏览器里的模拟）实现同一个 `Backend` 接口，`pnpm dev` 不需要桌面壳也能开发界面。
- Tauri 命令是手写类型（`src/api/types.ts`），没有用 `tauri-specta`：它面向 Tauri v1，这里只有二十来个命令，手写更省事。IR 本身的类型由 Rust 生成，不会漂移。

## 本地数据

所有状态都在 `~/.sky/`（`SKY_HOME` 可覆盖），由 `sky-store::LocalStore` 原子写入：

- `scripts/<id>.json`：脚本全部内容，没有草稿/版本之分，保存即覆盖。
- `assets/<sha256>.png`：模板素材，按内容寻址，缺失即报错，不会去网络补取。
- `runs/<id>.json`：运行记录，含状态、耗时、匹配得分汇总和日志。

脚本可随时覆盖保存，所以历史运行记录反映的是当时运行的脚本，不保证与当前内容一致。

## 与原方案的偏离与验证状况

| 项 | 说明 |
| --- | --- |
| 类型生成 | 计划写 `tauri-specta`，实际 IR 类型用 `schemars` → JSON Schema → TypeScript，命令手写。原因见上。 |
| 旧实现 | Python / PySide6 实现已整体删除，不迁移旧脚本；旧 pytest 里与界面相关的用例随之作废，与匹配、解释器相关的场景由 Rust 的对拍夹具（13 个匹配场景 + 2 个坐标点场景）和 `sky-engine` 的 40 个图测试覆盖。 |
| OpenCV 版本 | 对拍夹具由 OpenCV 5.0.0 生成，Rust 侧用 4.12.0；在得分 ±0.01、位置 ±1 像素内一致。 |
| macOS 真机 | 已在本机验证：权限检测、全屏与单窗口 ScreenCaptureKit 截图（Retina 缩放 2.0）、CGEvent 点击（点击落点与预期的全局坐标一致）、`sky doctor/windows/capture/probe/run`，以及桌面应用里的列表、画布、实时预览、冻结、候选检测、模板创建、探测、运行（含失败节点高亮与运行记录落盘）。 |
| 真实匹配 | 在真实窗口截图上，Rust 与旧 Python 实现得分完全一致（0.7874644994735718）。 |
| 打包体积 | macOS arm64 release 的 `Sky.app` 为 14 MB，OpenCV 静态链接，只依赖系统框架（WebKit、ScreenCaptureKit 等）；同类 PyInstaller 打包通常是几百 MB（未在本机量过）。 |
| Windows | **未验证。** `sky-driver-win` 的平台层（不含 OpenCV 的部分）通过了 `cargo check --target x86_64-pc-windows-msvc --no-default-features`；`Driver` 实现、`apps/desktop/src-tauri` 里 Windows 分支、Windows Graphics Capture 的实际截图、`SendInput`、WebView2 与 WKWebView 的画布一致性、Windows 安装包体积，都没有在真实 Windows 上运行过。Windows 输入使用物理像素坐标，也还没在多显示器、不同缩放下验证。 |
| 签名与公证 | 产物只有 ad-hoc 签名，Developer ID 签名和公证没有做（需要证书与 Apple 账号）。 |
| 画面一致性 | 画布在 macOS 的 WKWebView 里渲染正常；与 Windows WebView2 的对比无法在本机完成。 |
| 移动端 | 不做。`Driver` 抽象为以后接 ADB / iOS 留了位置，但仓库里没有任何移动端代码。 |

## 当前边界

- 仅 macOS 和 Windows 桌面。
- 截图、输入和窗口焦点受操作系统权限、其他窗口遮挡及应用保护机制影响，无法保证所有应用均能自动化。
- 全屏模式下 Sky 自己的窗口也会出现在画面里；运行脚本前应把目标窗口放到前面，或改用单窗口模式。
- 脚本不会跨机器同步：换电脑需要自行复制 `~/.sky/`，模板素材也必须一并带走。
- 列表里看不到无法解析的脚本（例如旧的 v1 文件）；它们不会被删除，也不会提示。
