import type { StoredStep } from "./types";

type Props = {
  steps: StoredStep[];
  selected: number;
  dirty: boolean;
  publishing: boolean;
  message: string | null;
  onSelect: (index: number) => void;
  onDelete: (index: number) => void;
  onMove: (index: number, delta: number) => void;
  onInsertSleep: (index: number) => void;
  onThreshold: (index: number, value: number) => void;
  onPublish: () => void;
  onClear: () => void;
};

export function StepEditor(props: Props) {
  const { steps, selected } = props;

  return (
    <div className="steps">
      <div className="steps-head">
        <strong>脚本步骤</strong>
        <span className="dim">{steps.length} 步</span>
        {props.dirty && <span className="dot-dirty" title="有未发布的改动" />}
      </div>

      {steps.length === 0 && <p className="dim">还没有步骤。在左边实时画面上点一下控件，就会生成第一步。</p>}

      <ol className="step-list">
        {steps.map((item, index) => (
          <li
            key={index}
            className={`step${index === selected ? " active" : ""}`}
            onClick={() => props.onSelect(index)}
          >
            <span className="step-index">{index + 1}</span>
            {item.thumb ? <img className="thumb" src={item.thumb} alt={item.targetKey ?? item.step.op} /> : <span className="thumb" />}
            <div className="step-body">
              <div className="step-title">
                <code>{item.step.op}</code> <span>{item.targetKey ?? describe(item.step)}</span>
                {item.confidence === "ax" && <span className="tag">系统控件</span>}
                {item.confidence === "fallback" && <span className="tag warn">手动框选</span>}
              </div>
              {item.target && item.box ? (
                <>
                  <div className="step-meta dim">
                    {Math.round(item.box.logical.w)}×{Math.round(item.box.logical.h)} pt · anchor{" "}
                    {item.target.anchor.map((v) => v.toFixed(2)).join(", ")} · ROI{" "}
                    {item.target.roi.map((v) => v.toFixed(2)).join(", ")}
                  </div>
                  <div className="step-meta">
                    <label className="dim">
                      阈值
                      <input
                        type="number"
                        step="0.01"
                        min="0.5"
                        max="0.99"
                        value={item.target.threshold}
                        onClick={(event) => event.stopPropagation()}
                        onChange={(event) => props.onThreshold(index, Number(event.target.value))}
                      />
                    </label>
                  </div>
                </>
              ) : (
                <div className="step-meta dim">等待 {String(item.step.ms ?? "")} ms</div>
              )}
            </div>
            <div className="step-actions">
              <button title="上移" onClick={(event) => stop(event, () => props.onMove(index, -1))}>
                ↑
              </button>
              <button title="下移" onClick={(event) => stop(event, () => props.onMove(index, 1))}>
                ↓
              </button>
              <button title="插入等待" onClick={(event) => stop(event, () => props.onInsertSleep(index))}>
                ⏱
              </button>
              <button title="删除" onClick={(event) => stop(event, () => props.onDelete(index))}>
                ✕
              </button>
            </div>
          </li>
        ))}
      </ol>

      <div className="steps-actions">
        <button className="primary" disabled={props.publishing || steps.length === 0} onClick={props.onPublish}>
          {props.publishing ? "发布中…" : "发布版本"}
        </button>
        <button disabled={props.publishing || steps.length === 0} onClick={props.onClear}>
          清空
        </button>
        {props.message && <span className="dim">{props.message}</span>}
      </div>
    </div>
  );
}

function stop(event: React.MouseEvent, action: () => void) {
  event.stopPropagation();
  action();
}

function describe(step: { op: string; [key: string]: unknown }): string {
  if (step.op === "sleep") return `${step.ms} ms`;
  if (step.op === "log") return String(step.msg ?? "");
  return step.op;
}
