# 跨平台游戏挂机系统设计（Cloudflare Workers + 4 平台 Agent）

## 1. 目标与边界

**要做的**：一个"脚本管理 + 录制 + 下发 + 执行 + 回传"的闭环。手机/电脑上的 Agent 拉取脚本，在本地做图像识别与输入注入，循环执行；Web 面板远程控制、看日志、看截图。

**核心设计原则**（决定了后面所有选型）：

1. **云端只做控制面，不做像素计算。** Workers 免费版每次请求 10ms CPU，任何图像匹配/OCR 都必须留在客户端。服务端只处理 JSON、鉴权、转发、存储。
2. **脚本与执行分离。** 脚本是平台无关的中间表示（IR），每个平台只实现"能力适配器"。否则 4 个平台要写 4 套脚本。
3. **客户端自治。** 断网必须能继续跑（本地脚本缓存 + 本地状态机）。云端是遥控器，不是大脑。
4. **日志批量、截图抽样、实时只传元数据。** 免费额度是按请求数算的，任何"每帧上报"的设计都会瞬间打爆。

**明确不做的**：实时视频串流、服务端图像处理、对强反作弊竞技游戏的对抗、云手机集群管理（可作为后续扩展）。

---

## 2. 总体架构

```
                    ┌──────────────────────────────────────────┐
   Web 面板 (React)  │            Cloudflare (免费版)           │
        │           │                                          │
        │ HTTPS/WS  │  Worker (Hono)  ── REST 控制面            │
        └──────────►│    ├─ 鉴权 (JWT / Device Token)          │
                    │    ├─ /scripts /devices /runs /recordings│
                    │    ├─ R2 预签名直传 (截图/素材)            │
                    │    └─ 日志批量写入                         │
                    │                                          │
                    │  Durable Object  ── 实时面                │
                    │    ├─ DeviceHub (每用户一个)              │
                    │    │   设备 WS 长连接 / 指令下发 / 在线状态│
                    │    └─ RecordingRoom (每次录制一个)         │
                    │        录制事件流 / 面板实时预览           │
                    │                                          │
                    │  D1 元数据 │ R2 素材+截图+日志归档 │ KV 配置│
                    └──────────────────────────────────────────┘
                              ▲                    ▲
                   WS 指令通道 │                    │ HTTP 批量日志 + R2 直传
                              │                    │
        ┌─────────────────────┴────────────────────┴──────────────────┐
        │                        Agent (每台设备一个)                  │
        │  ┌──────────────────────────────────────────────────────┐  │
        │  │ 通用引擎: 调度器 / IR 解释器 / 状态机 / 重试 / 看门狗   │  │
        │  └──────────────────────────────────────────────────────┘  │
        │  ┌──────────┬──────────┬──────────┬──────────────────────┐ │
        │  │ Windows  │  macOS   │ Android  │ iOS (经 macOS 代理)   │ │
        │  │ SendInput│ CGEvent  │ 无障碍   │ iPhone Mirroring /   │ │
        │  │ DXGI 截屏 │ SCK 截屏 │ 截图 API │ WebDriverAgent       │ │
        │  └──────────┴──────────┴──────────┴──────────────────────┘ │
        └────────────────────────────────────────────────────────────┘
```

---

## 3. 后端设计（Cloudflare）

### 3.1 技术栈

| 层 | 选型 | 理由 |
|---|---|---|
| 运行时 | Workers (TypeScript) | 免费 10 万请求/天 |
| Web 框架 | **Hono** | 轻、路由/中间件齐全、Workers 一等公民 |
| 实时 | **Durable Objects + WebSocket Hibernation** | 唯一能做有状态长连接的方案；Hibernation 让空闲连接不烧 CPU |
| 元数据库 | **D1** (SQLite) | 免费 5GB，够存脚本/设备/运行记录 |
| 对象存储 | **R2** | 免费 10GB，**出流量免费**，存截图和模板图 |
| 缓存/配置 | KV | 存脚本清单、灰度开关（写少读多才用，KV 每天只有 1000 次写） |
| ORM | **Drizzle + drizzle-kit** | 有 D1 驱动，迁移可用 SQL 文件 |
| 校验 | Zod | 请求体 + IR schema 双向复用 |
| 鉴权 | `jose` (JWT) + 设备 token 哈希存 D1 | 无第三方依赖 |
| Dashboard | React + Vite + TanStack Query + shadcn/ui，部署在 **Pages** | 静态托管，不占 Workers 额度 |
| 本地开发 | `wrangler dev` + `--remote` | DO/D1/R2 都有本地模拟 |

### 3.2 为什么用 Durable Object 而不是纯 REST

- 设备需要"随时可被下发指令"，纯轮询要么延迟高要么请求多。
- 每用户一个 `DeviceHub` DO：持有该用户所有在线设备的 WS，Worker 收到 `POST /runs` 时转发给对应 DO，DO 推给设备。**一条指令 1 次请求**，而不是 N 个设备各轮询一次。
- 录制房间需要多端广播（手机推事件 → 面板实时看），天然是 DO 的活儿。
- 用 `state.acceptWebSocket()`（Hibernation API），空闲时不占 CPU、不计时长，只在你发消息时唤醒。
- **心跳用 WebSocket 协议层 ping/pong，不要用应用层 JSON 心跳**，否则每 30 秒就是一次计费请求。

### 3.3 数据模型（D1）

```sql
-- 用户与访问
users(id, email, pwd_hash, created_at)
api_tokens(id, user_id, name, token_hash, scopes, last_used_at, revoked_at)

-- 设备
devices(id, user_id, name, platform, os_version, screen_w, screen_h, dpr,
        caps_json, agent_ver, token_hash, last_seen_at, status)
device_groups(id, user_id, name)
device_group_members(group_id, device_id)

-- 脚本
scripts(id, user_id, name, game, tags_json, current_version, created_at)
script_versions(id, script_id, version, ir_json, ir_hash, note, created_by, created_at)
assets(id, user_id, script_id, kind, r2_key, w, h, sha256, created_at)

-- 下发与运行
assignments(id, script_id, target_type, target_id, schedule_cron,
            params_json, enabled, created_at)
runs(id, user_id, device_id, script_id, script_version, assignment_id,
     state, started_at, ended_at, step_count, error_code, error_msg,
     last_shot_key, stats_json)
run_logs_hot(id, run_id, ts, level, event, payload_json)   -- 只留错误和关键事件

-- 录制
recordings(id, user_id, device_id, name, state, started_at, ended_at,
           event_count, meta_json)
recording_frames(recording_id, seq, ts, r2_key, w, h)
```

> **关键约束**：D1 免费版每天约 10 万行写入。所以**不要一行一条日志**。普通日志写 R2（每次运行一个 JSONL 文件），D1 只存"运行摘要 + 错误"。一条 run 结束写 1 行，而不是写 5000 行。

### 3.4 R2 目录约定

