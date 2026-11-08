# LiverGuard 架构

LiverGuard 是纯本地工具：画布编辑、截图、图像识别、脚本执行和全部数据都在桌面客户端内完成，没有服务端、账号或云同步。启动、打包和使用说明见 [README.md](README.md)。

## 分层

```
apps/desktop/web        React 19 + @xyflow/react：画布、属性面板、实时画面、日志
        │  invoke / Channel（JSON 命令；二进制画面帧）
apps/desktop/src-tauri  Tauri 2 外壳：命令、实时画面线程、运行线程、冻结帧
        │
crates/liverguard-engine ── Driver 抽象 + 图解释器
crates/liverguard-vision ── 模板匹配、控件检测、模板裁剪（OpenCV）
crates/liverguard-ir     ── IR v3 数据模型与校验（也是前端类型的来源）
crates/liverguard-store  ── ~/.liverguard 本地存储
crates/liverguard-driver-mac / liverguard-driver-win ── 各平台的截图、输入、窗口、权限
crates/liverguard-cli    ── 与桌面应用共用同一套核心的命令行
```

平台相关代码只存在于两个 `liverguard-driver-*` crate 和外壳里的 `platform.rs`、CLI 里的 `platform.rs`；匹配、等待、分支、重试、报告都在平台无关的 `liverguard-engine` / `liverguard-vision` 里，两个平台跑的是同一份代码。

## 脚本 IR v3

一个脚本就是一张有向图（顶层字段 `flow`，含 `nodes` / `edges`），没有多流程，也没有「调用流程」节点：脚本里的复用靠 `loop` / `retry` 与显式回连。图里恰好一个 `start`，到达 `end` 即结束当前这一轮（只跑一轮时就是整个运行）：

- 节点 `{ id, type, pos, label?, maxVisits?, data }`，连线 `{ id, source, sourceHandle, target }`；每个输出口至多一条线，允许成环。
- 输出口按节点类型固定：`tap` / `longPress` / `swipe` 为 `next`、`fail`；`waitFor` 为 `found`、`timeout`；`waitGone` / `waitStable` 为 `next`、`timeout`；`if` 为 `true`、`false`；`loop` 为 `body`、`done`；`retry` 为 `body`、`done`、`failed`；`switch` 为 `case:<目标>` 与 `default`；`end` 没有输出；其余为 `next`。
- 未接线的 `fail` / `timeout` / `failed` 输出口不是静默继续，而是让运行以对应原因中止（日志里带节点 id）。
- 目标（`targets`）分模板目标和坐标点目标。模板目标记录素材 SHA-256、素材像素尺寸、录制时的缩放与逻辑宽度、搜索范围（归一化）、锚点、匹配策略与阈值；坐标点目标只记录屏幕比例位置，不做图像匹配。
- 条件可以引用目标可见性、变量比较，并可用 `all` / `any` / `not` 组合。

`liverguard-ir` 用 `schemars` 导出 JSON Schema，`apps/desktop/web/scripts/gen-types.mjs` 把它转成 `src/ir/generated.ts`，前后端对脚本的理解来自同一处定义。

## 解释器

`liverguard-engine` 的 `Interpreter` 沿连线走图，每个节点按 `Driver` 的几个原语（`bounds` / `capture` / `tap` / `swipe` / `key` / `text`，拟人输入用的 `pointer`，另有可选的 `ax_at` / `pointer_position` / `check_input`）执行。保护机制：

