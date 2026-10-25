type LiveScore = { stepId: string; targetId: string; score: number; ms: number; reused: boolean };
type LogRow = { ts: number; level: string; event: string; payload?: unknown };

type Props = {
  publishedVersion: number;
  online: boolean;
  run: { runId: string; state: string; step?: number; errorCode?: string } | null;
  scores: LiveScore[];
  logs: LogRow[];
  onStart: () => void;
  onStop: () => void;
};

export function RunPanel({ publishedVersion, online, run, scores, logs, onStart, onStop }: Props) {
  const running = !!run && !["done", "failed", "stopped"].includes(run.state);
  const worst = [...scores].sort((a, b) => a.score - b.score).slice(0, 8);

  return (
    <div className="run-panel">
      <div className="run-head">
        <strong>运行</strong>
        <span className="dim">版本 v{publishedVersion || "—"}</span>
        <span className={`pill ${online ? "ok" : "off"}`}>{online ? "设备在线" : "设备离线"}</span>
      </div>

      <div className="run-actions">
        <button className="primary" disabled={!online || !publishedVersion || running} onClick={onStart}>
          开始执行
        </button>
        <button disabled={!running} onClick={onStop}>
          停止
        </button>
        {run && (
          <span className="dim">
            {run.state}
            {run.step !== undefined ? ` · 第 ${run.step + 1} 步` : ""}
            {run.errorCode ? ` · ${run.errorCode}` : ""}
          </span>
        )}
      </div>

      {worst.length > 0 && (
        <div className="score-grid">
          {worst.map((score) => (
            <div key={`${score.stepId}:${score.targetId}`} className={`score${score.score < 0.82 ? " low" : ""}`}>
              <span>{score.stepId}</span>
              <b>{score.score.toFixed(3)}</b>
              <span className="dim">
                {score.ms}ms{score.reused ? " · 复用" : ""}
              </span>
            </div>
          ))}
        </div>
      )}

      <div className="log-list">
        {logs.slice(-80).map((log, index) => (
          <div key={index} className={`log ${log.level}`}>
            <span className="dim">{new Date(log.ts).toLocaleTimeString()}</span> <b>{log.event}</b>
            {log.payload ? <code>{JSON.stringify(log.payload)}</code> : null}
          </div>
        ))}
        {logs.length === 0 && <p className="dim">暂无日志</p>}
      </div>
    </div>
  );
}
