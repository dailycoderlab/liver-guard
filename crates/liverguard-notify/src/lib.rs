//! Failure alerts over webhooks.
//!
//! One text message per alert, sent to a group-robot URL. Request building and response
//! checking are pure functions so they can be tested without a network; `send` adds the
//! HTTP call and a small retry.
//!
//! Errors never contain the webhook URL: it carries the robot's access token, and errors
//! end up in run logs.

use std::time::Duration;

use base64::Engine as _;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

const ATTEMPTS: u32 = 3;
const TIMEOUT: Duration = Duration::from_secs(10);
/// Longest response body kept for an error message.
const BODY_SNIPPET: usize = 200;

/// Which robot flavour the URL belongs to; they differ in payload and in how a signing
/// secret is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    /// 钉钉自定义机器人（支持「加签」与「自定义关键词」）。
    #[default]
    Dingtalk,
    /// 企业微信群机器人。
    Wecom,
    /// 飞书自定义机器人（支持「签名校验」）。
    Feishu,
    /// POST a JSON object `{ event, title, text, details }` to any URL.
    Generic,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotifyConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub channel: Channel,
    /// The robot's webhook URL, access token included.
    #[serde(default)]
    pub url: String,
    /// Signing secret (DingTalk 加签 / Feishu 签名校验); empty when the robot has none.
    #[serde(default)]
    pub secret: String,
    /// The robot's keyword rule: the text must contain it. It is prepended when missing.
    #[serde(default)]
    pub keyword: String,
    /// Also tell when a looping run (more than one round) finishes every round successfully.
    /// Failures are always reported when `enabled`; this only adds the success notice.
    #[serde(default = "yes")]
    pub on_loop_done: bool,
}

fn yes() -> bool {
    true
}

impl Default for NotifyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            channel: Channel::default(),
            url: String::new(),
            secret: String::new(),
            keyword: String::new(),
            on_loop_done: true,
        }
    }
}

/// What to tell the people watching.
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    /// What happened, as a stable machine-readable name (`run_failed`, `loop_done`, `test`);
    /// sent to `Channel::Generic` as `event`.
    pub event: &'static str,
    pub title: String,
    pub text: String,
    /// Machine-readable extras, sent only to `Channel::Generic`.
    pub details: Value,
}

#[derive(Debug, thiserror::Error)]
pub enum NotifyError {
    #[error("{0}")]
    Invalid(String),
    #[error("网络错误：{0}")]
    Network(String),
    #[error("服务器返回 HTTP {status}：{body}")]
    Http { status: u16, body: String },
    #[error("机器人拒绝了消息：{0}")]
    Rejected(String),
}

impl NotifyError {
    /// Worth another attempt: the message may simply not have arrived.
    fn is_transient(&self) -> bool {
        match self {
            NotifyError::Network(_) => true,
            NotifyError::Http { status, .. } => *status >= 500 || *status == 429,
            _ => false,
        }
    }
}

/// A request ready to POST.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub url: String,
    pub body: Value,
}

/// Reject configurations that can never work, with a message the settings form can show.
pub fn validate(config: &NotifyConfig) -> Result<(), NotifyError> {
    let url = config.url.trim();
    if url.is_empty() {
        return Err(NotifyError::Invalid("请填写 Webhook 地址".into()));
    }
    if !(url.starts_with("https://") || url.starts_with("http://")) || url.contains(char::is_whitespace) {
        return Err(NotifyError::Invalid("Webhook 地址必须是 http:// 或 https:// 开头的链接".into()));
    }
    if !config.secret.trim().is_empty() && matches!(config.channel, Channel::Wecom | Channel::Generic) {
        return Err(NotifyError::Invalid("这种接收方不支持签名密钥，请留空".into()));
    }
    Ok(())
}

