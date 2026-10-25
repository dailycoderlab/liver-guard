import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, getToken, setToken } from "./api";
import { CropEditor } from "./crop_editor";
import {
  blobToDataUrl,
  computeAnchor,
  cropToPng,
  inferRoi,
  normalizedToLogical,
  pixelToLogical,
  sha256Hex,
  type LogicalBox,
} from "./crop";
import { detectCandidates } from "./detect";
import { LiveView, type ClickMark } from "./live";
import { RunPanel } from "./run_panel";
import { StepEditor } from "./step_editor";
import type { Bounds, Candidate, DeviceInfo, DraftIR, PixelBox, Step, StoredStep, TemplateTarget } from "./types";
import { emptyDraft } from "./types";
import { connectConsole, type ConsoleSocket } from "./ws";

type ContextState = {
  reqId: string;
  url: string;
  imageSize: { w: number; h: number };
  windowRect: Bounds;
  clickPx: [number, number];
  axBox: PixelBox | null;
};

type RunState = { runId: string; state: string; step?: number; errorCode?: string };
type LiveScore = { stepId: string; targetId: string; score: number; ms: number; reused: boolean };
type LogRow = { ts: number; level: string; event: string; payload?: unknown };

const DEFAULT_CAPS_SCALE = 1;

export default function App() {
  const [authed, setAuthed] = useState(!!getToken());
  const [password, setPassword] = useState("");
  const [loginError, setLoginError] = useState<string | null>(null);

  const [devices, setDevices] = useState<DeviceInfo[]>([]);
  const [deviceId, setDeviceId] = useState<string | null>(null);
  const [status, setStatus] = useState<"connecting" | "open" | "closed">("closed");
  const [toast, setToast] = useState<string | null>(null);

  const [frameUrl, setFrameUrl] = useState<string | null>(null);
  const [bounds, setBounds] = useState<Bounds | null>(null);
  const [marks, setMarks] = useState<ClickMark[]>([]);
  const [busy, setBusy] = useState(false);

  const [context, setContext] = useState<ContextState | null>(null);
  const [candidates, setCandidates] = useState<Candidate[]>([]);
  const [selectedCandidate, setSelectedCandidate] = useState(0);
  const [box, setBox] = useState<PixelBox | null>(null);
  const [strictness, setStrictness] = useState<"loose" | "normal" | "tight">("normal");

  const [scripts, setScripts] = useState<Array<{ id: string; name: string; currentVersion: number }>>([]);
  const [scriptId, setScriptId] = useState<string | null>(null);
  const [scriptName, setScriptName] = useState("");
  const [publishedVersion, setPublishedVersion] = useState(0);
  const [steps, setSteps] = useState<StoredStep[]>([]);
  const [selectedStep, setSelectedStep] = useState(-1);
  const [dirty, setDirty] = useState(false);
  const [publishing, setPublishing] = useState(false);
  const [publishMessage, setPublishMessage] = useState<string | null>(null);

  const [run, setRun] = useState<RunState | null>(null);
  const [scores, setScores] = useState<LiveScore[]>([]);
  const [logs, setLogs] = useState<LogRow[]>([]);

  const socketRef = useRef<ConsoleSocket | null>(null);
  const seqRef = useRef(0);
  const pendingRef = useRef<{ reqId: string; timer: number } | null>(null);
  const contextBitmapRef = useRef<ImageBitmap | null>(null);
  const contextDataRef = useRef<ImageData | null>(null);
  const lastConfirmRef = useRef(0);
  const frameObjectUrlRef = useRef<string | null>(null);
  const contextObjectUrlRef = useRef<string | null>(null);

  const selectedDevice = useMemo(() => devices.find((device) => device.id === deviceId) ?? null, [devices, deviceId]);
  const scale = selectedDevice?.bounds?.scale ?? context?.windowRect.scale ?? DEFAULT_CAPS_SCALE;

  const flash = useCallback((message: string) => {
    setToast(message);
    window.setTimeout(() => setToast((current) => (current === message ? null : current)), 3200);
  }, []);

  // ------------------------------------------------------------------ login

  const login = async () => {
    setLoginError(null);
    try {
      const { token } = await api.login(password);
      setToken(token);
      setAuthed(true);
    } catch (error) {
      setLoginError(error instanceof Error ? error.message : String(error));
    }
  };

  const loadDevices = useCallback(async () => {
    const { devices: list } = await api.devices();
    setDevices(list);
    setDeviceId((current) => current ?? list.find((device) => device.online)?.id ?? list[0]?.id ?? null);
  }, []);

  const loadScripts = useCallback(async () => {
    const { scripts: list } = await api.scripts();
    setScripts(list);
    return list;
  }, []);

  useEffect(() => {
    if (!authed) return;
    loadDevices().catch((error) => flash(`加载设备失败：${error}`));
    loadScripts().catch((error) => flash(`加载脚本失败：${error}`));
  }, [authed, loadDevices, loadScripts, flash]);

  // ------------------------------------------------------------- detection

  const runDetection = useCallback(
    (data: ImageData, clickPx: [number, number], windowScale: number, axBox: PixelBox | null, mode: typeof strictness) => {
      const found = detectCandidates(data.data as unknown as Uint8ClampedArray, data.width, data.height, { x: clickPx[0], y: clickPx[1] }, {
        scale: windowScale,
        strictness: mode,
        axBox,
      });
      setCandidates(found);
      setSelectedCandidate(0);
      setBox(found[0] ? { x: found[0].x, y: found[0].y, w: found[0].w, h: found[0].h } : null);
    },
    [],
  );

  // ----------------------------------------------------------------- socket

  useEffect(() => {
    if (!authed) return;
    const token = getToken();
    if (!token) return;

    const socket = connectConsole(token, {
      onStatus: setStatus,
      onJson: (message: any) => {
        switch (message.t) {
          case "device_state":
            setDevices((current) => {
              const next = current.map((device) =>
                device.id === message.deviceId
                  ? { ...device, online: message.online, caps: message.caps ?? device.caps, bounds: message.bounds ?? device.bounds, name: message.name ?? device.name }
                  : device,
              );
              return next.some((device) => device.id === message.deviceId)
                ? next
                : [
                    ...next,
                    {
                      id: message.deviceId,
                      name: message.name ?? message.deviceId,
                      platform: message.platform ?? "unknown",
                      online: message.online,
                      caps: message.caps ?? null,
                      bounds: message.bounds ?? null,
                    },
                  ];
            });
            break;
          case "stale_frame":
            pendingRef.current && window.clearTimeout(pendingRef.current.timer);
            pendingRef.current = null;
            setBusy(false);
            flash("画面已经变化，请重新点一下");
            break;
          case "run_state":
            setRun({ runId: message.runId, state: message.state, step: message.step, errorCode: message.errorCode });
            break;
          case "score":
            setScores(message.items ?? []);
            break;
          case "log":
            setLogs((current) => [...current, ...(message.items ?? [])].slice(-400));
            break;
          case "run_summary":
            setRun({ runId: message.runId, state: message.state, errorCode: message.errorCode });
            break;
          case "error":
            flash(`设备错误：${message.code}`);
            break;
          default:
            break;
        }
      },
      onImage: async (header, blob) => {
        if (header.t === "frame") {
          seqRef.current = header.seq;
          setBounds(header.bounds);
          const url = URL.createObjectURL(blob);
          if (frameObjectUrlRef.current) URL.revokeObjectURL(frameObjectUrlRef.current);
          frameObjectUrlRef.current = url;
          setFrameUrl(url);
          return;
        }

        if (header.t === "context" || header.t === "recrop_context") {
          const bitmap = await createImageBitmap(blob);
          contextBitmapRef.current?.close();
          contextBitmapRef.current = bitmap;

          const canvas = document.createElement("canvas");
          canvas.width = bitmap.width;
          canvas.height = bitmap.height;
          const ctx = canvas.getContext("2d", { willReadFrequently: true });
          if (!ctx) return;
          ctx.drawImage(bitmap, 0, 0);
          const data = ctx.getImageData(0, 0, bitmap.width, bitmap.height);
          contextDataRef.current = data;

          const clickPx = header.clickPx ?? [Math.round(bitmap.width / 2), Math.round(bitmap.height / 2)];
          const url = URL.createObjectURL(blob);
          if (contextObjectUrlRef.current) URL.revokeObjectURL(contextObjectUrlRef.current);
          contextObjectUrlRef.current = url;

          setContext({
            reqId: header.reqId,
            url,
            imageSize: { w: bitmap.width, h: bitmap.height },
            windowRect: header.windowRect,
            clickPx: [clickPx[0], clickPx[1]],
            axBox: header.axBox ? boxFromBounds(header.axBox, header.windowRect, header.windowRect.scale) : null,
          });
          runDetection(data, [clickPx[0], clickPx[1]], header.windowRect.scale, header.axBox ? boxFromBounds(header.axBox, header.windowRect, header.windowRect.scale) : null, strictness);

          if (pendingRef.current) {
            window.clearTimeout(pendingRef.current.timer);
            pendingRef.current = null;
          }
          setBusy(false);
        }
      },
    });

    socketRef.current = socket;
    return () => {
      socket.close();
      socketRef.current = null;
    };
  }, [authed, runDetection, strictness, flash]);

  useEffect(() => {
    if (!deviceId || status !== "open") return;
    socketRef.current?.send({ t: "subscribe", deviceId });
    socketRef.current?.send({ t: "stream", on: true, fps: 6, quality: 60 });
    return () => {
      socketRef.current?.send({ t: "stream", on: false, fps: 6, quality: 60 });
    };
  }, [deviceId, status]);

  // ------------------------------------------------------------------ clicks

  const handleSurfaceClick = (normalized: [number, number]) => {
    if (!socketRef.current || !deviceId) {
      flash("请先选择一台在线设备");
      return;
    }
    const reqId = crypto.randomUUID();
    setBusy(true);
    setMarks((current) => [...current.slice(-19), { id: Date.now(), nx: normalized[0], ny: normalized[1], ts: Date.now() }]);

    socketRef.current.send({
      t: "inject",
      reqId,
      kind: "tap",
      at: normalized,
      frameSeq: seqRef.current,
      record: true,
      durationMs: 60,
    });

    const timer = window.setTimeout(() => {
      if (pendingRef.current?.reqId === reqId) {
        pendingRef.current = null;
        setBusy(false);
        flash("设备没有在 6 秒内回应，检查客户端是否在线");
      }
    }, 6000);
    pendingRef.current = { reqId, timer };
  };

  const reselect = (index: number) => {
    setSelectedCandidate(index);
    const candidate = candidates[index];
    if (candidate) setBox({ x: candidate.x, y: candidate.y, w: candidate.w, h: candidate.h });
  };

  const changeStrictness = (value: typeof strictness) => {
    setStrictness(value);
    if (contextDataRef.current && context) {
      runDetection(contextDataRef.current, context.clickPx, context.windowRect.scale, context.axBox, value);
    }
  };

  // ---------------------------------------------------------------- confirm

  const confirmStep = async () => {
    const bitmap = contextBitmapRef.current;
    if (!context || !box || !bitmap || !bounds) return;
    setBusy(true);
    try {
      const blob = await cropToPng(bitmap, box);
      const sha = await sha256Hex(blob);
      const windowScale = context.windowRect.scale;

      await api.putAsset(sha, blob, { w: box.w, h: box.h, scale: windowScale });

      const clickLogical = normalizedToLogical(
        [context.clickPx[0] / context.imageSize.w, context.clickPx[1] / context.imageSize.h],
        context.windowRect,
      );
      const logicalBox: LogicalBox = pixelToLogical(box, context.windowRect, windowScale);
      const anchor = computeAnchor(clickLogical, logicalBox);
      const roi = inferRoi(logicalBox, bounds);

      const chosen = candidates[selectedCandidate];
      const target: TemplateTarget = {
        type: "template",
        asset: sha,
        assetBox: { w: box.w, h: box.h },
        recordScale: windowScale,
        // Recorded surface width: the anchor for normalizing this crop later.
        recordWidth: bounds.w,
        roi,
        anchor,
        strategy: ["tpl", "mstpl"],
        scale: { mode: "auto", min: 0.7, max: 1.5, steps: 9 },
        threshold: 0.82,
        warnThreshold: 0.7,
        verify: "rgb",
        mask: "none",
        multi: "nearest-last",
        auto: chosen ? chosen.kind !== "fallback" : false,
      };

      const index = steps.length + 1;
      const targetKey = `t${index}`;
      const thumb = await blobToDataUrl(blob);

      const additions: StoredStep[] = [];
      const gap = lastConfirmRef.current ? Date.now() - lastConfirmRef.current : 0;
      if (gap > 800) {
        additions.push({ step: { op: "sleep", id: `s${index}`, ms: Math.min(60000, Math.round(gap / 100) * 100) } });
      }
      additions.push({
        step: { op: "tap", id: targetKey, target: targetKey },
        targetKey,
        target,
        thumb,
        box: { pixel: box, logical: logicalBox },
        confidence: chosen?.kind ?? "fallback",
      });
      lastConfirmRef.current = Date.now();

      setSteps((current) => [...current, ...additions]);
      setDirty(true);
      setContext(null);
      setCandidates([]);
      setBox(null);
      contextBitmapRef.current?.close();
      contextBitmapRef.current = null;
    } catch (error) {
      flash(`保存裁图失败：${error instanceof Error ? error.message : error}`);
    } finally {
      setBusy(false);
    }
  };

  // ------------------------------------------------------------ draft sync

  const buildIR = useCallback(
    (items: StoredStep[], name: string): DraftIR => {
      const ir = emptyDraft(name || "未命名脚本");
      const targets: Record<string, TemplateTarget> = {};
      for (const item of items) {
        if (item.targetKey && item.target) targets[item.targetKey] = item.target;
      }
      ir.meta.name = name || "未命名脚本";
      ir.blocks.main = items.map((item) => item.step);
      ir.targets = targets;
      return ir;
    },
    [],
  );

  useEffect(() => {
    if (!scriptId || !dirty) return;
    const handle = window.setTimeout(() => {
      api
        .saveDraft(scriptId, buildIR(steps, scriptName))
        .then(() => setDirty(false))
        .catch(() => undefined);
    }, 2000);
    return () => window.clearTimeout(handle);
  }, [steps, scriptId, dirty, scriptName, buildIR]);

  const openScript = async (id: string) => {
    const { script } = await api.script(id);
    setScriptId(id);
    setScriptName(script.name);
    setPublishedVersion(script.currentVersion);
    setSteps([]);
    setDirty(false);
    setPublishMessage(
      script.draft
        ? `已有草稿：${(script.draft.blocks?.main ?? []).length} 步（当前编辑器尚未载入草稿内容）`
        : null,
    );
  };

  const createScript = async () => {
    const name = window.prompt("脚本名称", `脚本 ${scripts.length + 1}`);
    if (!name) return;
    const { script } = await api.createScript(name);
    await loadScripts();
    await openScript(script.id);
  };

  const publish = async () => {
    if (steps.length === 0) return;
    setPublishing(true);
    setPublishMessage(null);
    try {
      const ir = buildIR(steps, scriptName);
      const result = await api.publish(scriptId!, ir, `录制共 ${steps.length} 步`);
      setPublishedVersion(result.version);
      setPublishMessage(`已发布 v${result.version}`);
      setDirty(false);
    } catch (error) {
      const detail = error instanceof Error ? error.message : String(error);
      setPublishMessage(`发布失败：${detail}`);
    } finally {
      setPublishing(false);
    }
  };

  // -------------------------------------------------------------------- run

  const startRun = async () => {
    if (!deviceId || !scriptId) return;
    setScores([]);
    setLogs([]);
    try {
      const { runId } = await api.startRun(deviceId, scriptId, publishedVersion || undefined);
      setRun({ runId, state: "starting" });
    } catch (error) {
      flash(`启动失败：${error instanceof Error ? error.message : error}`);
    }
  };

  const stopRun = async () => {
    if (!run) return;
    await api.stopRun(run.runId).catch(() => undefined);
  };

  // ------------------------------------------------------------------ views

  if (!authed) {
    return (
      <div className="login">
        <h1>Sky 录制台</h1>
        <p className="dim">输入控制台口令。口令保存在 Worker 的 ADMIN_PASSWORD 密钥里。</p>
        <input
          type="password"
          value={password}
          placeholder="控制台口令"
          onChange={(event) => setPassword(event.target.value)}
          onKeyDown={(event) => event.key === "Enter" && login()}
        />
        <button className="primary" onClick={login}>
          进入
        </button>
        {loginError && <p className="warn">{loginError}</p>}
      </div>
    );
  }

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="side-block">
          <div className="side-head">
            <strong>设备</strong>
            <span className={`dot ${status === "open" ? "ok" : "off"}`} title={status} />
          </div>
          {devices.map((device) => (
            <button
              key={device.id}
              className={`device${device.id === deviceId ? " active" : ""}`}
              onClick={() => setDeviceId(device.id)}
            >
              <span>{device.name}</span>
              <span className="dim">{device.online ? "在线" : "离线"}</span>
            </button>
          ))}
          {devices.length === 0 && <p className="dim">还没有注册设备。用 `sky enroll` 在客户端注册。</p>}
          <button className="ghost" onClick={() => loadDevices().catch(() => undefined)}>
            刷新
          </button>
        </div>

        <div className="side-block">
          <div className="side-head">
            <strong>脚本</strong>
            <button className="ghost" onClick={createScript}>
              + 新建
            </button>
          </div>
          {scripts.map((script) => (
            <button
              key={script.id}
              className={`device${script.id === scriptId ? " active" : ""}`}
              onClick={() => openScript(script.id)}
            >
              <span>{script.name}</span>
              <span className="dim">v{script.currentVersion}</span>
            </button>
          ))}
          {scripts.length === 0 && <p className="dim">还没有脚本。</p>}
        </div>

        <div className="side-block">
          <p className="dim small">
            客户端不跑任何图像检测：它只把点击点周围的搜索窗推过来，检测与裁图都在这个页面上完成。
          </p>
        </div>
      </aside>

      <main className="main">
        <section className="pane">
          <header className="pane-head">
            <h2>实时画面</h2>
            {selectedDevice && (
              <span className="dim">
                {selectedDevice.name} · {selectedDevice.platform}
                {selectedDevice.caps ? ` · 上限 ${selectedDevice.caps.maxFps} fps · UI 查询 ${selectedDevice.caps.uiQuery}` : ""}
              </span>
            )}
          </header>
          <LiveView frameUrl={frameUrl} bounds={bounds} marks={marks} busy={busy} onClickSurface={handleSurfaceClick} />
        </section>

        <section className="pane">
          <header className="pane-head">
            <h2>裁图</h2>
            <button className="ghost" disabled={!context} onClick={() => setMarks([])}>
              清除标记
            </button>
          </header>
          <CropEditor
            contextUrl={context?.url ?? null}
            imageSize={context?.imageSize ?? null}
            windowRect={context?.windowRect ?? null}
            clickPx={context?.clickPx ?? null}
            candidates={candidates}
            selected={selectedCandidate}
            box={box}
            strictness={strictness}
            busy={busy}
            onSelect={reselect}
            onBoxChange={setBox}
            onStrictness={changeStrictness}
            onConfirm={confirmStep}
            onCancel={() => {
              setContext(null);
              setCandidates([]);
              setBox(null);
            }}
          />
        </section>

        <section className="pane">
          <StepEditor
            steps={steps}
            selected={selectedStep}
            dirty={dirty}
            publishing={publishing}
            message={publishMessage}
            onSelect={setSelectedStep}
            onDelete={(index) => {
              setSteps((current) => current.filter((_, i) => i !== index));
              setDirty(true);
            }}
            onMove={(index, delta) => {
              setSteps((current) => {
                const next = [...current];
                const target = index + delta;
                if (target < 0 || target >= next.length) return current;
                [next[index], next[target]] = [next[target], next[index]];
                return next;
              });
              setDirty(true);
            }}
            onInsertSleep={(index) => {
              setSteps((current) => {
                const next = [...current];
                next.splice(index + 1, 0, { step: { op: "sleep", id: `s${Date.now()}`, ms: 800 } as Step });
                return next;
              });
              setDirty(true);
            }}
            onThreshold={(index, value) => {
              setSteps((current) =>
                current.map((item, i) =>
                  i === index && item.target ? { ...item, target: { ...item.target, threshold: value } } : item,
                ),
              );
              setDirty(true);
            }}
            onPublish={publish}
            onClear={() => {
              setSteps([]);
              setDirty(true);
            }}
          />
        </section>

        <section className="pane">
          <RunPanel
            publishedVersion={publishedVersion}
            online={!!selectedDevice?.online}
            run={run}
            scores={scores}
            logs={logs}
            onStart={startRun}
            onStop={stopRun}
          />
        </section>
      </main>

      {toast && <div className="toast">{toast}</div>}
      {busy && <div className="busy-hint">等待设备…</div>}
      <span className="hidden-scale" data-scale={scale} />
    </div>
  );
}

function boxFromBounds(box: Bounds, window: Bounds, scale: number): PixelBox {
  return {
    x: (box.x - window.x) * scale,
    y: (box.y - window.y) * scale,
    w: box.w * scale,
    h: box.h * scale,
  };
}