- 每个节点可设 `maxVisits`；`loop` 无限循环有迭代上限；整体**没有**运行总时长上限（已移除 `maxDurationMs`），挂机循环要靠手动停止、失败告警和停滞保护兜底。
- **整脚本循环**：轮数是运行参数而不是脚本内容（不在 IR 里，不随脚本保存）：桌面端 `run_start` 的 `repeat`、CLI 的 `--repeat`，最终落到 `Interpreter::set_rounds`（至少 1）。引擎把整张图从 `start` 起连续走这么多轮。每轮开始时变量清空（脚本不再有「变量初始值」，变量只由「设置变量」节点在运行中产生；没设过的变量在条件里按未定义处理：`==` 与比较为假，`!=` 为真），`maxVisits` 的计数、停滞检测和画面缓存清零；`MAX_STEPS` 按单轮计；没有总时长预算，轮数再多也不会被超时截断。某一轮失败、超时或被手动停止，整个运行终止，剩余轮次不再执行。轮与轮之间不插入等待，需要间隔就在图尾加「等待」节点。轮数大于 1 时日志里每轮开头有 `round_start`（`round` / `total`），`run_done` 带 `rounds`。
- **失败告警**（`liverguard-engine::alert`、`liverguard-notify`）：运行以 `RunState::Failed` 结束时，若设置里开了通知，就向 Webhook 发一条文本。放在「整个运行」这一层而不是某个节点上，原因：
  1. 挂机场景要的是兜底。节点级的做法要求每个节点都记得接「失败」出口再接一个通知节点，漏接一个就静默失败，而且和「失败出口是用来走恢复分支」的语义混在一起。
  2. 截图卡住、驱动错误、步数上限这些故障不属于任何一个节点的「失败」出口（`Reason::is_routable` 为假），只有运行级才拦得住。
  3. Webhook 地址是本机的机密，不该写进脚本（脚本会导出分享）；整个应用一份设置，所有脚本自动生效，CLI 与桌面端共用 `settings.json`。
  脚本接了 `fail` / `timeout` 出口的失败属于脚本自己的控制流，不通知；`Stopped` 不是失败，也不通知。多轮运行里任一轮失败即终止并告警，消息里带轮次。**循环完成通知**：轮数大于 1 的运行全部轮次成功（`RunState::Done`）时，再发一条「循环执行完成」（脚本名、轮数、步数、总用时）；没有单独的开关（原 `onLoopDone` 已删除）；单轮运行成功、被手动停止、失败都不发这一条。整个循环只发一条，不是每轮一条。通用 Webhook 的 `event` 字段区分 `run_failed` / `loop_done` / `test`。日志里 `alert_sent` / `alert_failed` 带 `event`。发送失败只记 `alert_failed` 日志，不改变运行结果；`alert_sent` 记成功。以后如果需要「脚本走到某一步就主动通知」（例如恢复分支里报个信），再单独加一个 `notify` 节点，复用同一份设置，与兜底告警并存。
