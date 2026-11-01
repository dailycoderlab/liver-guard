# Sky

Sky 是一个使用 OpenCV 的纯本地桌面脚本录制和执行工具。录制、屏幕预览、模板选择、脚本编辑和运行全部在本机完成；脚本、模板素材和运行记录都保存在本机用户目录，不需要账号，也不联网。

## 目录

- `src/skycore/`：跨平台匹配、脚本解释器、控件检测和本地存储逻辑。
- `src/skygui/`：基于 PySide6 的桌面窗口。
- `src/skymac/`、`src/skywin/`：系统相关的窗口、截图和输入驱动。
- `tests/`：pytest 测试，使用模拟对象，无需真实屏幕。

## 本地开发

需要 Python 3.11 或更高版本及 [uv](https://docs.astral.sh/uv/)。macOS 和 Windows 分别在本机安装依赖：

```sh
uv sync --group dev
uv run sky-gui
```

点击“选择窗口”，从当前可见窗口列表中选择目标；打开新窗口后可点击“刷新列表”。列表不包含最小化窗口或 Sky 自身。选定目标窗口后，在脚本列表新建脚本，点击预览画面录制，并调整模板裁剪区域；保存草稿、发布后即可在本机运行。运行时可用“停止”按钮或系统托盘请求停止。关闭窗口时，如果系统托盘可用，程序继续在后台运行。

本地数据位于 `~/.sky/`：

- `scripts/`：脚本草稿与已发布版本，一个脚本一个 JSON 文件。
- `assets/`：模板素材，按 SHA-256 命名的无损 PNG。
- `runs/`：运行记录，包含汇总指标和 JSONL 日志。

macOS 首次使用需在系统设置中允许该应用屏幕录制和辅助功能访问；未授予屏幕录制权限时，窗口标题可能显示为“无标题”。macOS 预览单独捕获选中窗口，即使被其他窗口遮挡也不会将遮挡窗口混入画面。Windows 窗口捕获目前使用屏幕区域截图：目标被遮挡时可能捕获到其他窗口；受保护内容、最小化窗口、权限级别不同的程序可能无法正常捕获或注入。开始运行前应确认目标窗口可见、截图正确。请勿在存在反作弊或禁止自动化条款的应用中使用。

## 命令行

`sky` 提供无需 GUI 的本地命令：

```sh
uv run sky doctor                  # 检查权限、截图和坐标
uv run sky windows                 # 列出可控窗口
uv run sky capture --out shot.png  # 保存一张截图
uv run sky probe --script <id>     # 只定位模板，不点击
uv run sky run --script <id>       # 在本机运行已发布版本
```

`run` 和 `probe` 也可以用 `--file` 直接指定 IR JSON 文件。

## 测试

```sh
uv run pytest -q
```

客户端还需要在真实 macOS 和 Windows 桌面上手动验证屏幕捕获、窗口激活、输入权限及多显示器缩放。自动化测试使用模拟对象，不能替代真机验证。

## 桌面应用打包

分别在目标操作系统执行（不能在 macOS 上直接生成 Windows 程序）：

```sh
uv sync --group dev
uv run pyinstaller --noconfirm Sky.spec
```

产物位于 `dist/`。macOS 发布时需配置应用签名、公证和系统权限；Windows 对外分发建议使用代码签名。打包行为和体积需在实际目标机器验证。
