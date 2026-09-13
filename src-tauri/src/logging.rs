//! 极简日志设施（零额外依赖）。
//!
//! 为什么不用 `tracing` / `log`：项目里虽然传递依赖了它们，但没有引
//! subscriber 就不会有任何输出；再引 `tracing-subscriber` 又要新增依赖。
//! 本地单用户应用只需要「带时间戳、可分级、写 stderr」这三件事，
//! 因此这里用一个几十行的实现，够用且不增加构建负担。
//!
//! 级别由环境变量 `WIKIYA_LOG` 控制：`off|error|warn|info|debug|trace`。
//! 未设置时：debug 构建默认 `debug`（便于排查），release 构建默认 `info`。

use std::fmt;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

use regex::Regex;

/// 日志级别（数值越大越啰嗦）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Off = 0,
    Error = 1,
    Warn = 2,
    Info = 3,
    Debug = 4,
    Trace = 5,
}

impl Level {
    fn tag(self) -> &'static str {
        match self {
            Level::Off => "OFF",
            Level::Error => "ERROR",
            Level::Warn => "WARN",
            Level::Info => "INFO",
            Level::Debug => "DEBUG",
            Level::Trace => "TRACE",
        }
    }

    fn parse(raw: &str) -> Option<Level> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "off" => Some(Level::Off),
            "error" => Some(Level::Error),
            "warn" | "warning" => Some(Level::Warn),
            "info" => Some(Level::Info),
            "debug" => Some(Level::Debug),
            "trace" => Some(Level::Trace),
            _ => None,
        }
    }
}

/// 默认级别：debug 构建更啰嗦，release 更克制。
const DEFAULT_LEVEL: u8 = if cfg!(debug_assertions) { 4 } else { 3 };

static MAX_LEVEL: AtomicU8 = AtomicU8::new(DEFAULT_LEVEL);

/// 从环境变量 `WIKIYA_LOG` 初始化级别；非法值回退默认并提示。
/// 幂等，可在进程启动时调用一次。
pub fn init_from_env() {
    match std::env::var("WIKIYA_LOG") {
        Ok(raw) if !raw.trim().is_empty() => match Level::parse(&raw) {
            Some(level) => {
                set_max_level(level);
                log(
                    Level::Info,
                    format_args!("日志级别设为 {}（来自 WIKIYA_LOG）", level.tag()),
                );
            }
            None => log(
                Level::Warn,
                format_args!(
                    "WIKIYA_LOG=`{raw}` 非法（期望 off|error|warn|info|debug|trace），沿用默认级别 {}",
                    current_level().tag()
                ),
            ),
        },
        _ => set_max_level(level_from_u8(DEFAULT_LEVEL)),
    }
}

/// 设置全局最高级别。
pub fn set_max_level(level: Level) {
    MAX_LEVEL.store(level as u8, Ordering::Relaxed);
}

/// 当前最高级别。
pub fn current_level() -> Level {
    level_from_u8(MAX_LEVEL.load(Ordering::Relaxed))
}

fn level_from_u8(value: u8) -> Level {
    match value {
        0 => Level::Off,
        1 => Level::Error,
        2 => Level::Warn,
        3 => Level::Info,
        4 => Level::Debug,
        _ => Level::Trace,
    }
}

/// 判断某级别是否会被输出（供调用方跳过昂贵的字符串拼接）。
pub fn enabled(level: Level) -> bool {
    (level as u8) <= MAX_LEVEL.load(Ordering::Relaxed)
}

/// 真正写日志。格式：`2026-09-13 16:42:36.346 [DEBUG] 正文`。
///
/// 所有日志都会先过 [`redact`]：即使调用方误把密钥/Token 拼进日志，
/// 也不会泄露到终端或日志文件（SEC-003）。
pub fn log(level: Level, args: fmt::Arguments) {
    if !enabled(level) {
        return;
    }
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    let message = redact(&args.to_string());
    eprintln!("{ts} [{}] {message}", level.tag());
}

/// 密钥脱敏（SEC-003）：屏蔽 `Bearer xxx`、`sk-xxx` 以及
/// `api_key` / `token` / `secret` / `password` 等敏感字段的值。
///
/// 允许保留「是否存在」的信息（如 `api_key_set=true`），但绝不保留值本身。
pub fn redact(text: &str) -> String {
    /// 规则集（进程内编译一次）。
    fn rules() -> &'static [Regex; 3] {
        static RULES: OnceLock<[Regex; 3]> = OnceLock::new();
        RULES.get_or_init(|| {
            [
                // Bearer <token>
                Regex::new(r"(?i)\b(bearer)\s+[A-Za-z0-9._\-]+").expect("bearer 规则"),
                // 常见密钥前缀（OpenAI / 多数兼容端点）
                Regex::new(r"\bsk-[A-Za-z0-9_\-]{6,}").expect("sk 规则"),
                // 敏感字段赋值：api_key / apikey / token / secret / password
                Regex::new(
                    r#"(?i)(api[_-]?key|apikey|token|secret|password)(\s*["']?\s*[:=]\s*["']?)([^\s"',}]{4,})"#,
                )
                .expect("字段规则"),
            ]
        })
    }

    let rules = rules();
    let out = rules[0].replace_all(text, "$1 ***");
    let out = rules[1].replace_all(&out, "sk-***");
    rules[2].replace_all(&out, "$1$2***").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_bearer_tokens_and_openai_keys() {
        assert_eq!(redact("Authorization: Bearer abc.def-123_XYZ"), "Authorization: Bearer ***");
        assert_eq!(redact("key=sk-abcdef123456"), "key=sk-***");
    }

    #[test]
    fn redacts_sensitive_field_values_but_keeps_presence() {
        let out = redact(r#"{"api_key":"sk-live-999","api_key_set":true}"#);
        assert!(!out.contains("sk-live-999"));
        assert!(out.contains("api_key_set"));
        assert!(redact("token=abcdefgh password=hunter22").contains("***"));
    }

    #[test]
    fn keeps_ordinary_text_intact() {
        assert_eq!(redact("model=gpt-4o-mini max_tokens=8192"), "model=gpt-4o-mini max_tokens=8192");
    }
}

/// 把长文本截断（取头部），用于日志里回显响应体。
pub fn clip(text: &str, limit: usize) -> String {
    let mut out: String = text.chars().take(limit).collect();
    if text.chars().count() > limit {
        out.push_str("…（已截断）");
    }
    out
}

/// 取文本**末尾**若干字符（推理模型的最终答案常在结尾），超长时前部省略。
pub fn clip_tail(text: &str, limit: usize) -> String {
    let count = text.chars().count();
    if count <= limit {
        return text.to_string();
    }
    let mut out = String::from("…（前部省略）");
    out.extend(text.chars().skip(count - limit));
    out
}

#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {
        $crate::logging::log($crate::logging::Level::Error, format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => {
        $crate::logging::log($crate::logging::Level::Warn, format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {
        $crate::logging::log($crate::logging::Level::Info, format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_debug {
    ($($arg:tt)*) => {
        $crate::logging::log($crate::logging::Level::Debug, format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_trace {
    ($($arg:tt)*) => {
        $crate::logging::log($crate::logging::Level::Trace, format_args!($($arg)*))
    };
}