```
u/{userId}/script/{scriptId}/{version}/ir.json
u/{userId}/script/{scriptId}/assets/{sha256}.png       # 模板图（内容寻址，天然去重）
u/{userId}/run/{runId}/log.jsonl                       # 全量日志
u/{userId}/run/{runId}/shot-{ts}.webp                  # 抽样截图
u/{userId}/rec/{recordingId}/frame-{seq}.webp
```

截图统一 WebP q=55~65、长边 ≤ 1280，一张 30~60KB。10 分钟跑一次、每 10 秒一张 ≈ 60 张 ≈ 3MB。

**模板图和有损截图的存储策略必须分开**：模板图（`assets/`）一律无损 PNG，因为压缩噪声会直接拉低匹配分数，跨设备场景下掉 3~8 分就可能从"能用"变成"不能用"；截图日志（`run/`、`rec/`）才用有损 WebP，反正是给人看的。

**直传**：Worker 生成 R2 预签名 PUT URL（S3 SigV4），设备直接上传，不经过 Worker 请求体。上传完成后再 `POST /runs/:id/shots` 登记 key。这样既不占 Worker CPU，也不受请求体大小限制。

### 3.5 API 草案

```
# 设备
POST   /v1/devices/register          {name, platform, screen, caps} → device_token (只返回一次)
POST   /v1/devices/:id/heartbeat     (兜底，正常走 WS)
GET    /v1/devices                   在线状态、电量、当前运行

# 脚本
GET    /v1/scripts                   列表
POST   /v1/scripts                   新建（空 IR 或从录制生成）
GET    /v1/scripts/:id/versions
PUT    /v1/scripts/:id/versions      发布新版本（服务端校验 IR schema + 资源完整性）
GET    /v1/scripts/:id/bundle?v=3    一次拉全：IR + 素材签名 URL（ETag 缓存）

# 下发
POST   /v1/runs                       {deviceIds[], scriptId, version, params}  → 经 DO 秒级下发
POST   /v1/runs/:id/stop
GET    /v1/runs?device=&from=&to=    分页查运行
POST   /v1/runs/:id/logs              批量日志（gzip JSON）→ 写 R2，摘要写 D1
POST   /v1/runs/:id/shots             登记截图 key

# 录制
POST   /v1/recordings                 → {id, wsUrl, uploadPrefix}
WS     /v1/recordings/:id/stream      设备推事件 / 面板订阅预览
POST   /v1/recordings/:id/compile     {segmentation 参数} → 生成 IR + 素材切图

# 实时通道
WS     /v1/device/hub                 设备指令通道（DO 转发）
WS     /v1/console/hub                面板订阅所有设备状态
```

### 3.6 免费额度预算（关键）

| 项 | 免费额度（以官网为准） | 本设计的用量 |
|---|---|---|
| Workers 请求 | 10 万/天 | 单设备 ~1.5k/天（日志 60s 一批 + 少量 API） |
| 单请求 CPU | 10ms | 只做 JSON，无图像处理，安全 |
| D1 写入 | ~10 万行/天 | 每 run 1~3 行，每设备每天 ~50 行 |
| D1 读取 | ~500 万行/天 | 面板查询，远低于上限 |
| R2 存储 | 10GB | 3MB/run × 50 run/天，需配生命周期清理 |
| R2 Class A/B | 百万级/月 | 预签名上传属 A 类，注意截图频率 |
| KV 写 | 1000/天 | 只写脚本清单缓存，别用来存日志 |
| Queues | 万级/天 | **不建议**用于日志，用 R2 直写更省 |

**结论**：**5~10 台设备完全在免费额度内**；20~50 台需要把日志批处理间隔拉到 120s、心跳改协议层、截图降到 30s 一张；超过 50 台建议上付费（$5/月）或改用自建 VPS。

---

## 4. 脚本模型（整个系统最关键的决策）

### 4.1 设计原则

- **声明式、可校验**：纯 JSON，不用 JS 代码。原因：安全（服务端可静态校验、可做可视化编辑）、跨语言（TS/Kotlin/Swift 三个解释器都能吃同一份）。
- **图像锚点是一等公民，坐标只是逃生舱**：脚本默认只描述"点哪个图"，运行时的真实坐标全部由识别结果算出来。坐标只允许出现在确实没有视觉特征的动作里，而且必须带区域约束、必须由录制器显式标出让人确认。
- **目标是一个描述符，不是一张图**：`裁图 + 搜索区 + 裁图内锚点 + 尺度策略 + 阈值 + 歧义策略`，缺一不可。只存一张截图的话，跨设备时匹配分数会掉到不可用——这是纯模板方案最常见的失败原因。
- **能力声明**：脚本开头声明所需能力（`screenshot` / `templateMatch` / `ocr` / `gesture` / `keyboard`），Agent 不支持就拒绝执行，而不是跑到一半崩。

**为什么坐标不能当主锚点**（不是理论，是实测会踩的坑）：

- 同一控件在 1170×2532@3x 的 iPhone 和 1080×2400@2.75x 的安卓上，物理像素尺寸差 10%~40%；
- 刘海、挖孔、手势条让同一元素的边距在不同机型上偏移 20~60px；
- 平板 4:3 的布局常常是重新排版的，不是整屏等比放大；
- 归一化坐标只是把误差从"像素"换成"百分比"，误差本身没消失。

所以坐标的合理用途只有一个：**把搜索范围限死在一块区域里（ROI）**，而不是当最终落点。

**但坐标也不能完全删掉**，否则会有一类步骤永远做不了：

- 点击空白处推进对话、点 3D 场景地面移动、点两个一模一样的道具格里的第二个；
- 这些动作天生没有可锚定的图像。IR 里保留 `type:"point"`，但强制带 `zone`（允许落点的区域），并且录制器要显式提示"这一步没有图像锚点"，而不是静默降级成坐标。

结论：**主模型是图像锚点，坐标退化成约束条件和逃生舱**。

### 4.2 目标描述符（Target）

脚本里每个可交互对象都是一个 Target 描述符，它才是"图像识别"这件事的完整定义：

```json
"btn_battle": {
  "type": "template",
  "asset": "sha256:9f2c41d8...",        // 录制时裁下的图，内容寻址，不可变
  "assetBox": { "w": 148, "h": 132 },   // 裁图在录制设备物理像素下的尺寸
  "recordScale": 3,                     // 录制设备的像素/逻辑点比
  "recordWidth": 390,                   // 录制设备捕获面的逻辑宽度
  "roi": [0.55, 0.72, 0.42, 0.20],      // 归一化搜索区，限制在大致会出现的区域
  "anchor": [0.32, 0.48],               // 点击点在裁图内的相对位置，录制时自动算出
  "strategy": ["tpl", "mstpl", "sift"], // 匹配策略回退链，前一个不中才试下一个
  "scale": { "mode": "auto", "min": 0.7, "max": 1.5, "steps": 9 },
  "threshold": 0.82,
  "warnThreshold": 0.70,
  "verify": "rgb",                      // 灰度匹配后，再做一次彩色二阶校验
  "mask": "none",
  "multi": "nearest-last",
  "fallback": { "type": "point", "norm": [0.76, 0.84], "zone": [0.60, 0.75, 0.30, 0.20] }
}
```