fn with_keyword(config: &NotifyConfig, text: &str) -> String {
    let keyword = config.keyword.trim();
    if keyword.is_empty() || text.contains(keyword) {
        text.to_string()
    } else {
        format!("{keyword}\n{text}")
    }
}

fn hmac_base64(key: &[u8], message: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts keys of any length");
    mac.update(message);
    base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
}

/// Percent-encode everything but RFC 3986 unreserved characters.
fn percent_encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// DingTalk 加签: `sign = urlencode(base64(HMAC-SHA256(key = secret, msg = "{ts}\n{secret}")))`,
/// `ts` in milliseconds, both appended to the URL.
fn dingtalk_signature(secret: &str, timestamp_ms: u64) -> String {
    percent_encode(&hmac_base64(secret.as_bytes(), format!("{timestamp_ms}\n{secret}").as_bytes()))
}

/// Feishu 签名校验: `sign = base64(HMAC-SHA256(key = "{ts}\n{secret}", msg = ""))`, `ts` in
/// seconds, both sent in the JSON body.
fn feishu_signature(secret: &str, timestamp_s: u64) -> String {
    hmac_base64(format!("{timestamp_s}\n{secret}").as_bytes(), b"")
}

/// Build the POST for `message`. `now_ms` only matters when a secret signs the request.
pub fn build_request(config: &NotifyConfig, message: &Message, now_ms: u64) -> Result<Request, NotifyError> {
    validate(config)?;
    let url = config.url.trim().to_string();
    let secret = config.secret.trim();
    let text = with_keyword(config, &message.text);
    Ok(match config.channel {
        Channel::Dingtalk => {
            let url = if secret.is_empty() {
                url
            } else {
                let joiner = if url.contains('?') { '&' } else { '?' };
                format!("{url}{joiner}timestamp={now_ms}&sign={}", dingtalk_signature(secret, now_ms))
            };
            Request { url, body: json!({ "msgtype": "text", "text": { "content": text } }) }
        }
        Channel::Wecom => Request { url, body: json!({ "msgtype": "text", "text": { "content": text } }) },
        Channel::Feishu => {
            let mut body = json!({ "msg_type": "text", "content": { "text": text } });
            if !secret.is_empty() {
                let seconds = now_ms / 1000;
                body["timestamp"] = Value::String(seconds.to_string());
                body["sign"] = Value::String(feishu_signature(secret, seconds));
            }
            Request { url, body }
        }
        Channel::Generic => Request {
            url,
            body: json!({ "event": message.event, "title": message.title, "text": text, "details": message.details }),
        },
    })
}

fn snippet(body: &str) -> String {
    let body = body.trim();
    match body.char_indices().nth(BODY_SNIPPET) {
        Some((end, _)) => format!("{}…", &body[..end]),
        None => body.to_string(),
    }
}

/// Interpret a response. DingTalk, WeCom and Feishu answer HTTP 200 even when they reject the
/// message (wrong keyword, bad signature, rate limit), so the body has to be read.
pub fn check_response(channel: Channel, status: u16, body: &str) -> Result<(), NotifyError> {
    if !(200..300).contains(&status) {
        return Err(NotifyError::Http { status, body: snippet(body) });
    }
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let rejected = |code: Option<i64>, message: Option<&str>| {
        Err(NotifyError::Rejected(format!("错误码 {}：{}", code.unwrap_or(-1), message.filter(|m| !m.is_empty()).unwrap_or("无说明"))))
    };
    match (channel, parsed) {
        (Channel::Dingtalk | Channel::Wecom, Some(v)) => match v["errcode"].as_i64() {
            Some(0) | None => Ok(()),
            code => rejected(code, v["errmsg"].as_str()),
        },
        (Channel::Feishu, Some(v)) => {
            // Newer bots answer `code`, older ones `StatusCode`.
            match v["code"].as_i64().or_else(|| v["StatusCode"].as_i64()) {
                Some(0) | None => Ok(()),
                code => rejected(code, v["msg"].as_str().or_else(|| v["StatusMessage"].as_str())),
            }
        }
        _ => Ok(()),
    }
}