- 连续多个动作而画面没有变化会触发**停滞保护**。
- 截图耗时异常（macOS 在权限丢失时会卡住而不是报错）会被识别并以可操作的原因中止。
- `sleep` 和等待会响应停止请求与整体超时，不会睡过头。
- `sleep` 节点存 `minMs` / `maxMs`（整数毫秒），每次执行在闭区间内均匀随机取一个整数毫秒并写进 `sleep` 日志；`minMs > maxMs` 校验为错误。界面以秒输入，保存时四舍五入到毫秒。
- **拟人输入。** `tap` / `longPress` / `swipe` 的 `human` 为真时，不再调用 `Driver::tap` / `swipe` 一次性注入，而是由 `liverguard-engine/src/human.rs` 规划一串带时间的原始指针事件（`PointerAction::{Move, Down, Drag, Up}`），解释器按计划逐个发给 `Driver::pointer` 并可随时响应停止。规划是纯函数，平台无关，用固定种子的 RNG 测试；平台层只发事件，不做规划。不勾选时仍走原来的精确 `tap` / `swipe`，行为不变。拟人的各部分如下：
  - **落点**：以目标锚点为中心的二维正态，x、y 独立，标准差 `σ = 框的宽或高 / 4.133 × spread`（`spread` 是节点上的「分散度」，默认 1.0，范围 0.3～2.0，超出范围的值在引擎里夹回、非数值按 1.0；校验对范围外的值报错。1.0 对应 Fitts 定律的有效宽度 `We = 4.133σ`，人手点击落点约 96% 落在目标宽度内，也就是此前固定的行为）；`spread` 越大越接近在框内均匀取点，越小越贴近锚点；落到匹配框外的采样丢弃重抽（最多 16 次，仍失败则夹回框内），所以点不会出框。正态用 Box–Muller 自己生成，没有引入新依赖。
  - **按下时长**：对设定值 `durationMs` 取对数正态（σ=0.22），限制在 0.6～1.8 倍，取整毫秒、至少 1ms；`durationMs` 为 0 则保持 0。界面上拟人的点击节点不显示「按下时长」输入框（它本来就是随机的），此时仍以节点里已存的 `durationMs`（新建默认 60）作为浮动的中心值；长按和滑动仍显示各自的时长，因为那是有意义的设定。
  - **鼠标移动**：从 `Driver::pointer_position` 取当前位置，沿三次贝塞尔弧线移到落点：控制点偏离直线最多 `距离 × 12%`（上限 120px），20% 概率换边成 S 形；沿路径的位置按最小加加速度曲线 `10t³−15t⁴+6t⁵`（起步慢、中段快、到达慢）；每约 8ms 一个事件（±25% 抖动，约 125Hz）；叠加手抖（正态 0.35px，起止处淡出，终点精确落在落点）。耗时按 Fitts 定律 `120 + 110·log2(1 + 距离/目标宽度)` ms，乘对数正态噪声（σ=0.15），限制在 120～1500ms。距离 ≥ 220px 时 30% 概率冲过头 1.5%～5%（4～30px）再用一小段修正回来。到达后停顿 25～250ms（对数正态，中位约 70ms）再按下。平台读不到当前位置时（`pointer_position` 返回 `None`），指针直接出现在落点，不会凭空编一条路径。
  - **滑动**：起点、终点各在自己的目标框里按上面的方式取点；只给向量时，终点是「起点 + 向量」再叠加正态误差（σ = 滑动长度的 3% × `spread`，两个轴各一份），并夹在截图区域内。先拟人地移到起点，按下后停顿 20～200ms，再沿弧线（弯曲幅度约为移动时的 40%，手抖 0.5px）拖到终点，总时长对 `durationMs` 取对数正态（σ=0.12，限制在 0.8～1.35 倍），松开前再停 15～150ms。
  - **安全**：拟人事件序列开始前调用一次 `Driver::check_input`（macOS 上就是原来的「窗口在前、点在窗口内、没被遮挡」检查，对象是所有按下、拖动、松开的点；仅悬停的移动不检查）。运行中被停止、超时或出错时，如果鼠标键正按着，会先补发一次松开，不会把键卡在按下状态。检查与实际事件之间窗口可能被挪动或遮挡（最长约 2 秒），这一点比原来的一次性注入弱。
  - **日志**：`tap` 记录 `human`、`travelMs`、`holdMs`、`overshoot`、`spread`；拟人滑动另记 `swipe`（`fromX/fromY/toX/toY`、`travelMs`、`strokeMs`、`spread`）。`dx` / `dy` 仍是实际点击点相对锚点的偏离。
  - **未做 / 未验证**：这些分布和参数是按人机交互文献（Fitts 定律、最小加加速度模型）取的经验值，没有用真人鼠标轨迹数据拟合或校准；没有模拟触屏特有的偏置（例如偏下）、双击、键盘节奏；不保证能通过任何具体的反作弊或行为检测。真机上的拟人移动与点击只做了编译与单元、图测试，没有在真实桌面上点过。
- 事件订阅者 panic 不会中断运行。
- 每次运行产生 JSON Lines 日志与按（步骤、目标）聚合的匹配得分，供界面和 `runs/` 使用。

## 匹配

模板匹配的流程是：把当前画面和模板都归一化到同一个规范宽度（480 逻辑单位），在目标的搜索范围内做模板匹配（`tpl`，失败再用多尺度 `mstpl`，可选 `sift`），再做 RGB 颜色校验，最后按 `multi` 规则选一个结果。低于目标 `threshold` 的结果按未找到处理，`tap` / `longPress` / `swipe` 以 `not_found` 中止且不注入输入；`waitGone` 与条件判断不受阈值拦截——低分说明画面在变化，判成“已消失”会得出错误结论。

已知特性：因为先归一化到 480 宽，在很高分辨率的画面上，哪怕模板就是从同一帧里裁出来的，得分也可能只有 0.8 左右（在 5000 像素宽的窗口截图上实测 0.787，位置正确）。这是沿用旧实现的算法，在这个样例上 Rust 与旧 Python 的结果完全一致；若要提高区分度，需要调整规范宽度，这会改变所有已存脚本的匹配行为，因此没有改。

## 桌面外壳