| 字段 | 作用 | 不填会怎样 |
|---|---|---|
| `asset` | 裁图的内容哈希，同一张图全局只存一份 | — |
| `assetBox` | 裁图原始尺寸，用来推算该用多大的尺度去搜 | 跨 DPR 设备必然找不到 |
| `recordScale` | 录制设备的像素/逻辑点比 | 跨 DPR 设备必然找不到 |
| `recordWidth` | **录制设备捕获面的逻辑宽度**。模板按录制宽度归一化、实时帧按当前宽度归一化，两边一致时控件就是 1.0 尺度命中，不需要任何尺度搜索 | 换一台屏幕更宽/更窄的设备就会整体缩放错位，只能靠尺度搜索硬凑 |
| `roi` | 归一化搜索区域，**性能命门 + 消歧手段**。与控件框取并集，落到屏幕边缘的控件也一定在自己的搜索区里 | 全屏搜索慢一个数量级，且容易命中同名的另一个按钮 |
| `anchor` | 点击点在裁图内的相对位置（0~1） | 只能点裁图几何中心，长条按钮会点偏 |
| `strategy` | 匹配策略回退链：`tpl` 灰度模板 → `mstpl` 多尺度 → `sift`/`brisk` 特征点 | 尺度或皮肤一变就整体失效 |
| `scale` | 尺度搜索范围，`auto` 时由 `assetBox` 和目标逻辑尺寸推算 | 差 15% 尺度就掉 30 分 |
| `threshold` | 判定阈值，一般 0.78~0.88 | 太低误匹配，太高漏匹配 |
| `warnThreshold` | 低置信区间下界 | 无法做"漂移预警" |
| `verify` | `rgb`：灰度匹配出候选后再做彩色二阶校验 | 灰度图里的假阳性无法排除 |
| `mask` | `alpha` / `edges`，圆角、半透明、纯色背景必须用 | 圆角外的背景变化会拉低分数 |
| `multi` | 多命中策略：`nearest-last` / `highest` / `center` / `nth` | 多命中时行为不确定 |
| `fallback` | 可选坐标兜底，仅在识别失败且允许降级时使用 | 某一步必挂 |

### 4.3 运行时匹配引擎（跨设备能不能跑通，全看这一节）

```
截图 → 逻辑尺寸归一化 → ROI 裁剪 → 1/4 分辨率多尺度粗搜
     → top-3 候选 → 全分辨率局部精搜 → 判定/消歧 → box[4] + 点击点
```

**七个关键点：**

1. **逻辑尺寸归一化（最关键）**。按 DPR 把截图换算成"逻辑点"再匹配，比如统一缩放成逻辑宽 400pt。这一步把 2x / 2.75x / 3x 设备的像素尺寸差基本抹平，尺度搜索范围从 ±60% 收到 ±15%，同一张裁图才能在 iPhone 和安卓上同时命中。**没有这一步，纯模板匹配跨设备基本不可用**——很多人试过"录一张图跨设备跑"失败，根因就在这里。
2. **ROI 先行**。只在录制时那块区域里搜。1080p 全屏 × 13 个尺度做归一化互相关，WASM 里要 1~3 秒；裁到 400×300 后降到几十毫秒。ROI 同时是消歧手段：弹窗里的"确定"和列表里的"确定"长得一样，区域能把它们分开。
3. **金字塔粗到细**。1/4 分辨率粗搜拿 top-3 候选，再回原图对应的小窗口做全分辨率精搜。整体 20~80ms，够 2~5 Hz 轮询。
4. **跟踪优先**。记住上次命中的 box，下一步先在这个 box 外扩 15% 的邻域里搜；屏幕 hash 没变时直接复用上次结果，连搜都不搜。挂机时大部分时间画面是静的，这一条能把平均耗时压到个位数毫秒。
5. **消歧与低置信管理**。多命中按 `multi` 选。分数落在 `[warnThreshold, threshold)` 记为 `low_confidence`：仍然执行，但必须上报。连续多次低置信的步骤会在面板标成"正在漂移"，提示重裁，而不是等某天突然跑飞。
6. **策略回退链**（借自 Airtest 的 `ST.CVSTRATEGY`）。灰度单尺度模板 → 多尺度模板 → 特征点匹配（SIFT/BRISK + 单应矩阵）。前一个没结果才试下一个，命中即停。这解决的是模板匹配的先天短板：**尺度变了、皮肤换了、有轻微旋转时，模板分数会掉到阈值以下，但特征点匹配照样能找到**。代价是特征点路径慢得多（几十到几百毫秒），所以只作为兜底，且同样只在 ROI 内跑。
7. **二阶彩色校验**（借自 Airtest 的 `rgb=True`）。灰度匹配容易在"形状相似、颜色不同"的地方产生假阳性——比如灰色按钮和亮起状态的同一按钮。做法是：灰度匹配出候选框后，把候选区域和模板都转成颜色直方图做相关性校验，不通过就丢弃。这一步能把误点率显著压下来，代价只有几毫秒。

**匹配失败的常见原因**（按出现频率排）：

| 现象 | 原因 | 对策 |
|---|---|---|
| 分数 0.4~0.6 | 尺度差太多 | 加逻辑尺寸归一化、放宽 `scale` |
| 全屏多处命中 | 裁图太泛（纯色块、纹理少） | 扩大裁图、加 ROI、换 `edges` mask |
| 完全找不到 | 动画图、按下态、换皮肤 | 多帧模板 any-of，或改用边缘匹配 |
| 安卓正常 iOS 找不到 | DPR 差异 | 逻辑尺寸归一化 |
| 时好时坏 | 半透明/渐变背景 | `edges` mask + 降到 0.75 |

对应到脚本层，还要能表达"这张图我识别不了"的几种替代：

| 场景 | 方案 |
|---|---|
| 动画 icon、旋转 loading | 多帧模板 `anyOf` |
| 数字/文本会变 | 不匹配图，改用 `ocr` 区域 + 正则 |
| 纯色按钮、无纹理 | 颜色直方图 + ROI 内质心 |
| 3D 场景里移动 | `point` + `zone` |
| 多个相同控件 | ROI 或 `multi:"nth"` + 序号 |
| 半透明遮罩 | `edges` mask + 低阈值 |

### 4.4 点击位置与偏移

- **`anchor` 记的是"点在控件的哪个相对位置"**，不是几何中心。1200px 宽的长条按钮点在左侧 30% 处，换到 728px 的平板仍然点左侧 30% 处。
- **`offset` 做每步语义微调**，单位是裁图尺寸的比例而不是像素，所以天然跨分辨率：
  ```json
  { "op": "tap", "target": "btn_battle", "offset": { "dx": 0.04, "dy": -0.12 } }
  ```
  表示在命中框内右移 4%、上移 12%。"点按钮上方一点避开图标""点格子右下角"都用它。
