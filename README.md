# Sky — 跨平台游戏挂机平台

云上控制面（Cloudflare Workers + Durable Objects + D1 + R2）加一个 macOS 设备客户端。
录制在平台端完成：浏览器实时看设备画面、点一下控件，客户端截下点击前的原图并回传搜索窗，
浏览器自动识别按钮轮廓、裁图、存成模板；运行时客户端用这张模板做实时图像识别定位。

设计文档在 [DESIGN.md](DESIGN.md)。

---

## 目录

```
workers/                 Cloudflare 控制面
  src/                   Hono 路由、RecordingHub DO、IR 与协议的 zod schema
  console/               录制台 SPA（Vite + React）
  migrations/            D1 表结构
  scripts/               smoke.py / ws_smoke.py / client_smoke.py / run_smoke.py
client/                  macOS 设备客户端
  src/skycore/           平台无关：几何、匹配、IR 解释器、上报（禁止 import pyobjc）
  src/skymac/            macOS 驱动：截屏、注入、窗口、权限、AX、主循环、CLI
  tests/                 单元测试 + 真实屏幕集成脚本
```

`skycore` 与 `skymac` 的边界是硬约束：Windows / Android / iOS 客户端只需要重写 `skycore.driver.Driver`
里的 6 个原语，匹配与解释器直接复用。

---

## 前置条件