fn post(request: &Request, channel: Channel) -> Result<(), NotifyError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .http_status_as_error(false)
        .build()
        .into();
    // The error text is rebuilt on purpose: ureq's own messages can include the URL.
    let mut response = agent
        .post(&request.url)
        .header("Content-Type", "application/json; charset=utf-8")
        .send(request.body.to_string())
        .map_err(|e| NotifyError::Network(network_hint(&e)))?;
    let status = response.status().as_u16();
    let body = response.body_mut().read_to_string().unwrap_or_default();
    check_response(channel, status, &body)
}

fn network_hint(error: &ureq::Error) -> String {
    match error {
        ureq::Error::Timeout(_) => "请求超时".into(),
        ureq::Error::HostNotFound => "域名解析失败".into(),
        ureq::Error::ConnectionFailed => "连接失败".into(),
        ureq::Error::Tls(_) | ureq::Error::Rustls(_) => "TLS 握手失败".into(),
        _ => "请求失败".into(),
    }
}

/// Send `message`, retrying transient failures (network errors, 5xx, 429) a couple of times.
/// Blocks for at most a few tens of seconds; call it off the UI thread.
pub fn send(config: &NotifyConfig, message: &Message) -> Result<(), NotifyError> {
    let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    let request = build_request(config, message, now_ms)?;
    let mut last = None;
    for attempt in 0..ATTEMPTS {
        if attempt > 0 {
            std::thread::sleep(Duration::from_secs(u64::from(attempt)));
        }
        match post(&request, config.channel) {
            Ok(()) => return Ok(()),
            Err(error) if error.is_transient() => last = Some(error),
            Err(error) => return Err(error),
        }
    }
    Err(last.expect("at least one attempt ran"))
}