- **实时画面**：`live_start` 起一个线程循环截图，缩成不超过 1280 宽的 JPEG，通过 `ipc::Channel` 以二进制发给前端。每帧前有 48 字节头：`seq u64` + `x, y, w, h, scale`（`f64`，小端）。
- **冻结与裁剪**：`snapshot` 重新截一张全分辨率 PNG 并在后端固定住，返回它的 `seq`；之后的 `detect_candidates`、`create_template` 都凭 `seq` 使用这张固定的帧，帧过期会明确报错。
- **二次框选已有目标**：`refine_template` 命令按模板图片自身的像素坐标裁出子区域（`author::refine_template`），只换 `asset` / `asset_box`，其余字段（`roi`、`record_scale`、`record_width`、阈值、`anchor` 比例等）原样保留；新裁区域落在旧框内、旧框落在 `roi` 内，所以 `roi` 不需要改。旧素材不删，编辑器撤销可用。`anchor` 是相对框的比例，裁小后点击点会随新框重新按比例落位（默认居中则仍居中）。**未验证**：裁小后在真实画面上的匹配得分与点击位置；前端弹窗只做过类型检查和纯函数测试，没有在 Tauri 窗口里手工操作过。
- **上传截图**：`load_image` 接收文件原始字节，用 `liverguard_vision::author::decode_image` 解码后固定为同样的 `seq` 帧，之后走同一条框选与裁剪路径。没有真实表面，所以按 1x、整幅图当作整个表面处理（`record_scale = 1`，`record_width` 为图片像素宽）；匹配按宽度归一化，因此分辨率不必和运行时一致，但图片必须是完整表面截图而不是裁剪。该帧标记为 `external`，`detect_candidates` 不再查询系统辅助功能。**未验证**：上传的 Retina 截图与实机运行时的匹配得分，以及 WKWebView 里文件选择框的取消行为；尚未在 Tauri 窗口里手工跑过这条链路。`Frame` 内含缓存、不是 `Send`，所以跨线程传的是像素 `Mat` 和边界，需要时在目标线程重建。
- **运行**：`run_start` 在独立线程里跑 `run_local`，事件通过 `Channel` 推给前端驱动节点高亮；结束时落盘运行记录并发送 `done`。同一时间只允许一个运行。
- 耗时命令都不在主线程执行。

## 前端

- 画布使用 `@xyflow/react`，状态在 Zustand 的 `editor` store；撤销/重做、脏标记、保存、关闭拦截都在 store 与 `App.tsx` 里，图操作（加点、连线规则、整理）是纯函数，有单元测试。
- **整理布局**：用 `elkjs`（ELK 的 layered 算法，0.12.1，许可 EPL-2.0 OR GPL-3.0）替换了原来的 dagre。节点卡片的高度和每个输出口（handle）的高度是固定的（`ir/nodeBox.ts`，`LiverGuardNode.tsx` 用同一组数字：头 32、目标缩略图行 44、每个输出行 24），所以布局能把每个输出口作为固定位置的端口交给 ELK：分支按输出口顺序排布，只有一个后继的连线被放成一条直线；输入口画在第一个输出行上（结束节点没有输出行，输入口在头部），这样一串节点首尾对齐。环（循环、重试）按从开始节点深度优先断开，不连通的节点分开摆放，整理后画布自动适应视图。ELK 是异步的，首次整理时才加载它（约 1.4MB，gzip 后约 450KB 的独立分块）。连线仍由 React Flow 的 smoothstep 画，不用 ELK 算出的折线，所以回边（回到前面节点的连线）会斜穿画面，不会绕开其他节点。
- 前端命令层 `src/api/`：`tauri.ts`（真实后端）与 `mock.ts`（浏览器里的模拟）实现同一个 `Backend` 接口，`pnpm dev` 不需要桌面壳也能开发界面。
- Tauri 命令是手写类型（`src/api/types.ts`），没有用 `tauri-specta`：它面向 Tauri v1，这里只有二十来个命令，手写更省事。IR 本身的类型由 Rust 生成，不会漂移。

## 本地数据

所有状态都在 `~/.liverguard/`（`LIVERGUARD_HOME` 可覆盖），由 `liverguard-store::LocalStore` 原子写入：