- **引擎级保底抖动**：默认 ±3px、±15ms，换算成归一化量再施加，避免不同分辨率下抖动幅度差 3 倍。随机化只在引擎里做，脚本不写死，这样手动调试和挂机运行的行为差异可控。
- **边缘安全**：最终落点自动夹到裁图内 10%~90%，防止 anchor 靠边导致点到控件外面。

### 4.5 IR 示例

```json
{
  "irVersion": 1,
  "meta": { "name": "每日刷本", "game": "xxx", "minEngine": "0.3.0" },
  "requires": ["screenshot", "templateMatch", "gesture"],
  "profile": { "recordedOn": "iphone14,3", "physW": 1170, "physH": 2532, "dpr": 3 },
  "vars": { "loopCount": 3, "staminaMin": 20 },
  "entry": "main",
  "blocks": {
    "main": [
      { "op": "waitFor", "target": "btn_battle", "timeout": 20000, "interval": 500 },
      { "op": "tap", "target": "btn_battle" },
      { "op": "loop", "count": { "var": "loopCount" }, "body": [
          { "op": "waitFor", "target": "btn_start", "timeout": 10000 },
          { "op": "tap", "target": "btn_start" },
          { "op": "waitFor", "any": ["result_win", "result_fail"], "timeout": 180000 },
          { "op": "if", "cond": { "visible": "result_fail" }, "then": [
              { "op": "log", "level": "warn", "msg": "战斗失败，重试" },
              { "op": "tap", "target": "btn_retry" }
          ], "else": [
              { "op": "tap", "target": "btn_confirm" }
          ]},
          { "op": "tap", "target": "empty_advance" },
          { "op": "waitStable", "ms": 800 }
      ]},
      { "op": "ocr", "region": "stamina", "save": "staminaText" },
      { "op": "if", "cond": { "match": { "var": "staminaText", "re": "(\\d+)/" }, "lt": 20 },
        "then": [ { "op": "call", "block": "usePotion" } ] }
    ],
    "usePotion": [
      { "op": "tap", "target": "btn_item" },
      { "op": "swipe", "from": { "target": "item_list" },
        "to": { "target": "item_list", "offset": { "dx": 0, "dy": -0.8 } },
        "duration": 300 }
    ]
  },
  "targets": {
    "btn_battle": {
      "type": "template",
      "asset": "sha256:9f2c41d8",
      "assetBox": { "w": 148, "h": 132 },
      "roi": [0.55, 0.72, 0.42, 0.20],
      "anchor": [0.32, 0.48],
      "scale": { "mode": "auto" },
      "threshold": 0.86
    },
    "result_win":  { "type": "template", "asset": "sha256:d4e5f6a0", "roi": [0.20, 0.28, 0.60, 0.24], "threshold": 0.84 },
    "result_fail": { "type": "template", "asset": "sha256:77aa10bc", "roi": [0.20, 0.28, 0.60, 0.24], "threshold": 0.84 },
    "btn_start":   { "type": "template", "asset": "sha256:1b2c3d4e", "roi": [0.60, 0.74, 0.36, 0.18], "anchor": [0.5, 0.5], "threshold": 0.86 },
    "btn_retry":   { "type": "template", "asset": "sha256:aa11bb22", "roi": [0.45, 0.55, 0.40, 0.16], "threshold": 0.86 },
    "btn_confirm": { "type": "template", "asset": "sha256:cc33dd44", "roi": [0.45, 0.55, 0.40, 0.16], "threshold": 0.86 },
    "btn_item":    { "type": "template", "asset": "sha256:ee55ff66", "roi": [0.02, 0.90, 0.26, 0.10], "threshold": 0.85 },
    "item_list":   { "type": "template", "asset": "sha256:1234abcd", "roi": [0.10, 0.20, 0.80, 0.60], "threshold": 0.80 },

    "empty_advance": { "type": "point", "norm": [0.50, 0.62], "zone": [0.30, 0.40, 0.40, 0.40] },

    "stamina": { "type": "ocr", "roi": [0.70, 0.06, 0.20, 0.04], "lang": "eng+chi_sim" }
  }
}
```

`empty_advance` 就是那个"没有视觉锚点"的例子：点屏幕中下部推进对话。它写成了 `point` + `zone`，面板会把这步标黄提醒，并且只在 `zone` 区域内落点——只要设备布局没把按钮挪进这个区，就不会误点。

### 4.6 动作集（跨平台最小共识）

| 类别 | 动作 |
|---|---|
| 输入 | `tap` `longPress` `swipe` `drag` `text` `key` `home` `back` |
| 等待 | `sleep` `waitFor`(任一目标出现) `waitGone` `waitStable`(画面稳定) |
| 控制 | `loop` `if/then/else` `call` `break` `retry`(最多 n 次，退避) `random` |
| 感知 | `templateMatch` `ocr` `colorAt` `hashScreen` `screenshot` |
| 观测 | `log` `emit`(自定义事件) `snapshot`(存截图) |

**刻意不做**的：任意代码执行、网络抓包改包、内存读写。这三个会把项目从"自动化"变成"外挂"，风险量级完全不同。

### 4.7 执行语义

- 每个动作返回 `{ok, reason, evidence}`；`reason` 是枚举（`timeout` / `not_found` / `ambiguous` / `low_confidence` / `permission_denied` / `screen_locked`），便于聚合分析。
- **每次成功识别都回传 `score` 和命中框**（不进单条日志，按步聚合上报）。这是整个系统最有价值的数据：某步分数从 0.95 慢慢掉到 0.78，说明游戏 UI 在改版或设备在漂移，可以在它彻底挂掉之前就收到告警。
- 引擎有**看门狗**：单次运行超时、连续 N 步画面 hash 不变（卡死）、连续 N 次识别失败 → 上报 `stagnation` 并执行恢复策略（返回主界面 → 重启游戏 → 停止）。
- 所有随机化（点击抖动 ±3px、等待 ±15ms、随机顺序）在引擎层统一做，脚本不用关心。

### 4.8 参考实现：Airtest 到底怎么做的

网易 Airtest 就是做"靠截图定位"的，而且这套机制在游戏自动化里已经跑了多年，证明了这条路可行。它的真实实现（`airtest/core/cv.py` + `airtest/aircv/*`）：

**1. 模板不是一张图，是图 + 元数据**

```python
Template(filename, threshold=None, target_pos=TargetPos.MID,
         record_pos=None, resolution=(), rgb=False,
         scale_max=800, scale_step=0.005)
```

- `record_pos`：录制时点击点相对屏幕中心的偏移，归一化成 -0.5~0.5
- `resolution`：录制时的屏幕分辨率
- `target_pos`：落点，但只有 **9 个预设位置**（四角 / 四边中点 / 中心），默认中点
- `scale_max` / `scale_step`：多尺度搜索的上限与步长

**2. 跨分辨率靠"把模板缩放到当前屏幕"，不是归一化坐标**