/// The message the settings form's test button sends.
pub fn test_message() -> Message {
    Message {
        event: "test",
        title: "测试通知".into(),
        text: "【护肝】测试通知：收到这条消息说明通知的 Webhook 配置可用。".into(),
        details: json!({ "test": true }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(channel: Channel) -> NotifyConfig {
        NotifyConfig { enabled: true, channel, url: "https://example.com/robot/send?access_token=T".into(), ..Default::default() }
    }

    fn message() -> Message {
        Message { event: "run_failed", title: "失败".into(), text: "脚本失败了".into(), details: json!({ "script": "契灵" }) }
    }

    #[test]
    fn dingtalk_posts_a_text_message() {
        let request = build_request(&config(Channel::Dingtalk), &message(), 1).unwrap();
        assert_eq!(request.url, "https://example.com/robot/send?access_token=T");
        assert_eq!(request.body, json!({ "msgtype": "text", "text": { "content": "脚本失败了" } }));
    }

    #[test]
    fn dingtalk_signs_with_timestamp_and_encoded_signature() {
        // Reference value computed with Python's hmac/base64/urllib for the same inputs.
        let mut cfg = config(Channel::Dingtalk);
        cfg.secret = "SECabc".into();
        let request = build_request(&cfg, &message(), 1_700_000_000_000).unwrap();
        assert!(request.url.starts_with("https://example.com/robot/send?access_token=T&timestamp=1700000000000&sign="));
        assert_eq!(dingtalk_signature("SECabc", 1_700_000_000_000), "jcUpW0QmtKduN03n4JqQ0PBosVjqnM8gU7fIIvsDmCM%3D");
        // `+`, `/` and `=` of base64 must be percent-encoded in the query.
        assert!(!request.url.split("sign=").nth(1).unwrap().contains(['+', '/', '=']));
    }

    #[test]
    fn feishu_signs_in_the_body_with_seconds() {
        let mut cfg = config(Channel::Feishu);
        cfg.secret = "SECabc".into();
        let request = build_request(&cfg, &message(), 1_700_000_123_456).unwrap();
        assert_eq!(request.body["timestamp"], "1700000123");
        assert_eq!(request.body["sign"], "G2zGJLgIW63SzyMrHNVepLZux0morfQ6o+EMM0xSGLk=");
        assert_eq!(request.body["msg_type"], "text");
        assert_eq!(request.body["content"]["text"], "脚本失败了");
        assert!(!request.url.contains("sign"));
    }

    #[test]
    fn the_keyword_is_prepended_only_when_missing() {
        let mut cfg = config(Channel::Wecom);
        cfg.keyword = "护肝".into();
        let plain = Message { text: "出错了".into(), ..message() };
        assert_eq!(build_request(&cfg, &plain, 1).unwrap().body["text"]["content"], "护肝\n出错了");
        let tagged = Message { text: "【护肝】出错了".into(), ..message() };
        assert_eq!(build_request(&cfg, &tagged, 1).unwrap().body["text"]["content"], "【护肝】出错了");
    }

    #[test]
    fn generic_carries_the_details() {
        let request = build_request(&config(Channel::Generic), &message(), 1).unwrap();
        assert_eq!(request.body["event"], "run_failed");

        let done = Message { event: "loop_done", ..message() };
        assert_eq!(build_request(&config(Channel::Generic), &done, 1).unwrap().body["event"], "loop_done");
        assert_eq!(request.body["details"]["script"], "契灵");
    }

    #[test]
    fn configurations_that_cannot_work_are_rejected() {
        let mut cfg = config(Channel::Dingtalk);
        cfg.url = "  ".into();
        assert!(validate(&cfg).is_err());
        cfg.url = "ftp://x".into();
        assert!(validate(&cfg).is_err());
        cfg.url = "https://a b".into();
        assert!(validate(&cfg).is_err());
        let mut cfg = config(Channel::Wecom);
        cfg.secret = "x".into();
        assert!(validate(&cfg).is_err());
        assert!(validate(&config(Channel::Feishu)).is_ok());
    }

    #[test]
    fn robots_that_answer_200_with_an_error_code_count_as_rejected() {
        assert!(check_response(Channel::Dingtalk, 200, r#"{"errcode":0,"errmsg":"ok"}"#).is_ok());
        let error = check_response(Channel::Dingtalk, 200, r#"{"errcode":310000,"errmsg":"keywords not in content"}"#).unwrap_err();
        assert!(matches!(error, NotifyError::Rejected(_)));
        assert!(error.to_string().contains("keywords not in content"));
        assert!(check_response(Channel::Wecom, 200, r#"{"errcode":93000,"errmsg":"invalid webhook url"}"#).is_err());
        assert!(check_response(Channel::Feishu, 200, r#"{"code":0,"msg":"success"}"#).is_ok());
        assert!(check_response(Channel::Feishu, 200, r#"{"code":19021,"msg":"sign match fail"}"#).is_err());
        assert!(check_response(Channel::Feishu, 200, r#"{"StatusCode":0}"#).is_ok());
        // Generic only looks at the status, and an HTML body from a proxy is not an error by itself.
        assert!(check_response(Channel::Generic, 204, "").is_ok());
        assert!(check_response(Channel::Dingtalk, 200, "<html>ok</html>").is_ok());
    }

    #[test]
    fn http_errors_keep_a_short_body_and_server_errors_are_retried() {
        let long = "x".repeat(1000);
        let error = check_response(Channel::Generic, 502, &long).unwrap_err();
        assert!(error.is_transient());
        assert!(error.to_string().chars().count() < 300);
        assert!(!check_response(Channel::Generic, 404, "no").unwrap_err().is_transient());
        assert!(!NotifyError::Rejected("x".into()).is_transient());
    }

    #[test]
    fn errors_never_mention_the_webhook_url() {
        let cfg = config(Channel::Dingtalk);
        let error = check_response(Channel::Dingtalk, 400, "bad").unwrap_err();
        assert!(!error.to_string().contains(&cfg.url));
        assert_eq!(network_hint(&ureq::Error::HostNotFound), "域名解析失败");
    }

    /// Serve `replies` to successive connections on a loopback port; returns the URL and the
    /// request bodies seen so far.
    fn serve(replies: Vec<(u16, &'static str)>) -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/robot/send?access_token=SECRET", listener.local_addr().unwrap());
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = seen.clone();
        std::thread::spawn(move || {
            for (status, body) in replies {
                let Ok((mut stream, _)) = listener.accept() else { return };
                let mut buffer = vec![0u8; 8192];
                let mut read = 0;
                // Read until the headers and the whole JSON body have arrived.
                loop {
                    let n = stream.read(&mut buffer[read..]).unwrap_or(0);
                    read += n;
                    let text = String::from_utf8_lossy(&buffer[..read]).to_string();
                    if n == 0 || text.rsplit("\r\n\r\n").next().is_some_and(|b| b.ends_with('}')) {
                        log.lock().unwrap().push(text.rsplit("\r\n\r\n").next().unwrap_or_default().to_string());
                        break;
                    }
                }
                let reply = format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        (url, seen)
    }

    #[test]
    fn send_posts_json_and_accepts_an_ok_reply() {
        let (url, seen) = serve(vec![(200, r#"{"errcode":0,"errmsg":"ok"}"#)]);
        let cfg = NotifyConfig { url, ..config(Channel::Dingtalk) };
        send(&cfg, &message()).unwrap();
        let bodies = seen.lock().unwrap();
        assert_eq!(bodies.len(), 1);
        let sent: Value = serde_json::from_str(&bodies[0]).unwrap();
        assert_eq!(sent["text"]["content"], "脚本失败了");
    }

    #[test]
    fn send_reports_a_rejection_without_retrying_or_leaking_the_url() {
        let (url, seen) = serve(vec![(200, r#"{"errcode":310000,"errmsg":"keywords not in content"}"#)]);
        let cfg = NotifyConfig { url, ..config(Channel::Dingtalk) };
        let error = send(&cfg, &message()).unwrap_err();
        assert!(matches!(error, NotifyError::Rejected(_)), "{error}");
        assert!(!error.to_string().contains("SECRET"));
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[test]
    fn send_retries_a_server_error_once_more() {
        let (url, seen) = serve(vec![(503, "busy"), (200, r#"{"errcode":0}"#)]);
        let cfg = NotifyConfig { url, ..config(Channel::Dingtalk) };
        send(&cfg, &message()).unwrap();
        assert_eq!(seen.lock().unwrap().len(), 2);
    }

    #[test]
    fn an_unreachable_server_is_a_network_error_that_hides_the_url() {
        // Nothing listens on this port.
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let cfg = NotifyConfig { url: format!("http://127.0.0.1:{port}/robot/send?access_token=SECRET"), ..config(Channel::Generic) };
        let error = send(&cfg, &message()).unwrap_err();
        assert!(matches!(error, NotifyError::Network(_)), "{error}");
        assert!(!error.to_string().contains("SECRET"));
    }

    #[test]
    fn the_loop_done_notice_is_on_by_default_including_for_settings_saved_before_it_existed() {
        assert!(NotifyConfig::default().on_loop_done);
        let old: NotifyConfig = serde_json::from_str(r#"{"enabled":true,"channel":"feishu","url":"https://x.test/h"}"#).unwrap();
        assert!(old.enabled && old.on_loop_done);
        let off: NotifyConfig = serde_json::from_str(r#"{"onLoopDone":false}"#).unwrap();
        assert!(!off.on_loop_done);
        let round_trip: NotifyConfig = serde_json::from_str(&serde_json::to_string(&off).unwrap()).unwrap();
        assert_eq!(round_trip, off);
    }
}
