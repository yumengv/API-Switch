//! SSE 流式处理公共设施
//!
//! 把多处散落的 SSE 处理基础代码收敛到一处：
//! - UTF-8 跨 chunk 切分安全拼接（`append_utf8_safe`）
//! - SSE `data:` 前缀解析（`sse_data_payload`）
//!
//! 在阶段 1 之前，这些逻辑在 4 处各写一份（其中 3 处用 `from_utf8_lossy`
//! 导致多字节字符被切分时变成 `�`）。本模块把它们收敛到一处，所有入口
//! 和转发层共用，消除 P3（UTF-8 切字）和重复代码。
//!
//! 参考公理：我们是中转和翻译器，字节流在传输过程中不应被污染。

/// 把 `bytes` 追加到 `buffer`（UTF-8 字符串），保证多字节字符不被切坏。
///
/// 原理：
/// - `remainder` 存放上次调用留下的"不完整 UTF-8 字节"
/// - 新 bytes 来了，先拼到 remainder 后面
/// - 尝试把 remainder 解释为 UTF-8；合法前缀推入 buffer，剩余字节继续留存
/// - 如果剩余字节明确无法构成合法 UTF-8（非"尾部待续"），用 lossy 兜底一次
///
/// 这样跨 chunk 的中文、emoji 等多字节字符能被正确还原而不变成 `�`。
///
/// # 示例
///
/// ```ignore
/// let text = "你好";                    // E4 BD A0 E5 A5 BD
/// let bytes = text.as_bytes();
/// let mut buffer = String::new();
/// let mut remainder = Vec::new();
///
/// // TCP 把"你"的 3 字节 + "好"的第 1 字节切到一起
/// append_utf8_safe(&mut buffer, &mut remainder, &bytes[..4]);
/// assert_eq!(buffer, "你");              // "好"的首字节留在 remainder
///
/// // 第二个 chunk 送来"好"的剩余 2 字节
/// append_utf8_safe(&mut buffer, &mut remainder, &bytes[4..]);
/// assert_eq!(buffer, "你好");            // 完整还原
/// assert!(remainder.is_empty());
/// ```
pub fn append_utf8_safe(buffer: &mut String, remainder: &mut Vec<u8>, bytes: &[u8]) {
    remainder.extend_from_slice(bytes);

    match std::str::from_utf8(remainder) {
        Ok(valid) => {
            buffer.push_str(valid);
            remainder.clear();
        }
        Err(err) => {
            let valid_up_to = err.valid_up_to();
            if valid_up_to > 0 {
                // valid_up_to 由 Utf8Error 保证指向合法 UTF-8 边界
                let valid = std::str::from_utf8(&remainder[..valid_up_to])
                    .expect("valid UTF-8 prefix guaranteed by valid_up_to contract");
                buffer.push_str(valid);
                remainder.drain(..valid_up_to);
            }

            // error_len().is_some() 表示后面有明确的非法序列（不是简单的"尾部截断"）
            // 此时只能 lossy 兜底，但这种情况在合法上游流里很罕见
            if err.error_len().is_some() && !remainder.is_empty() {
                buffer.push_str(&String::from_utf8_lossy(remainder));
                remainder.clear();
            }
        }
    }
}

/// 解析 SSE `data:` 行，返回 payload（不含 "data:" 前缀和可选的空格）。
///
/// - `"data: {json}"` → `Some("{json}")`
/// - `"data:{json}"` → `Some("{json}")`（紧凑形式）
/// - `"event: message"` → `None`（非 data 行）
/// - 空行 / 其他 → `None`
pub fn sse_data_payload(line: &str) -> Option<&str> {
    let payload = line.strip_prefix("data:")?;
    Some(payload.strip_prefix(' ').unwrap_or(payload))
}

// ═══════════════════════════════════════════════════════════════════
//  测试
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_utf8_safe_basic_ascii() {
        let mut buffer = String::new();
        let mut remainder = Vec::new();
        append_utf8_safe(&mut buffer, &mut remainder, b"hello");
        assert_eq!(buffer, "hello");
        assert!(remainder.is_empty());
    }

    #[test]
    fn append_utf8_safe_preserves_split_chinese() {
        let text = "你好";
        let bytes = text.as_bytes();
        let mut buffer = String::new();
        let mut remainder = Vec::new();

        // 在"你"的 3 字节 + "好"的 1 字节处切开（任意非边界位置）
        let split_at = bytes.iter().position(|b| *b >= 0x80).unwrap() + 1;

        append_utf8_safe(&mut buffer, &mut remainder, &bytes[..split_at]);
        assert!(!buffer.contains('\u{FFFD}'), "不应在第一个 chunk 引入替换符");
        assert!(!remainder.is_empty(), "不完整的字节应留在 remainder 中");

        append_utf8_safe(&mut buffer, &mut remainder, &bytes[split_at..]);
        assert_eq!(buffer, text);
        assert!(remainder.is_empty());
    }

    #[test]
    fn append_utf8_safe_preserves_split_emoji() {
        // emoji 是 4 字节 UTF-8，测试切在各种位置
        let text = "🎉abc";
        let bytes = text.as_bytes();
        let mut buffer = String::new();
        let mut remainder = Vec::new();

        // 切在 emoji 的第 2 个字节后（3 字节不完整）
        append_utf8_safe(&mut buffer, &mut remainder, &bytes[..2]);
        assert!(!buffer.contains('\u{FFFD}'));

        append_utf8_safe(&mut buffer, &mut remainder, &bytes[2..]);
        assert_eq!(buffer, text);
    }

    #[test]
    fn append_utf8_safe_handles_many_small_chunks() {
        // 模拟极端情况：每次只送 1 字节
        let text = "你好世界🎉";
        let bytes = text.as_bytes();
        let mut buffer = String::new();
        let mut remainder = Vec::new();

        for byte in bytes {
            append_utf8_safe(&mut buffer, &mut remainder, &[*byte]);
        }

        assert_eq!(buffer, text);
        assert!(remainder.is_empty());
    }

    #[test]
    fn sse_data_payload_with_space() {
        assert_eq!(
            sse_data_payload("data: {\"ok\":true}"),
            Some("{\"ok\":true}")
        );
    }

    #[test]
    fn sse_data_payload_without_space() {
        assert_eq!(
            sse_data_payload("data:{\"ok\":true}"),
            Some("{\"ok\":true}")
        );
    }

    #[test]
    fn sse_data_payload_rejects_other_lines() {
        assert_eq!(sse_data_payload("event: message"), None);
        assert_eq!(sse_data_payload(""), None);
        assert_eq!(sse_data_payload(": comment"), None);
        assert_eq!(sse_data_payload("id: 1"), None);
    }

    #[test]
    fn sse_data_payload_done_marker() {
        assert_eq!(sse_data_payload("data: [DONE]"), Some("[DONE]"));
    }
}