`_resize_image()` 在分辨率不一致时，用 `cocos_min_strategy` 把模板整体缩放一遍：

```python
scale = min(src_w / 960, src_h / 640) / min(sch_w / 960, sch_h / 640)
```

也就是假设游戏按 Cocos 的 MIN 策略做分辨率适配（设计分辨率 960×640）。这个假设对 Cocos 游戏成立，对 Unity/UE 只是近似，所以官方也建议"每台设备录一套"。

**3. 搜索区是预测出来的，但只在特征点分支生效**

`Predictor.get_predict_area()` 用 `record_pos` 预测当前屏幕上的大概位置，外扩 `DEVIATION = 100` 像素作为搜索区。注意它算 y 偏移时**也除以屏幕宽度**（源码注释："都按宽度缩放，针对 G18 的实验结论"），这是"UI 随宽度缩放"的隐含假设，横竖屏切换或平板会偏。

**4. 匹配策略是一条回退链**

```python
ST.CVSTRATEGY = ["mstpl", "tpl", "sift", "brisk"]
```

多尺度模板 → 单尺度模板 → SIFT → BRISK，命中即停。默认阈值 `ST.THRESHOLD = 0.7`。

**5. 灰度匹配 + 可选彩色二阶校验**

`TemplateMatching` 先转灰度、用 `TM_CCOEFF_NORMED` 算置信度；`rgb=True` 时再用 `cal_rgb_confidence` 对候选区域做颜色相关性校验，压掉假阳性。找多命中时把已命中区域涂黑再找下一个，最多 10 个。

**6. 轮询在应用层**

```python
loop_find(query, timeout=20, interval=0.5, intervalfunc=None)
```

每次循环重新截屏 + 匹配。`exists()` 用 3 秒超时并且**返回坐标**，可以直接 `touch(pos)` 复用这次匹配结果，避免同一目标匹配两次。

**它做对、我们已经吸收的**

| 机制 | Airtest | 本设计 |
|---|---|---|
| 图 + 元数据 | `Template` 5 个参数 | Target 描述符（多 roi/scale/mask/multi/verify） |
| 策略回退链 | `CVSTRATEGY` | `strategy: ["tpl","mstpl","sift"]` |
| 二阶彩色校验 | `rgb=True` | `verify: "rgb"` |
| 搜索区 | `Predictor`，固定外扩 100px | `roi` 归一化 + 面板可调 |
| 轮询 | `loop_find(timeout, interval)` | `waitFor(timeout, interval)` |
| 结果复用 | `exists()` 返回坐标 | `waitFor` 返回 hit，`tap` 直接消费 |
| 多尺度 | `mstpl` 全图搜 | 金字塔粗搜 + 局部精搜 |

**它做不到、我们必须补的**

1. **点击点退化成框中心**。`record_pos` 只用来预测搜索区，最终落点是 `TargetPos` 的 9 个预设位置，默认中点。所以"点长条按钮的左端"表达不了——这正是你说的"方便做偏移"要解决的问题。我们的 `anchor` 是连续值 0~1。
2. **`tpl` / `mstpl` 分支是全屏搜索**，`Predictor` 的搜索区只用于 `sift`/`brisk` 分支。1080p 全屏多尺度匹配在移动端很慢。我们先裁 ROI。
3. **阈值全局**（0.7），不能按控件调，也无法做"接近阈值"的预警。我们 per-target `threshold` + `warnThreshold` + 分数漂移监控。
4. **没有 DPR 概念**。它用像素分辨率做缩放，2x/3x 设备之间全靠 `cocos_min_strategy` 猜。我们用逻辑点归一化，把 DPR 显式消掉。
5. **无跟踪、无画面 hash 复用**。每步都重新全屏匹配。我们记住上次命中框，画面不变就直接复用。
6. **没有分数可观测量**。跑挂了只能看截图猜。我们把每步 score 聚合成指标。

顺带一个佐证：Airtest 的设备层选型和我们 §6 的表格一致——**Android 走 ADB + minitouch/minicap，iOS 走 WebDriverAgent，Windows 走 pywinauto**，Poco 作为 UI 树增强。它没有做 macOS 原生，也没有解决"免越狱 iPhone 独立运行"，说明这两块确实是平台硬限制而不是方案没想全。

结论：Airtest 证明了"截图定位"这条路能走通，它踩出来的坑（回退链、彩色校验、搜索区预测、结果复用）都值得直接抄。但它停在"单设备可用"，我们要的是"一处录制、四端运行"，差的那一层就是 **逻辑尺寸归一化 + 每目标 ROI + 连续 anchor + 分数监控**。

还有一个架构层面的差别要说清楚：**Airtest 是"PC 驱动设备"的工具链，不是一个能塞进手机里的运行时**。它是 Python + ADB + minitouch/minicap，脚本在 PC 上跑，指令通过 ADB 发给手机。所以它能直接服务的是我们的桌面 Agent；Android/iOS Agent 只能**移植它的算法**，不能直接依赖它。这也解释了为什么它没有"设备端自治、断网继续跑"的能力——它的设计目标本来就是有 PC 在旁边的测试场景。

---

## 5. 录制管线

目标：在手机上手动玩一遍 → 自动裁下每个要点的控件图 → 生成能跨设备运行的脚本。**这一步的质量决定整个系统的成败**，因为脚本的鲁棒性在录制时就被裁图框决定死了，运行时再调只能补救。

```
设备端采集                服务端处理                      人工确认
─────────────           ─────────────────            ─────────────────
触摸事件(含时间戳) ─┐
关键帧截图(变化时)  ├→ 动作切分 → 分段列表            ↓
屏幕元数据(宽高/DPR)┘   (距离/时长阈值)          面板可视化编辑
                            ↓                     · 拖动调整裁图框
                    每个 tap 生成一个裁图         · 调整 ROI / 阈值
                    (按下面 5 条规则)             · 把 point 升级为模板
                            ↓                     · 换 OCR / 合并步骤
                    裁图去重 + 内容寻址存 R2       ↓
                            ↓                    发布版本
                    ROI 推断 → 生成 IR
                            ↓
                    回放自检(dry-run, 只识别不点击)
                            ↓
                    跨设备探针(只识别，报告哪些步骤别的机器找不到)
```

**关键实现点**：

