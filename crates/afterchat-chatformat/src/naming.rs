//! 文件名 / 路径组件清洗（契约 §7）。

/// 删除 Windows 非法字符与换行符（契约 §7：**删除**，不是替换）。
///
/// 非法字符：`\ / : * ? " < > |`，以及 `\r` `\n` `\t`。
/// 结果会 `trim()`，但不做 HTML / Markdown 转义。
pub fn sanitize_filename(name: &str) -> String {
    name.chars()
        .filter(|ch| {
            !matches!(
                ch,
                '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\r' | '\n' | '\t'
            )
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// 契约 §7 的完整规则：删除非法字符 → 截断到 `max_len` 个字符 → 为空则用 `fallback`。
pub fn sanitize_filename_with(name: &str, max_len: usize, fallback: &str) -> String {
    let cleaned = sanitize_filename(name);
    let truncated: String = cleaned.chars().take(max_len).collect();
    let truncated = truncated.trim();
    if truncated.is_empty() {
        fallback.to_string()
    } else {
        truncated.to_string()
    }
}

/// 用作 ZIP 内的子目录名；空 / `.` / `..` 一律兜底。
pub fn sanitize_path_component(name: &str, fallback: &str) -> String {
    let sanitized = sanitize_filename(name);
    if sanitized.is_empty() || sanitized == "." || sanitized == ".." {
        fallback.to_string()
    } else {
        sanitized
    }
}

/// 对齐 AfterChat 用户脚本的 JS `sanitizeFilename`（qwen 需要逐字节对齐）。
///
/// 非法字符 → `_`，控制符 → 空格，连续空白折叠为单个空格，
/// 超长按字符截断后去掉尾部 `\s._-`，空结果 → `untitled`。
pub fn sanitize_filename_js(name: &str, max_len: usize) -> String {
    let mut replaced = String::with_capacity(name.len());
    for ch in name.chars() {
        if matches!(ch, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
            replaced.push('_');
        } else if ch.is_control() {
            replaced.push(' ');
        } else {
            replaced.push(ch);
        }
    }

    let mut collapsed = String::with_capacity(replaced.len());
    let mut prev_whitespace = false;
    for ch in replaced.chars() {
        if ch.is_whitespace() {
            if !prev_whitespace {
                collapsed.push(' ');
                prev_whitespace = true;
            }
        } else {
            collapsed.push(ch);
            prev_whitespace = false;
        }
    }

    let trimmed = collapsed.trim();
    let truncated = if trimmed.chars().count() > max_len {
        let head: String = trimmed.chars().take(max_len).collect();
        head.trim_end_matches(|ch: char| ch.is_whitespace() || matches!(ch, '.' | '_' | '-'))
            .to_string()
    } else {
        trimmed.to_string()
    };

    if truncated.is_empty() {
        "untitled".to_string()
    } else {
        truncated
    }
}

/// 命名风格开关。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NameStyle {
    /// 契约 §7：删除非法字符，兜底 `Untitled_Conversation`。
    #[default]
    Spec,
    /// AfterChat 用户脚本的 JS 规则，兜底 `untitled`（qwen 逐字节对齐用）。
    Js,
}

impl NameStyle {
    /// 按风格清洗标题。
    pub fn sanitize(self, title: &str, max_len: usize) -> String {
        match self {
            NameStyle::Spec => sanitize_filename_with(title, max_len, "Untitled_Conversation"),
            NameStyle::Js => sanitize_filename_js(title, max_len),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_removes_illegal_chars() {
        assert_eq!(sanitize_filename("a/b\\c:d*e?f\"g<h>i|j"), "abcdefghij");
        assert_eq!(sanitize_filename("  keep 中文  "), "keep 中文");
        assert_eq!(sanitize_filename("line\r\nbreak\ttab"), "linebreaktab");
    }

    #[test]
    fn spec_falls_back_when_empty() {
        assert_eq!(
            sanitize_filename_with("///", 60, "Untitled_Conversation"),
            "Untitled_Conversation"
        );
        assert_eq!(
            sanitize_filename_with("  x  ", 60, "Untitled_Conversation"),
            "x"
        );
        assert_eq!(sanitize_filename_with("abcdef", 3, "f"), "abc");
    }

    #[test]
    fn path_component_guards_dots() {
        assert_eq!(sanitize_path_component("..", "Assistant"), "Assistant");
        assert_eq!(sanitize_path_component(".", "Assistant"), "Assistant");
        assert_eq!(sanitize_path_component("  ", "Assistant"), "Assistant");
        assert_eq!(sanitize_path_component("A/B", "Assistant"), "AB");
    }

    #[test]
    fn js_style_matches_reference() {
        assert_eq!(sanitize_filename_js("a/b", 100), "a_b");
        assert_eq!(sanitize_filename_js("  ", 100), "untitled");
        assert_eq!(sanitize_filename_js("a\t\tb", 100), "a b");
    }

    #[test]
    fn js_style_truncates_without_trailing_separators() {
        assert_eq!(sanitize_filename_js("abcdef.", 6), "abcdef");
        assert_eq!(sanitize_filename_js("abc def-", 7), "abc def");
    }
}
