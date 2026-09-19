//! 消息体文本处理（契约 §5）。
//!
//! 只作用于**消息体内容**。`## Metadata` / `## Conversation` / 角色头 /
//! `#### 🤔 Thought Process` / `#### 💡 Response` 都是转换器自己产出的结构标题，
//! 不要拿它们过 [`strip_hashes`]。

use std::borrow::Cow;

/// `^#{1,6}\s+(.+)$`（逐行）→ `**$1**`：不保留井号标题，但保留强调。
///
/// 1. **代码围栏内不动**（``` / ~~~），否则会把 Python / Shell 的 `# 注释` 误改成加粗。
/// 2. **整条标题加粗**：标题内原有的 `**` 会被吸收。否则外层 `**` 与内层 `**` 同级交错，
///    CommonMark 会错配定界符，导致整条标题强调不全、甚至残留可见的 `**`。
///    但行内代码（`` ` ``）里的 `**` 不是强调（如 glob `**/*.js`），必须原样保留。
pub fn strip_hashes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut fence: Option<&'static str> = None;

    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            out.push('\n');
        }

        let trimmed = line.trim_start();
        let marker = if trimmed.starts_with("```") {
            Some("```")
        } else if trimmed.starts_with("~~~") {
            Some("~~~")
        } else {
            None
        };

        match fence {
            Some(open) => {
                out.push_str(line);
                if marker == Some(open) {
                    fence = None;
                }
            }
            None => {
                if let Some(open) = marker {
                    fence = Some(open);
                    out.push_str(line);
                } else {
                    out.push_str(&strip_hashes_line(line));
                }
            }
        }
    }

    out
}

fn strip_hashes_line(line: &str) -> Cow<'_, str> {
    let hashes = line.bytes().take_while(|byte| *byte == b'#').count();
    if hashes == 0 || hashes > 6 {
        return Cow::Borrowed(line);
    }

    let rest = &line[hashes..];
    let trimmed = rest.trim_start_matches(char::is_whitespace);
    if trimmed.len() == rest.len() || trimmed.is_empty() {
        return Cow::Borrowed(line);
    }

    // 吸收标题内的 `**`，使整条标题落在一个加粗里（行内代码段除外）
    let merged = remove_bold_outside_code(trimmed);
    let inner = merged.trim();
    if inner.is_empty() {
        return Cow::Borrowed(line);
    }

    Cow::Owned(format!("**{inner}**"))
}

/// 去掉不在行内代码段里的 `**`。
///
/// 行内代码由反引号界定（CommonMark：N 个反引号开始、同样 N 个结束），
/// 其中的 `**` 属于代码内容（如 glob `**/*.js`），原样保留。
pub fn remove_bold_outside_code(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    // 当前代码段的定界反引号个数；0 表示不在代码段内
    let mut open_ticks = 0;

    while index < bytes.len() {
        if bytes[index] == b'`' {
            let start = index;
            while index < bytes.len() && bytes[index] == b'`' {
                index += 1;
            }
            let run = index - start;
            if open_ticks == 0 {
                open_ticks = run;
            } else if open_ticks == run {
                open_ticks = 0;
            }
            out.push_str(&text[start..index]);
        } else if open_ticks == 0 && bytes[index] == b'*' && bytes.get(index + 1) == Some(&b'*') {
            index += 2;
        } else {
            let ch = text[index..]
                .chars()
                .next()
                .expect("index is on a char boundary");
            out.push(ch);
            index += ch.len_utf8();
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_heading_levels() {
        assert_eq!(strip_hashes("# Title"), "**Title**");
        assert_eq!(strip_hashes("### Deep"), "**Deep**");
        assert_eq!(strip_hashes("###### Six"), "**Six**");
    }

    #[test]
    fn leaves_non_headings_alone() {
        assert_eq!(strip_hashes("####### 七个井号"), "####### 七个井号");
        assert_eq!(strip_hashes("#紧贴文字"), "#紧贴文字");
        assert_eq!(strip_hashes("# "), "# ");
        assert_eq!(strip_hashes("##"), "##");
    }

    #[test]
    fn absorbs_inner_bold() {
        assert_eq!(strip_hashes("# 1. **重点**"), "**1. 重点**");
        assert_eq!(
            strip_hashes("## 方案一：**“水珠”——像吃水果**"),
            "**方案一：“水珠”——像吃水果**"
        );
        assert_eq!(strip_hashes("# 🌅 **早晨**"), "**🌅 早晨**");
        assert_eq!(strip_hashes("# 1. **A** 2. **B**"), "**1. A 2. B**");
        assert_eq!(strip_hashes("# **Bold**"), "**Bold**");
    }

    #[test]
    fn keeps_single_star_italics() {
        assert_eq!(strip_hashes("# a *b* c"), "**a *b* c**");
    }

    #[test]
    fn keeps_bold_inside_inline_code() {
        assert_eq!(
            strip_hashes("# 匹配 `**/*.js` 的路径"),
            "**匹配 `**/*.js` 的路径**"
        );
    }

    #[test]
    fn keeps_fenced_code_untouched() {
        let input = "```python\n# 注释\n```";
        assert_eq!(strip_hashes(input), input);
        let tilde = "~~~\n# 注释\n~~~";
        assert_eq!(strip_hashes(tilde), tilde);
    }

    #[test]
    fn keeps_indented_code_untouched() {
        assert_eq!(strip_hashes("    # 缩进四格"), "    # 缩进四格");
    }

    #[test]
    fn keeps_inline_code_untouched() {
        assert_eq!(strip_hashes("`# not a heading`"), "`# not a heading`");
    }

    #[test]
    fn whole_heading_is_one_bold_run() {
        for input in [
            "# Title",
            "## 方案一：**“水珠”——像吃水果**",
            "# 1. **A** 2. **B**",
            "# 🌅 **早晨**",
            "# 匹配 `**/*.js` 的路径",
        ] {
            let out = strip_hashes(input);
            assert!(out.starts_with("**") && out.ends_with("**"), "{out}");
            let inner = &out[2..out.len() - 2];
            let without_code: String = inner.split('`').step_by(2).collect::<Vec<_>>().join("");
            assert!(!without_code.contains("**"), "{out}");
        }
    }
}