1. **动作切分**：`down → up` 距离 < 8px 且时长 < 300ms → tap；距离 ≥ 8px → swipe（贝塞尔采样 8~12 个点）；两次操作间隔 > 800ms → 插入 `sleep`；间隔 > 5s 且画面变化 → 插入 `waitStable`。
2. **裁图（核心，不只是"截个图"）**：
   - **取帧时机**：用 `down` 之前 200~400ms 的那一帧，不要用 `up` 之后的。按下态、弹窗、按钮高亮都会让控件外观改变，用错帧会导致匹配分数天生低 10~20 分。
   - **初始框**：以点击点为中心，边长 `max(64px, 2.2 × 点击目标估计尺寸)`。
   - **自动膨胀到控件边界**：在初始框周围做边缘能量扫描，把框扩到控件轮廓（类似"智能选区"），避免裁图画到一半按钮、一半背景。
   - **上限 220px**：裁图越大，包含的"会变化的内容"越多（数字、头像、特效），分数反而掉。宁小勿大，但要保证唯一性——用 `roi` 去补唯一性，不要用大裁图去补。
   - **无损存储**：模板必须用 PNG 存。用有损 WebP 会让匹配分数掉 3~8 分，在跨设备场景里这几分会直接把可用变成不可用。截图日志才可以有损。
   - **剔除无效步**：裁图区域是纯色（方差极低）时说明该处没有视觉特征，标记成 `point` 类型并在面板标黄，交给人工决策，不要静默生成一个必然误匹配的模板。
3. **`anchor` 记录**：直接记录 `(点击点 - 裁图左上角) / 裁图尺寸`，得到 0~1 的相对坐标。这就是"点在控件哪个位置"的精确表达。
4. **去重与内容寻址**：裁图 sha256 后存 R2，同一张图全局只存一份（`btn_confirm` 和 `btn_ok` 常常是同一张图）。脚本版本引用哈希，图不可变，天然可回滚。
5. **ROI 推断**：把屏幕按 3×5 网格划分，取裁图中心所在格 + 一圈邻域作为初始 ROI，再收缩到 `[0.05, 0.95]` 范围内。面板上可以拖动调整。ROI 给太大会明显变慢，给太小会在布局变化时找不到，默认值取"够覆盖弹窗出现位置变化"的程度。
6. **录制来源建议**：**在 DPR 最高的设备上录**。模板从高分辨率缩下来是安全的，反过来放大必然糊。逻辑尺寸归一化会处理其余的差异。
7. **回放自检（dry-run）**：编译完成后，立刻在录制设备上跑一次"只识别不点击"的回放，逐步骤输出 `best score` 和命中框。分数低于阈值的步骤在面板高亮，让人现场调 ROI、重裁或换 OCR，而不是等到线上挂机时才失败。这一步能把脚本可用率从"凭感觉"提到"有指标"。
8. **跨设备探针**：换一台设备后，先跑一次只识别的探针，输出"哪些步骤在这台机器上找不到"。逐设备记录到 `devices.caps_json`，后续可以据此做设备级的尺度补偿，甚至直接把某些设备排除在该脚本之外。
9. **实时预览**：录制时手机通过 WS 只推事件元数据（**不推视频**），面板收到后在"最近一张关键帧"上画出点击热区和裁图框。延迟 300ms 左右。关键帧只在 `hashScreen` 变化超过阈值时才上传，一分钟游戏操作大约 20~40 张。
10. **人工增益**：自动生成的脚本一定能跑，但一定有冗余。面板要提供四个操作：`拖动/重裁裁图框`、`调整 ROI 与阈值`、`把 point 步骤升级为模板`、`合并连续点击 + 插入条件判断`。把"事件回放"升级成"健壮的状态机"这一步，人工增益的价值远大于自动算法。

---

## 6. 客户端 Agent

### 6.1 通用结构（4 个平台共用一套骨架）

```
┌─────────────────────────────────────────────┐
│ Supervisor 进程/服务                          │
│  ├─ Config & Auth (device token, 本地加密存储)│
│  ├─ Sync       (拉 bundle，ETag 缓存，离线可用)│
│  ├─ Scheduler  (cron 表达式 + 手动触发)       │
│  ├─ Engine     (IR 解释器 + 状态机 + 看门狗)   │
│  ├─ Reporter   (日志缓冲、批量上报、失败重传)  │
│  └─ Transport  (WS 指令通道 + HTTP 上报)      │
└───────────────┬─────────────────────────────┘
                │  Platform Adapter (每个平台实现这 6 个原语)
                ▼
      capture() / tap(x,y) / swipe() / key() / text() / capabilities()
```

**适配器只需要 6 个原语**，所有复杂逻辑（识别、等待、分支、重试）都在共享的 Engine 里。这是"4 个平台"能维护得动的前提。

### 6.2 平台方案对比

| 平台 | 输入注入 | 屏幕采集 | 可行性 | 主要坑 |
|---|---|---|---|---|
| **Windows** | `SendInput` / `pydirectinput` / `enigo`(Rust) | DXGI Desktop Duplication (`dxcam`)、Windows.Graphics.Capture | ★★★★★ | 内核级反作弊（Vanguard/EAC）会标记合成输入；GPU 渲染窗口用 `PrintWindow` 会黑屏 |
| **macOS** | `CGEvent.post` | ScreenCaptureKit | ★★★★★ | 需要用户手动授予"辅助功能 + 屏幕录制"权限；Retina 下点/像素要换算 |
| **Android** | `AccessibilityService.dispatchGesture` | `takeScreenshot`(API 30+) / MediaProjection | ★★★★☆ | 部分游戏开 `FLAG_SECURE` 截图全黑；有游戏检测无障碍服务；`MediaProjection` 每次授权 |
| **iOS** | **系统不开放**，见 6.5 | 同左 | ★★☆☆☆ | 独立 App 无法操控其他 App，必须借外部宿主 |

### 6.3 Windows / macOS 桌面 Agent —— 先用 TypeScript 打通

**推荐第一版**：Node + TypeScript。
- 输入：`@nut-tree-fork/nut-js`（macOS CGEvent / Windows SendInput 都封装好了），或 `robotjs`（老但简单）。
- 截图：`screenshot-desktop`（简单）/ `dxcam` 的 Node 绑定（Windows 高帧率）/ macOS 用 `screencapture` CLI 起步。
- 图像匹配：`opencv4nodejs` 或 `@techstark/opencv-js`（WASM，免编译，慢一点但够用：1080p 单次模板匹配 20~60ms）。
- OCR：`tesseract.js`（首次加载慢，可预热）或调用系统 OCR（macOS Vision / Windows.Media.Ocr，精度更好）。
- 优势：**解释器和 Workers 后端都是 TypeScript，IR 的 schema 和校验代码 100% 复用**，一天能跑通闭环。

**长期**：如果要做成单文件分发给别人用，迁到 **Tauri + Rust**（`enigo` + `xcap` + `opencv` crate + `reqwest`），体积 10MB 级、内存低、可复用 UniFFI 给 Android/iOS。建议先不改，等闭环验证完再迁。

### 6.4 Android Agent

- **Kotlin**，前台服务（`foregroundServiceType=mediaProjection|specialUse`）+ `WakeLock` + `keepScreenOn`。
- 输入：`AccessibilityService` + `GestureDescription`（API 24+，无需 root，事件是标准触摸事件，绝大多数游戏接受）。
- 截图：API 30+ 用 `AccessibilityService.takeScreenshot()`（**无授权弹窗**，这是首选）；低版本回退 `MediaProjection`。
- 识别：OpenCV Android SDK（`matchTemplate` + mask，毫秒级）或 TFLite 做 UI 元素检测。
- 加速/增强（可选）：Shizuku 或 root 下用 `input`/`sendevent`，以及 `FLAG_SECURE` 游戏的兼容。
- **Engine 复用方式**：两种，按你的团队偏好选。
  - (a) 用 `quickjs-android` 内嵌 JS 引擎跑同一份 TypeScript 编译产物（一套代码，但调试链路长）；
  - (b) 用 Kotlin 重写解释器（约 1500 行，性能最好，但要维护两份）。
  - 我倾向 (b)：解释器很小，重写成本低于维护跨语言运行时。