- `settings.json`：应用级设置，目前只有运行通知（接收方、Webhook 地址、签名密钥、关键词；没有开关，填了 Webhook 地址就是开启，旧文件里的 `enabled` / `onLoopDone` 会被忽略）。明文保存，没有设文件权限；不属于任何脚本，也不进导出文件。读不出来时运行会在日志里写 `alert_unavailable` 并照常运行，不会悄悄当成「通知已关闭」。

- `scripts/<id>.json`：脚本全部内容，没有草稿/版本之分，保存即覆盖。
- `assets/<sha256>.png`：模板素材，按内容寻址，缺失即报错，不会去网络补取。
- `runs/<id>.json`：运行记录，含状态、耗时、匹配得分汇总和日志。

导出文件（`liverguard-store::ScriptBundle`）：`{ format: "liverguard-script", version: 1, name, ir, assets: { <sha256>: <base64 png> } }`，只带脚本实际引用的素材。导入先在内存里校验全部内容（格式、`version` 不高于当前、`irVersion` 等于当前、每个素材的 SHA-256 与 PNG 头、脚本引用的素材要么在文件里要么已在本机），通过后才写素材并以新 id 建脚本，重名时加后缀，不覆盖。桌面端的文件对话框由 Rust 侧的 `tauri-plugin-dialog` 弹出，文件内容不经过前端；浏览器 mock 用下载链接和文件选择框代替。

脚本可随时覆盖保存，所以历史运行记录反映的是当时运行的脚本，不保证与当前内容一致。

## 与原方案的偏离与验证状况

