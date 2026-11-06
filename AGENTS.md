# AGENTS.md

给 AI 编码助手（Cursor、Claude 等）和新协作者看的仓库约定。项目介绍见 [README.md](README.md)，架构与已知偏离见 [DESIGN.md](DESIGN.md)；这里只写动手前必须知道的事。

## 项目一句话

LiverGuard（中文名「护肝」）是纯本地的桌面自动化工具：用节点画布编辑脚本，OpenCV 截图匹配后在本机点击。技术栈是 Tauri 2 + React / `@xyflow/react` + Rust 核心。macOS 与 Windows 共用核心代码；移动端不做。

## 目录

| 路径 | 说明 |
| --- | --- |
| `crates/liverguard-ir` | 脚本 IR v2、校验、JSON Schema（前端类型的来源） |
| `crates/liverguard-vision` | 模板匹配、控件检测、模板裁剪（OpenCV） |
| `crates/liverguard-engine` | `Driver` trait 与图解释器，平台无关 |
| `crates/liverguard-store` | `~/.liverguard` 本地存储（`LIVERGUARD_HOME` 可覆盖） |
| `crates/liverguard-driver-mac` / `liverguard-driver-win` | 各平台截图、输入、窗口、权限 |
| `crates/liverguard-cli` | 命令行 `liverguard`，与桌面应用共用核心 |
| `apps/desktop/src-tauri` | Tauri 外壳，只做命令转发 |
| `apps/desktop/web` | 画布前端 |

分层规则：匹配、等待、分支、重试、报告只能写在 `liverguard-engine` / `liverguard-vision`；平台差异只能出现在 `liverguard-driver-*`、`src-tauri/src/platform.rs`、`liverguard-cli/src/platform.rs` 三处，不要在其他地方写 `cfg(target_os)`。

## 工作方式

- 只改当前讨论到的文件和范围，保持既有行为不变。范围外发现的问题（过时测试、遗留逻辑等）以说明或建议的形式提出，不要顺手改。
- 不要自行 `git commit` / `git push`，除非用户明确要求。
- 与用户交流和写文档用中文；代码标识符、路径、配置键、日志示例保持原样。代码注释语言跟随所在文件已有的风格（现有 Rust 注释是英文）。
- 改了行为、命令或目录，就同步更新 `README.md` / `DESIGN.md`，并如实记录没验证的部分，不要写成已验证。
- 优先用成熟的开源方案，不自造轮子；引入新依赖时说明版本和风险。
- 不要修改 `.cursor/plans/` 下的计划文件，除非用户要求。

## 环境与常用命令

编译 Rust 需要 OpenCV 4.x 和 libclang；本机的最小静态 OpenCV 在 `~/.local/opencv-4.12`，用前先加载环境（这台开发机上是 `source /tmp/ocvenv.sh`，如果文件不存在需要重建：`OpenCV_DIR`、`CMAKE_PREFIX_PATH`、`OPENCV_DISABLE_PROBES=pkg_config,vcpkg_cmake,vcpkg`）。

```sh
cargo test --workspace                                  # Rust 全部测试
cargo run -p liverguard-cli -- doctor                          # 真机检查权限与截图
cd apps/desktop/web && pnpm typecheck && pnpm test      # 前端类型与测试
cd apps/desktop/web && pnpm gen:types                    # IR 变更后重新生成 TS 类型
cd apps/desktop && ./web/node_modules/.bin/tauri dev     # 真实桌面应用（不是浏览器里的 1420）
cd apps/desktop && ./web/node_modules/.bin/tauri build --bundles app
```

注意：

- `http://127.0.0.1:1420/` 在浏览器里打开用的是 `src/api/mock.ts` 的模拟后端，不截图、不点击、不写磁盘；验证录制和执行必须用 Tauri 窗口。
- Tauri CLI 必须在 `apps/desktop` 下运行（那里的 `src-tauri/tauri.conf.json` 才能被找到），不要在 `web/` 下运行。
- 长时间运行的命令（`tauri dev`、OpenCV 编译）放后台，并把命令本身作为后台命令；在命令里再写 `&` 的子任务会在调用返回时被杀掉。
- 不要用 `pkill -f "tauri dev"` 这类宽匹配去停进程，可能杀到自己的 shell；按 `target/debug/liverguard-desktop`、`tauri.js dev` 或端口 1420 精确结束。