- **WM 检测提示**：部分游戏会枚举无障碍服务。App 名/包名做中性化处理，或者用 root 注入替代无障碍。这是策略问题，不是技术问题。
- **规模化**：一台 PC 跑 4~8 个 MuMu/LDPlayer 实例，每个实例里装一个 Agent，对后端来说就是 4~8 台"设备"。这是成本最低的多开方案。

### 6.5 iOS —— 唯一有硬限制的平台，必须正视

**事实**：iOS 沙盒不允许 A 应用模拟触摸 B 应用。所有"iPhone 上挂机"的方案本质都是绕过这一点。三条可行路径：

**(a) Mac 作为宿主，通过「iPhone 镜像」驱动（推荐，不需要越狱）**
- macOS 15+ 的 iPhone Mirroring 把手机画面投到 Mac 窗口，鼠标点击会转成手机触摸。
- Mac 侧 Agent 用 ScreenCaptureKit 抓这个窗口 → 模板匹配 → `CGEvent` 点击窗口对应位置。
- 优点：零越狱、系统级稳定、可直接复用 macOS Agent 的全部代码（只多一个"窗口定位 + 坐标映射"驱动）。
- 缺点：必须有一台常开的 Mac；手机要锁屏待机；部分 App 可拒绝镜像；镜像有 100ms 级延迟。
- 风险：iOS 26+ 可能收紧镜像的输入通路，需要关注。

**(b) WebDriverAgent / Appium（不需要 Mac 常开？仍需要一台 Mac 编译和连接）**
- Xcode 编译 WDA 到真机，Appium 通过 `XCUITest` 驱动：`mobile: tap` 按坐标点击、`/screenshot` 取图。
- 优点：官方测试框架，输入是系统级合成事件；可同时连多台 iPhone。
- 缺点：免费开发者账号 7 天要重签；Unity/UE 游戏的可访问性树是空的，只能纯坐标 + 图像匹配；启动慢（秒级）。
- 适合：稳定性优先、不介意维护成本的场景。

**(c) 越狱 + Frida / 触摸 tweak**
- 能力最强（可直接 hook 游戏内函数、读取内存），但设备/系统版本受限、维护成本极高、封号风险最高。只在你有明确的越狱设备时考虑。

**不建议**：用 Shortcuts 做"定时打开 App"——只能做最浅层的自动化，做不了识别和闭环。

### 6.6 Engine 主循环（伪代码）

```ts
async function runScript(ir: IR, ctx: RunCtx) {
  const env = createEnv(ir.vars);
  const deadline = Date.now() + ir.meta.maxDurationMs;
  let lastHash = "";
  let stagnant = 0;

  for (const step of ir.blocks[ir.entry]) {
    if (Date.now() > deadline) throw new RunError("timeout");

    const r = await exec(step, env, ctx);          // 单步执行，内部自带 retry
    if (!r.ok) {
      ctx.snapshot("failure");                      // 失败必存图，否则无法排查
      throw new RunError(r.reason);
    }

    const h = await ctx.hashScreen();               // 降采样后算 hash，很便宜
    stagnant = h === lastHash ? stagnant + 1 : 0;
    lastHash = h;
    if (stagnant > 10) throw new RunError("stagnation");
  }
}
```

外层再包一层：`try/catch → 上报 → 恢复动作 → 指数退避重试`，最多 3 次，然后置设备为 `needs_attention` 并在面板亮红灯。

**单步里的"目标 → 落点"解析**（脚本里没有坐标，坐标在这里被算出来）：

```ts
async function resolveTarget(t: Target, ctx: RunCtx): Promise<Hit | null> {
  const prev = ctx.lastHit(t.id);                       // 上次命中的框，用于跟踪
  const full = await ctx.screenshot();                  // 采一帧
  const frame = logicalNormalize(full, ctx.dpr);         // ① 逻辑尺寸归一化
  const roi = crop(frame, t.roi);                        // ② 只在 ROI 里搜
  const coarse = matchPyramid(roi, t.asset, t.scale, {  // ③ 1/4 粗搜 → top-3
    hint: prev ? inflate(prev, 0.15) : undefined        // ④ 跟踪优先
  });
  const best = await refine(frame, coarse, t);           // 全分辨率精搜
  if (!best || best.score < t.warnThreshold) return null;
  ctx.reportScore(t.id, best.score);                     // ⑤ 回传分数，用于漂移检测
  return {
    box: best.box,
    point: applyAnchor(best.box, t.anchor, t.offset ?? { dx: 0, dy: 0 }, ctx.jitter)
  };
}
```

这 30 行是整个跨平台能力的技术核心：**脚本里只有图，落点在运行时算出来**。

---

## 7. 通信协议

**设备 → 云端**

| 类型 | 通道 | 频率 |
|---|---|---|
| 在线保活 | WS 协议层 ping | 25s（不计费） |
| 状态变更 | WS JSON | 事件驱动（开始/结束/错误） |
| 日志 | HTTP 批量 gzip | 30~120s 一批 |
| 截图 | R2 预签名直传 | 抽样，10~30s |
| 心跳/电量 | 随日志批带上 | — |

**云端 → 设备**：全部走 WS（`run` / `stop` / `pause` / `config` / `ack`）。DO 保存最近 200 条指令，设备重连后按 `lastSeq` 补发，避免"下发时设备恰好断线"。

**为什么日志不走 WS**：一条 WS 消息算一次请求，而一个 HTTP 批量请求能装 500 条日志。差 500 倍。

---

## 8. 落地路线

| 阶段 | 内容 | 产出 | 预估 |
|---|---|---|---|
| **M0 骨架** | Wrangler 工程、Hono 路由、D1 schema、设备注册/鉴权、脚本 CRUD、面板登录 | 能注册设备、存脚本 | 2~3 天 |
| **M1 闭环** | macOS/Windows Agent（Node）+ IR 解释器 + 手写一个脚本 + 批量日志 + 远程启停 | **能真的挂机了** | 1~2 周 |
| **M2 录制** | 录制 WS 房间 + 关键帧上传 + 动作切分 + 自动锚点 + 面板回放/编辑 | 手机玩一遍就生成脚本 | 2 周 |
| **M3 多端** | Android Agent（Kotlin 解释器 + 无障碍）；iOS 走 Mac 镜像驱动 | 4 平台跑同一脚本 | 3~4 周 |
| **M4 打磨** | 定时调度、多开、异常恢复、截图时间线、告警（Telegram/Bark）、额度监控 | 7×24 无人值守 | 持续 |

