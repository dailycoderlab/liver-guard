//! The notices for an unattended run: what to say, and when to say it.
//!
//! A failure alert is sent when a run ends in `RunState::Failed`, i.e. a failure nobody handled:
//! a node without a wired `fail` / `timeout` handle, or a fault no handle can take (driver
//! error, stalled capture, step limit...). A failure the script routes on purpose is normal
//! control flow and stays silent; a manual stop is not a failure either.
//!
//! A looping run (more than one round) that finishes every round also sends a "done" notice,
//! unless `NotifyConfig::on_loop_done` is off. A single-round success sends nothing.

use std::sync::Arc;

use serde_json::json;
use liverguard_ir::IrDoc;
use liverguard_notify::{Message, NotifyConfig, NotifyError};

use crate::interp::RunResult;

/// How an alert is delivered; tests substitute their own.
pub type AlertSender = Arc<dyn Fn(&NotifyConfig, &Message) -> Result<(), NotifyError> + Send + Sync>;

/// The real thing: an HTTP POST to the configured webhook.
pub fn webhook_sender() -> AlertSender {
    Arc::new(|config, message| liverguard_notify::send(config, message))
}

/// A short operator-facing hint for a reason code.
fn hint(code: &str) -> &'static str {
    match code {
        "not_found" => "画面里没有找到目标",
        "timeout" => "等待超时",
        "capture_stalled" => "截图卡住了，屏幕录制权限可能失效，或目标窗口被全屏应用遮住",
        "driver_error" => "点击或截图失败，请检查系统权限和目标窗口",
        "stagnation" => "连续多次点击后画面没有变化",
        "visit_limit" | "step_limit" => "脚本疑似陷入死循环",
        "unknown_target" | "bad_step" => "脚本本身有问题",
        _ => "",
    }
}

fn duration_text(ms: u64) -> String {
    let seconds = ms / 1000;
    match (seconds / 3600, seconds / 60 % 60, seconds % 60) {
        (0, 0, s) => format!("{s} 秒"),
        (0, m, s) => format!("{m} 分 {s} 秒"),
        (h, m, s) => format!("{h} 小时 {m} 分 {s} 秒"),
    }
}

/// The message for a failed run. `round` is the round that failed, `rounds` the planned total.
pub fn failure_message(ir: &IrDoc, result: &RunResult, round: u32, rounds: u32) -> Message {
    let code = result.error_code.clone().unwrap_or_default();
    let reason = result.error_msg.clone().unwrap_or_default();
    let node = result.error_node.as_deref().and_then(|id| ir.flow.node(id));
    let elapsed_ms = result.ended_at.saturating_sub(result.started_at);

    let mut lines = vec!["【护肝】脚本运行失败".to_string(), format!("脚本：{}", ir.meta.name)];
    if let Some(id) = &result.error_node {
        let name = node.and_then(|n| n.label.clone().filter(|l| !l.is_empty())).unwrap_or_else(|| id.clone());
        let kind = node.map(|n| format!("（{}）", n.kind.type_name())).unwrap_or_default();
        lines.push(format!("节点：「{name}」{kind}"));
    }
    let hint = hint(&code);
    lines.push(format!("原因：{code}{}{}", if reason.is_empty() { "" } else { " — " }, reason));
    if !hint.is_empty() {
        lines.push(format!("可能是：{hint}"));
    }
    let progress = format!("已执行 {} 步，已运行 {}", result.step_count, duration_text(elapsed_ms));
    if rounds > 1 {
        lines.push(format!("进度：第 {round}/{rounds} 轮，{progress}"));
        if round < rounds {
            lines.push(format!("已停止循环，剩余 {} 轮不会再执行", rounds - round));
        }
    } else {
        lines.push(format!("进度：{progress}"));
    }
    lines.push(format!("时间：{}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S")));

    Message {
        event: "run_failed",
        title: "脚本运行失败".into(),
        text: lines.join("\n"),
        details: json!({
            "script": ir.meta.name,
            "errorCode": code,
            "errorMsg": reason,
            "errorNode": result.error_node,
            "nodeType": node.map(|n| n.kind.type_name()),
            "round": round,
            "rounds": rounds,
            "steps": result.step_count,
            "elapsedMs": elapsed_ms,
        }),
    }
}

/// The notice for a looping run that completed all `rounds` rounds.
pub fn loop_done_message(ir: &IrDoc, result: &RunResult, rounds: u32) -> Message {
    let elapsed_ms = result.ended_at.saturating_sub(result.started_at);
    let lines = [
        "【护肝】循环执行完成".to_string(),
        format!("脚本：{}", ir.meta.name),
        format!("结果：共 {rounds} 轮全部成功"),
        format!("已执行 {} 步，总用时 {}", result.step_count, duration_text(elapsed_ms)),
        format!("时间：{}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S")),
    ];
    Message {
        event: "loop_done",
        title: "循环执行完成".into(),
        text: lines.join("\n"),
        details: json!({
            "script": ir.meta.name,
            "rounds": rounds,
            "steps": result.step_count,
            "elapsedMs": elapsed_ms,
        }),
    }
}