## 改 IR 时

`crates/liverguard-ir` 是唯一的事实来源：

1. 改 Rust 结构（`node.rs` / `target.rs` / `doc.rs`）。
2. 同步 `validate()`、`liverguard-engine` 的解释逻辑和对应测试。
3. 运行 `pnpm gen:types` 更新 `apps/desktop/web/src/ir/generated.ts`，再修前端编译错误；不要手改这个生成文件。
4. 输出口（handle）的命名是前后端的约定：改了就要同时改 `DESIGN.md` 里的列表和前端的节点定义。

没有历史兼容负担：旧 IR 不迁移，直接改结构即可，但要在 `README.md` 里写清不兼容。

## 修改匹配与图像代码时

- `crates/liverguard-vision/tests/golden/` 是对拍夹具，由已删除的 Python 实现（OpenCV 5.0.0）生成，当前是冻结的。不要为了让测试通过去改夹具；测试红了先查代码，确实要改算法行为时再单独说明并重建夹具。
- `opencv` crate 的 trait 方法需要 `use opencv::prelude::*`；`Mat` 是 `Send`，但 `liverguard_vision::Frame` 含缓存、不是 `Send`，跨线程传 `Mat` 和 `Bounds`，在目标线程再构造 `Frame`。
- 匹配前画面会归一化到 480 宽，高分辨率下得分偏低是已知特性，见 `DESIGN.md`；调这个常量会改变所有已存脚本的行为。

## 前端约定

- 状态在 `src/store/`（Zustand）。选择器如果返回新数组或新对象，必须用 `useShallow`，否则会无限重渲染。
- 图操作（加点、连线规则、整理）写成纯函数放在 `src/ir/`，并带 `vitest` 测试。
- 所有后端调用走 `src/api/` 的 `Backend` 接口：新增命令要同时改 `types.ts`、`tauri.ts`、`mock.ts` 和 `src-tauri/src/commands.rs` 里的命令与 `lib.rs` 里的注册。
- Tauri 命令里凡是截图、匹配、运行的都不能在主线程执行；同步命令会卡住窗口。
- 生产构建会禁用页面级右键菜单（`src/lib/lockdown.ts`），需要右键操作的地方用 `components/ContextMenu.tsx` 自己画菜单。
- 样式用 Tailwind 4，组件复用 `src/components/ui.tsx`，不要新写一套按钮和输入框。

## 真机验证与安全

- 运行真实脚本会**真的点击你的屏幕**。验证时优先用 `probe`（只定位不点击），要点击时用独立的测试窗口，并把点击目标限制在 LiverGuard 自己的窗口或一次性的测试应用里，不要对用户的浏览器、聊天或生产系统点击。
- 自动化测试和手工验证都用 `LIVERGUARD_HOME=/tmp/xxx` 指向临时数据目录，不要读写用户真实的 `~/.liverguard`。
- macOS 权限记在启动进程的应用上（终端或 IDE），授予屏幕录制后需要完全退出重启；测试里遇到“截图成功但画面是纯色”，先查权限。
- Windows 代码目前只做过交叉编译检查，没有在真实 Windows 上跑过；改这部分时不要声称已验证。
- 截图和实时预览会拍到屏幕上的任何内容，临时截图用完要删除，不要提交或上传。

## 提交前自检

- `cargo test --workspace` 和 `pnpm typecheck && pnpm test` 通过。
- 没有留下调试输出（`eprintln!("DEBUG ...")`、`console.log`）和临时文件。
- 文档与代码一致，未验证的内容在 `DESIGN.md` 的“偏离与验证状况”里有如实记录。
