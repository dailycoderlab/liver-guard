# Sky

Sky 是一个使用 OpenCV 的桌面脚本录制和执行工具。客户端负责屏幕预览、点击录制、模板选择、脚本编辑和本机执行；Cloudflare Workers 负责多用户身份验证、脚本版本、模板素材和运行记录同步，不再转发画面或远程控制设备。

## 目录

- `client/src/skycore/`：跨平台匹配、脚本解释器、检测和同步逻辑。
- `client/src/skygui/`：基于 PySide6 的桌面窗口。
- `client/src/skymac/`、`client/src/skywin/`：系统相关的窗口、截图和输入驱动。
- `workers/`：Cloudflare Workers、D1 和 R2 服务。

## 本地开发

需要 Python 3.11 或更高版本、[uv](https://docs.astral.sh/uv/) 及 Node.js 20 或更高版本。macOS 和 Windows 分别在本机安装客户端依赖。

启动本地云服务：

```sh
cd workers
npm ci
npx wrangler d1 migrations apply sky --local
npx wrangler dev --local --var JWT_SECRET:development-secret-at-least-32-characters-long
```

新开一个终端启动桌面窗口：

```sh
cd client
uv sync --group dev
uv run sky-gui
```

登录界面填写 `http://127.0.0.1:8787`、用户名和密码，首次使用选择注册。远程服务必须使用 HTTPS。点击“选择窗口”，从当前可见窗口列表中选择目标；打开新窗口后可点击“刷新列表”。列表不包含最小化窗口或 Sky 自身。选定目标窗口后，在脚本列表新建脚本，点击预览画面录制，并调整模板裁剪区域；保存草稿、发布后即可在本机运行。运行时可用“停止”按钮或系统托盘请求停止。关闭窗口时，如果系统托盘可用，程序继续在后台运行。

macOS 首次使用需在系统设置中允许该应用屏幕录制和辅助功能访问；未授予屏幕录制权限时，窗口标题可能显示为“无标题”。macOS 预览单独捕获选中窗口，即使被其他窗口遮挡也不会将遮挡窗口混入画面。Windows 窗口捕获目前使用屏幕区域截图：目标被遮挡时可能捕获到其他窗口；受保护内容、最小化窗口、权限级别不同的程序可能无法正常捕获或注入。开始运行前应确认目标窗口可见、截图正确。请勿在存在反作弊或禁止自动化条款的应用中使用。

## 测试

```sh
cd client && uv run pytest -q
cd ../workers && npm run check && npm test
```

客户端还需要在真实 macOS 和 Windows 桌面上手动验证屏幕捕获、窗口激活、输入权限及多显示器缩放。自动化测试使用模拟对象，不能替代真机验证。

## 桌面应用打包

分别在目标操作系统执行（不能在 macOS 上直接生成 Windows 程序）：

```sh
cd client
uv sync --group dev
uv run pyinstaller --noconfirm Sky.spec
```

产物位于 `client/dist/`。macOS 发布时需配置应用签名、公证和系统权限；Windows 对外分发建议使用代码签名。打包行为和体积需在实际目标机器验证。

## 服务部署与旧数据

在 `workers/wrangler.jsonc` 中填写自己的 D1 数据库与 R2 存储桶配置，通过 `wrangler secret put JWT_SECRET` 设置足够长的随机密钥，应用远程 D1 迁移后再部署：

```sh
cd workers
npx wrangler d1 migrations apply sky --remote
npm run deploy
```

迁移保留原有数据，但旧单用户数据仍归属于保留身份 `owner`，不会自动分配给新账号。迁移和上线前请备份 D1、R2，并先在独立环境验证升级、回滚与旧数据归属。新版本不再兼容原设备令牌、WebSocket 客户端和 Web 录制台。当前注册和登录尚需部署侧限流及生产安全审查，不建议未经防护地公开注册。
