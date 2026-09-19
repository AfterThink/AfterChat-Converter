//! 时间解析与本地格式化（契约 §3：`YYYY-MM-DD HH:mm:ss ±HH:MM`）。

use chrono::{Datelike, Local, TimeZone, Timelike};
use serde_json::Value;

/// 判定「毫秒时间戳」的下限：秒级时间戳不会大于它。
pub const MILLIS_THRESHOLD: i64 = 10_000_000_000;

/// 解析 epoch 秒 / 毫秒（数字或数字字符串），毫秒会被归一到秒。
pub fn value_to_secs(value: Option<&Value>) -> Option<i64> {
    let raw = match value? {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_f64().map(|f| f as i64))?,
        Value::String(text) => text.trim().parse::<f64>().ok().map(|f| f as i64)?,
        _ => return None,
    };
    Some(normalize_secs(raw))
}

/// 解析 RFC3339（Claude / Cherry 的 `created_at` 等）。
pub fn rfc3339_to_secs(text: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(text.trim())
        .ok()
        .map(|dt| dt.timestamp())
}

/// 数字字符串或 RFC3339 都能解析的宽松版本。
pub fn value_to_secs_any(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::String(text) => rfc3339_to_secs(text).or_else(|| {
            text.trim()
                .parse::<f64>()
                .ok()
                .map(|f| normalize_secs(f as i64))
        }),
        other => value_to_secs(Some(other)),
    }
}

fn normalize_secs(raw: i64) -> i64 {
    if raw >= MILLIS_THRESHOLD {
        raw / 1000
    } else {
        raw
    }
}

/// `2026-09-17 21:19:12 +08:00`
pub fn format_local_time(secs: i64) -> String {
    Local
        .timestamp_opt(secs, 0)
        .single()
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S %:z").to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

/// `20260917-211912`
pub fn format_local_compact(ms: i64) -> String {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|dt| dt.format("%Y%m%d-%H%M%S").to_string())
        .unwrap_or_else(|| "00000000-000000".to_string())
}

/// epoch 秒 → zip DOS 时间（本地时间；DOS 只有 2 秒精度）。
pub fn zip_datetime(secs: i64) -> Option<zip::DateTime> {
    let dt = Local.timestamp_opt(secs, 0).single()?;
    zip::DateTime::from_date_and_time(
        dt.year() as u16,
        dt.month() as u8,
        dt.day() as u8,
        dt.hour() as u8,
        dt.minute() as u8,
        dt.second() as u8,
    )
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalizes_millis() {
        assert_eq!(
            value_to_secs(Some(&json!(1_700_000_000))),
            Some(1_700_000_000)
        );
        assert_eq!(
            value_to_secs(Some(&json!(1_700_000_000_000i64))),
            Some(1_700_000_000)
        );
        assert_eq!(
            value_to_secs(Some(&json!("1700000000"))),
            Some(1_700_000_000)
        );
        assert_eq!(value_to_secs(Some(&json!(null))), None);
    }

    #[test]
    fn parses_rfc3339() {
        assert_eq!(
            rfc3339_to_secs("2024-09-09T14:22:31.424169Z"),
            Some(1_725_891_751)
        );
        assert_eq!(rfc3339_to_secs("not a date"), None);
    }

    #[test]
    fn local_time_matches_contract_shape() {
        let text = format_local_time(1_726_189_351);
        // 形状：YYYY-MM-DD HH:mm:ss ±HH:MM = 26 字符
        assert_eq!(text.len(), 26, "{text}");
        assert_eq!(&text[4..5], "-");
        assert_eq!(&text[7..8], "-");
        assert_eq!(&text[10..11], " ");
        assert_eq!(&text[13..14], ":");
        assert_eq!(&text[16..17], ":");
        assert_eq!(&text[19..20], " ");
        assert!(matches!(text.as_bytes()[20], b'+' | b'-'), "{text}");
        assert!(text[21..23].bytes().all(|b| b.is_ascii_digit()), "{text}");
        assert_eq!(&text[23..24], ":");
        assert!(text[24..26].bytes().all(|b| b.is_ascii_digit()), "{text}");
    }
}