| 项 | 说明 |
| --- | --- |
| 类型生成 | 计划写 `tauri-specta`，实际 IR 类型用 `schemars` → JSON Schema → TypeScript，命令手写。原因见上。 |
| 旧实现 | Python / PySide6 实现已整体删除，不迁移旧脚本；旧 pytest 里与界面相关的用例随之作废，与匹配、解释器相关的场景由 Rust 的对拍夹具（13 个匹配场景 + 2 个坐标点场景）和 `liverguard-engine` 的 40 个图测试覆盖。 |
| OpenCV 版本 | 对拍夹具由 OpenCV 5.0.0 生成，Rust 侧用 4.12.0；在得分 ±0.01、位置 ±1 像素内一致。 |
| macOS 真机 | 已在本机验证：权限检测、全屏与单窗口 ScreenCaptureKit 截图（Retina 缩放 2.0）、CGEvent 点击（点击落点与预期的全局坐标一致）、`liverguard doctor/windows/capture/probe/run`，以及桌面应用里的列表、画布、实时预览、冻结、候选检测、模板创建、探测、运行（含失败节点高亮与运行记录落盘）。 |
| 真实匹配 | 在真实窗口截图上，Rust 与旧 Python 实现得分完全一致（0.7874644994735718）。 |
| 整脚本循环 | 多轮运行的引擎行为由 `liverguard-engine` 的图测试覆盖（轮数、`end` 只结束当前轮、变量与访问次数每轮重置、失败与停止终止全部轮次）；桌面窗口里的「循环」弹层没有在真实 Tauri 窗口里点过；CLI 的 `--repeat` 只确认了参数解析，没有真机跑过。 |
| 脚本导入导出 | 导出导入的打包逻辑由 `liverguard-store` 的单元测试覆盖（往返、不覆盖与重名、损坏/篡改/缺素材/版本不符时拒绝且不写入），并用 CLI 在临时目录里对真实脚本做过一次往返（4 张图，导入两次得到「名称」「名称（导入）」）。桌面端的系统保存/打开对话框（`tauri-plugin-dialog` 2.x）没有在真实窗口里点过；Windows 的对话框更没有验证。 |
| 失败告警 | 请求构造（钉钉加签与飞书签名用 Python 独立算出的参考值核对）、响应判定（钉钉 / 企业微信 / 飞书在 HTTP 200 里返回错误码也算失败）、重试与「错误里不含 URL」由 `liverguard-notify` 的单元测试覆盖，并用本机回环 HTTP 服务器测过真实的 POST、拒绝、5xx 重试、连接失败；引擎侧的触发条件（未处理失败发、接了出口的不发、手动停止不发、多轮在第几轮失败、发送失败不改结果、循环全部成功发一条完成通知且可关闭、单轮成功 / 失败的循环 / 停止的循环不发完成通知）由 `liverguard-engine` 的图测试覆盖。**没有对真实的钉钉 / 企业微信 / 飞书机器人发过消息**，三家的消息格式和签名规则按官方文档实现，是否被真实服务接受要靠你用「发送测试通知」确认；`ureq` 3（rustls）在 Windows 上的 TLS 也没验证。设置窗口只在浏览器 mock 里看过，没有在真实 Tauri 窗口里点过。 |
| 打包体积 | macOS arm64 release 的 `LiverGuard.app` 为 14 MB，OpenCV 静态链接，只依赖系统框架（WebKit、ScreenCaptureKit 等）；同类 PyInstaller 打包通常是几百 MB（未在本机量过）。 |
| 拟人输入 | 规划算法由 `human.rs` 的单元测试覆盖（落点分布与出框、分散度对散布的缩放与各取值下不出框、按下时长、弧线与速度曲线、耗时随距离与目标大小变化、冲过头、起点未知、拖动结构），解释器行为由 `tests/graph.rs` 的假驱动测试覆盖（原始事件顺序、点在框内、不同分散度的实际散布、日志里的 `spread`、检查先于事件、出错或停止时补发松开）。macOS 真机上用一次性的「只悬停、不按键」探针验证过 `pointer_position` 与 `Move`：约 460ms 的计划实际播放约 620ms，另一线程采样到光标沿弧线、起止慢中间快，最大偏离直线约 18px，终点与目标坐标一致（探针已删除）；`Down` / `Drag` / `Up` 的真机注入没有跑过（macOS 上它们与已验证过的 `tap` / `swipe` 用同样的 CGEvent 调用，但拟人拖动整体没点过）。Windows 的平台层只通过了交叉编译检查，`Driver` 实现没编译过，也没有发过事件。参数没有拿真实用户轨迹校准。 |
| 运行画面选择 | 选择器在顶部栏（`SurfacePicker`），初始为未选；`requireSurface()`（`store/actions.ts`）在运行 / 探测 / 开始预览前检查：没选则提示并高亮，所选窗口不在最新窗口列表里则重置为未选并提示。判定逻辑 `lib/surface.ts` 有单元测试；切换画面会停止实时预览。选择只存在内存里，不随脚本、不跨重启保存（窗口编号重开后会变，按标题记忆是另一个改动）。只在浏览器 mock 里点过（未选时点运行出现提示与高亮、选窗口后预览、切换画面停预览、最小窗口宽度 1100px 下顶栏不溢出），**没有在真实 Tauri 窗口里验证**，「窗口已关闭」分支只有单元测试，没有用真实的窗口开关走过。 |
| 整理布局 | `nodeBox` 的算术与分支顺序、不重叠、环与汇合、孤立节点由 `graph.test.ts` 覆盖；在浏览器 mock 里对真实 DOM 量过卡片高度与各输出口、输入口的位置，与 `nodeBox` 一致（开始 58、点击 126、结束 34；输入口 89.5 / 17.5），并看过一条直链整理后连线是直线。**没有在真实 Tauri 窗口里验证**：包括 ELK 在 WKWebView 与 Windows WebView2 下的加载、以及 CSP（`default-src 'self'`）下是否正常（构建产物里没有 `eval` / `new Function`，但没实测）。复杂分支图的观感只由「不重叠、顺序」两条断言兜底，没有人工逐图确认。 |
| Windows | **未验证。** `liverguard-driver-win` 的平台层（不含 OpenCV 的部分）通过了 `cargo check --target x86_64-pc-windows-msvc --no-default-features`；`Driver` 实现、`apps/desktop/src-tauri` 里 Windows 分支、Windows Graphics Capture 的实际截图、`SendInput`、WebView2 与 WKWebView 的画布一致性、Windows 安装包体积，都没有在真实 Windows 上运行过。Windows 输入使用物理像素坐标，也还没在多显示器、不同缩放下验证。 |
| 签名与公证 | 产物只有 ad-hoc 签名，Developer ID 签名和公证没有做（需要证书与 Apple 账号）。 |
| CI 打包与发布 | `.github/workflows/release.yml`（标签 `v*` 出草稿 Release，手动运行只出 Artifacts）、`scripts/ci/build-opencv.sh`、`scripts/ci/check-version.mjs`。已验证：OpenCV 脚本在本机 macOS 上从官方源码包编出最小静态 OpenCV 4.12.0（缓存命中分支也走过），并用它在不加载本机环境的情况下编译、测试了 `liverguard-vision`；版本检查脚本的通过 / 失败 / 非法标签三种情况；workflow 过了 `actionlint`。**没有验证**：workflow 没在 GitHub 上跑过（runner 镜像上 libclang 路径、`pnpm/action-setup`、`rust-cache`、`tauri build --bundles dmg` 与 `nsis`、`gh release create --notes-file --generate-notes` 的组合都是按文档写的）；**Windows 整条链路是第一次**（已在 GitHub 上跑过两次：OpenCV 的 MSVC 构建与 `find_package` 自检通过，随后 `opencv` crate 因静态库依赖的系统库 `kernel32` 找不到而失败，workflow 里现在用 cargo `--config` 把 Windows SDK 的库目录加进 `-L`，这个修复在第三次运行里生效；随后 `liverguard-driver-win` 的 `Driver` 实现首次被编译，报出两个错误：缺少 `MatTraitManual` 的导入、`Capturer` 持有的 D3D11 对象不是 `Send`（已加 `unsafe impl Send for Gpu`，理由写在代码注释里，本机用 `cargo check --target x86_64-pc-windows-msvc --no-default-features` 确认 `Capturer: Send`）；这两处修复没有在带 OpenCV 的 Windows 构建里跑过，之后 `src-tauri` 的 Windows 分支、链接与 NSIS 打包能否通过仍未知；`v0.2.0` 标签那次运行里，OpenCV 命中缓存后 `build-opencv.sh` 提前退出、没有执行 `rm -f /usr/bin/link.exe`，Rust 链接时被 Git 的 `link.exe` 遮蔽而失败（`/usr/bin/link: extra operand`），现在 workflow 在构建前单独删一次，同时把 `ilammy/msvc-dev-cmd@v1`（仍是 Node 20，触发弃用警告）换成 `TheMrMilchmann/setup-msvc-dev@v4.1.0`（Node 24），这两处改动都还没有在 GitHub 上跑过；再下一次运行里 Windows 的 Rust 编译与链接已通过（说明上面的修复生效），随后在 Tauri 下载 `nsis-3.13.zip` 时 GitHub 返回 HTTP 504 而失败，这属于下载偶发失败，workflow 现在把 `tauri build` 最多重试 3 次（重试时 Rust 部分已编完，只重跑打包），该重试同样没有在 GitHub 上跑过，NSIS 打包本身是否成功仍未知。另外，WinRT 截图所在的工作线程是否需要先初始化 COM（目前只有 UI Automation 里调用了 `CoInitializeEx`）没有验证）：MSVC 环境下 Ninja 编 OpenCV（`/MD`）、Git Bash 的 `link.exe` 遮蔽处理、Rust 在 Windows 上首次完整编译 `liverguard-driver-win` 与 Tauri 的 Windows 分支，预计会出编译错误需要修；CI 只能证明能编译和打包，证明不了截图和点击可用。macOS 只打 arm64（`macos-26`，需要 macOS 26 SDK），Windows 只打 x64；不支持 Intel Mac（GitHub 的 Intel 镜像只保证到 2027 年 8 月，决定不做）。两个构建都是必需的，任何一个失败都不会发布。发布前不跑 Rust 测试。 |
| 画面一致性 | 画布在 macOS 的 WKWebView 里渲染正常；与 Windows WebView2 的对比无法在本机完成。 |
| 移动端 | 不做。`Driver` 抽象为以后接 ADB / iOS 留了位置，但仓库里没有任何移动端代码。 |

## 当前边界

- 仅 macOS 和 Windows 桌面。
- 截图、输入和窗口焦点受操作系统权限、其他窗口遮挡及应用保护机制影响，无法保证所有应用均能自动化。
- 全屏模式下 LiverGuard 自己的窗口也会出现在画面里；运行脚本前应把目标窗口放到前面，或改用单窗口模式。
- 脚本不会跨机器同步：换电脑需要自行复制 `~/.liverguard/`，模板素材也必须一并带走。
- 列表里看不到无法解析的脚本（例如旧的 v1 文件）；它们不会被删除，也不会提示。