- Node.js 20+（本项目用 26 开发）
- [uv](https://docs.astral.sh/uv/)（Python 3.11+，本项目用 3.12）
- macOS 上需要授予运行终端两项权限：**辅助功能**（注入点击）和 **屏幕录制**（截屏）。
  运行 `sky doctor` 会告诉你缺哪一项以及去哪里开。

---

## 本地跑起来

### 1. 控制面

```bash
cd workers
npm install
echo 'JWT_SECRET=dev-only-change-me-please-32-bytes-min' > .dev.vars
echo 'ADMIN_PASSWORD=sky-dev' >> .dev.vars

npm run build                 # 构建录制台到 console/dist
npx wrangler d1 migrations apply sky --local
npx wrangler dev --port 8787  # Worker + DO + D1 + R2 全部本地模拟
```

打开 <http://127.0.0.1:8787> 就是录制台，用 `.dev.vars` 里的口令登录。

### 2. 设备客户端

```bash
cd client
uv sync

# 权限自检：会打印权限、捕获面、截图耗时与内容、AX、窗口遮挡
./.venv/bin/sky doctor --window 阴阳师

# 注册设备（--window 指定要操控的窗口；不传就是整块屏幕）
SKY_PASSWORD=sky-dev ./.venv/bin/sky enroll \
  --server http://127.0.0.1:8787 --name mac-1 --window 阴阳师

# 上线，保持长连接
./.venv/bin/sky serve
```

客户端上线后录制台左侧会出现这台设备。在实时画面上点一下要操作的控件：

1. 客户端先截一张全分辨率原图，再执行注入（晚 50~80ms，人感觉不到，但拿到的是干净的无损原图）
2. 客户端裁出点击点周围 600x400 逻辑点的搜索窗，PNG 无损推给浏览器
3. 浏览器在搜索窗上跑轮廓检测，给出「系统控件 / 轮廓 / 色块 / 默认框」候选
4. 确认或拖框微调后，模板 PNG 上传 R2，脚本步骤立即追加，草稿每 2 秒自动保存
5. 点「发布版本」，客户端就能拉取并执行

### 3. 其他客户端命令

```bash
sky doctor   [--window X]   # 环境自检
sky windows  [--all]        # 列出可操控窗口
sky capture  --out a.png    # 截一张存盘
sky probe    --script <id>  # 只识别不点击，输出每步分数，缺失时返回非零
sky probe    --file ir.json # 离线跑一份 IR
sky run      --script <id>  # 本地执行一份脚本
```

---

## 测试

```bash
cd client  && .venv/bin/python -m pytest -q     # 40 项：几何/协议/匹配/解释器
cd workers && npm run check                     # Worker 与录制台的类型检查
cd workers && npm test                          # 7 项：按钮检测（真实像素合成场景）
cd client  && .venv/bin/ruff check src tests    # import 与未使用变量

# 需要 wrangler dev 在跑
python3           workers/scripts/smoke.py      # HTTP 面：鉴权、资产校验、发布、bundle
client/.venv/bin/python workers/scripts/ws_smoke.py      # DO 双端转发与持久化
client/.venv/bin/python workers/scripts/client_smoke.py  # 真实客户端：上线、推流、点击回路
client/.venv/bin/python workers/scripts/run_smoke.py     # 真实设备执行一条已发布脚本

# 真实屏幕上的匹配质量（分数、跨像素密度、落点误差）
client/.venv/bin/python client/tests/integration_probe.py --window 阴阳师
client/.venv/bin/python client/tests/bench_latency.py     # 捕获与匹配耗时
```

---

## 部署到 Cloudflare

```bash
cd workers
npx wrangler login
npx wrangler d1 create sky                 # 把返回的 database_id 填进 wrangler.jsonc
npx wrangler r2 bucket create sky-assets
npx wrangler secret put JWT_SECRET         # 32 字节以上随机串
npx wrangler secret put ADMIN_PASSWORD     # 录制台口令
npx wrangler d1 migrations apply sky --remote
npm run deploy
```

客户端注册时把 `--server` 换成 `https://<worker>.workers.dev` 即可。

免费额度上的实际用量（2026 年 10 月文档口径）：

- 入站 WebSocket 消息按 20:1 折算。8 fps 推流一小时 ≈ 1,440 个请求，一次 30 分钟录制会话 ≈ 540
- 出站消息免费，协议层 ping 不计费，所以设备挂着不推流时几乎零消耗
- 帧与搜索窗都不落 R2（搜索窗走 WS 直推浏览器），一次会话 R2 增量只有模板，通常 < 5 MB
- 分数按步聚合、日志整批写 R2：一次 30 分钟的运行只写几十行 D1，不会撞上每天 10 万行的写入上限

---

## 已知限制

- **权限**。macOS 不给自己授权，而且认的是父进程（Terminal / iTerm）。授权后要完全退出并重开终端。
- **捕获停滞**。权限失效或目标被全屏应用挡住时，macOS 不是报错，而是**阻塞约 30 秒后返回一张全黑图**。
  客户端会检测这种停滞（`capture_ms > 3000`），停止推流并按 `capture_stalled` 结束这次运行，`doctor`
  也会打印截图耗时与内容标准差。遇到过就重新授权屏幕录制。
- **窗口遮挡**。捕获的是屏幕上的矩形而不是窗口本身，覆盖在目标上的窗口会被拍进去。`doctor` 会列出
  盖住目标窗口的那些窗口并告诉你把游戏切到前台。
- **游戏屏蔽捕获**。部分游戏把窗口标记为不可共享，这种窗口任何屏幕捕获 API 都拿不到内容（实测
  阴阳师窗口就是这样：`screencapture -l <id>` 直接报 "could not create image from window"）。
  这类目标需要在客户端上换成 ScreenCaptureKit，或改用不遮挡的窗口/整屏布局。
- **注入**。CGEvent 在 Secure Input 下会被系统吞掉；带内核级反作弊的游戏会判定为合成输入。
- 匹配默认只跑 `tpl` + `mstpl`；`sift` 慢一个数量级，需要在目标上显式开启。

---

## 复用 Airtest 的边界

`client/src/skycore/matcher` 的做法参考了 Airtest 的 `aircv`（Apache-2.0）：灰度
`TM_CCOEFF_NORMED`、多候选涂黑抑制、彩色二阶校验、特征点回退、尺度搜索。代码是独立实现，因为
这个引擎的要求和 Airtest 的 `Template` 不一样：

- 按逻辑尺寸归一化（含 `recordWidth`），让一张裁图跨像素密度与跨屏幕尺寸都能命中
- 每个目标带搜索区（ROI），既提速也消歧
- 连续 `anchor` + 每步 `offset`，而不是九宫格预设落点
- 金字塔粗到细搜索、画面 hash 未变时复用上次结果、每步分数进入漂移监控
# sky