**建议顺序**：M0 → M1 先把"一台 Mac 跑通一个脚本"做扎实。**不要一开始就做 4 个平台**，4 个平台的适配层会消耗掉 80% 的时间，而 90% 的架构问题在第一个平台上就会暴露。

---

## 9. 能做到什么效果 / 做不到什么

**能做到**
- 单人 24/7 挂机：日常任务、刷副本、采集、放置类升级、自动战斗、断线重连、卡死自愈。
- **录一遍，跨设备复用**：脚本里存的是控件裁图而不是坐标，运行时靠图像识别算落点。DPR 归一化 + ROI + 多尺度搜索 + 首次跨设备探针，同一份脚本能在 iPhone、安卓、macOS、Windows 上跑。
- **识别精度可观测**：每步的匹配分数回传，面板能看到"哪一步正在变差"，在脚本彻底挂掉之前先收到告警。
- 远程遥控：面板点一下开跑，实时看当前步骤、最新截图、失败原因（延迟 1~3 秒）。
- 5~10 台设备同时挂，全部在 Cloudflare 免费额度内。
- 完整可追溯：每次运行的日志、截图、步骤耗时、失败分布。

**做不到 / 代价很高**
- 需要 < 50ms 反应的动作/竞技游戏：云端指令 + 识别延迟不够，且强反作弊会直接封号。
- 服务端实时看视频：免费额度撑不住，要用 WebRTC + 云主机。
- 免越狱的"iPhone 独立 App 自动点击别的 App"：系统层面不开放，只能借 Mac/Appium。
- **没有视觉特征的步骤必须退化成坐标**：点空白推进对话、点 3D 场景地面移动、点两个完全相同的道具格里的第二个。IR 支持 `point` + `zone`，但这类步骤跨设备时需要重新校准。
- 完全无人维护：游戏一次 UI 改版（换皮肤、调布局、换字体），模板就要重裁。区别只在于——有了分数监控，你会**先收到告警**，而不是第二天发现挂在登录界面转圈。

---

## 10. 风险（必须提前想清楚）

1. **封号**：合成输入、无障碍服务、录屏都会被风控捕获。内核级反作弊（Vanguard/BattlEye/EAC）基本一抓一个准。用于自有账号的放置类/单机游戏风险可控，用于竞技或多人对抗则明确不建议。
2. **ToS**：多数游戏的用户协议禁止第三方自动化。技术上可行不等于允许。
3. **iOS 政策**：iPhone Mirroring 的输入通路、WDA 的签名周期都可能变，架构上要把"iOS 驱动"做成可替换插件（`IPhoneDriver` 接口），而不是写死。
4. **免费额度边界**：超过 ~20 台设备就会开始挤额度，提前在面板做一个"本月请求数"仪表，接 Cloudflare Analytics API。
5. **隐私**：截图里会有聊天/支付信息。上传前在设备端做区域打码（只保留 ROI 附近），并对 R2 设置 7 天生命周期。

---

## 11. 实现后修正的设计

下面几条是写代码和跑真实屏幕过程中发现的，前面章节已经按结论改过，这里记下出处。

### 11.1 Target 必须带 `recordWidth`

原设计只有 `assetBox`（裁图像素尺寸）和 `recordScale`（像素/逻辑点）。这两个值只能算出裁图的
**绝对逻辑尺寸**，而绝对尺寸在不同宽度的屏幕上没有可比性：游戏在 600pt 宽的窗口里按钮占 20%，
在 900pt 宽的窗口里还占 20%，绝对尺寸却从 120pt 变成了 180pt。缺 `recordWidth` 时，模板会按
"120pt 放在 900pt 的面上"归一化，等于 13.3%，而实时帧里的按钮是 20%，平白多出 1.5 倍的尺度差，
只能靠尺度搜索硬凑，超过搜索范围就直接找不到。

补上 `recordWidth` 后，模板和实时帧各自按自己的捕获面宽度归一化，相对尺寸一致的控件就是 1.0
命中。这个是单元测试 `test_same_layout_on_a_larger_window` 暴露出来的：修之前那台"更大的窗口"
必须搜到 1.5 倍才命中，修之后直接命中。

顺带把尺度搜索范围从 `0.8~1.25` 放到 `0.7~1.5`，覆盖"控件尺寸不变但屏幕更大"这种固定尺寸 UI。

### 11.2 ROI 必须把控件本身框进去

原来的 ROI 是 3x5 网格取格再收紧到 `[0.05, 0.95]`。这会让屏幕最底部或最边缘的控件落进自己搜索区
之外，等于永远找不到。现在 ROI 与控件框取并集再夹到 `[0, 1]`。

### 11.3 空模板会让归一化互相关给出满分

`TM_CCOEFF_NORMED` 对纯色模板的相关系数在数学上是未定义的，OpenCV 会返回 1.0 或 NaN。不做保护
的话，一块纯色裁图会在画面左上角报出 1.0 的"完美命中"。现在模板标准差低于阈值就直接判定该目标
不可定位，录制台也会把这种区域标成 `auto=false` 让人手动框选。

### 11.4 金字塔粗到细的坐标要除不要乘

粗搜是在 1/2 分辨率上做的，粗搜坐标回到全分辨率要 **除以** 缩放系数。写反了的表现是：分数在
粗搜阶段高得离谱，精搜阶段掉到 0.06，最终报"找不到"。这类静默错误只能靠真实数据测出来。

### 11.5 macOS 捕获失败是"卡 30 秒 + 全黑"，不是报错

权限失效、或屏幕被全屏应用接管时，`CGDisplayCreateImage` 会阻塞约 30 秒然后返回一张全黑图。
这对客户端的解读是致命的：一次运行会在无意义的轮询上把时间预算烧光，日志里只有"找不到目标"。

现在客户端记录每次捕获的耗时（`Frame.capture_ms`），超过 3 秒就判为停滞：推流自动停止并说明原因，
运行以 `capture_stalled` 立即失败，`doctor` 也会打印截图耗时与内容标准差。实测环境里就是
`耗时 31039 ms / 内容标准差 0.00`。

### 11.6 遮挡检测要提前做

捕获的是屏幕矩形而不是窗口，被别的窗口盖住时拍到的就是那些窗口。`doctor` 现在按 z 序找出盖住
目标窗口的窗口并列出名字，直接告诉操作者"把游戏切到前台"，比让他猜为什么模板永远不匹配好得多。

### 11.7 有些游戏窗口就是拍不到

实测阴阳师窗口：任何窗口级捕获都被系统拒绝（`screencapture -l <id>` 直接报
"could not create image from window"），整屏捕获在它前台时也返回全黑。这类目标在 macOS 上需要换
ScreenCaptureKit 再试，或者干脆要求用户不要把游戏放在被保护的窗口里。这是平台能力问题，不是
参数问题——所以 `doctor` 的第一条检查就是"截图耗时与内容标准差"。
