//! Channel-agnostic Markdown message formatting.

use std::collections::HashMap;

/// Format a number with comma separators for thousands.
fn format_token_count(count: u64) -> String {
    if count >= 1_000_000 {
        let millions = count / 1_000_000;
        let thousands = (count % 1_000_000) / 1000;
        let ones = count % 1000;
        format!("{},{:03},{:03}", millions, thousands, ones)
    } else if count >= 1000 {
        format!("{},{:03}", count / 1000, count % 1000)
    } else {
        count.to_string()
    }
}

pub fn format_result(
    agent: &str,
    job_id: &str,
    status: &str,
    result: &str,
    error: &str,
    tool_counts: &HashMap<String, usize>,
    duration_secs: f64,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    total_tokens: u64,
    cost_usd: f64,
) -> String {
    let mut parts = Vec::new();

    parts.push(format!(
        "[{}] {}",
        agent,
        &job_id[..job_id.len().min(8)]
    ));

    match status {
        "failed" | "interrupted" => {
            parts.push(format!("❌ {}", error));
        }
        "completed" => {
            if !tool_counts.is_empty() {
                let mut tool_strs: Vec<String> = tool_counts
                    .iter()
                    .map(|(name, count)| {
                        if *count > 1 {
                            format!("{} ×{}", name, count)
                        } else {
                            name.clone()
                        }
                    })
                    .collect();
                tool_strs.sort();
                parts.push(format!("🔧 {}", tool_strs.join(" | ")));
            }
            if !result.is_empty() {
                parts.push(result.to_string());
            }
        }
        _ => {}
    }

    parts.push(format!("⏱ {:.0}s", duration_secs));

    if total_tokens > 0 {
        let mut usage_parts = Vec::new();
        if input_tokens > 0 {
            usage_parts.push(format!("input: {}", format_token_count(input_tokens)));
        }
        if output_tokens > 0 {
            usage_parts.push(format!("output: {}", format_token_count(output_tokens)));
        }
        if cache_read_tokens > 0 {
            usage_parts.push(format!(
                "cache read: {}",
                format_token_count(cache_read_tokens)
            ));
        }
        if cache_write_tokens > 0 {
            usage_parts.push(format!(
                "cache write: {}",
                format_token_count(cache_write_tokens)
            ));
        }
        usage_parts.push(format!(
            "total: {} tokens",
            format_token_count(total_tokens)
        ));
        parts.push(format!("📊 {}", usage_parts.join(" | ")));
    }

    if cost_usd > 0.0 {
        parts.push(format!("💰 ${:.4}", cost_usd));
    }

    parts.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_completed_with_result() {
        let out = format_result(
            "kiro", "abcdefghij", "completed", "Done!", "",
            &HashMap::new(), 1.5, 0, 0, 0, 0, 0, 0.0,
        );
        assert!(out.contains("[kiro]"));
        assert!(out.contains("abcdefgh"));
        assert!(out.contains("Done!"));
        assert!(out.contains("⏱ 2s") || out.contains("⏱ 1s"));
    }

    #[test]
    fn format_failed() {
        let out = format_result(
            "codex", "job12345", "failed", "", "OOM killed",
            &HashMap::new(), 3.0, 0, 0, 0, 0, 0, 0.0,
        );
        assert!(out.contains("❌ OOM killed"));
        assert!(out.contains("⏱ 3s"));
    }

    #[test]
    fn format_with_tools_and_counts() {
        let mut tools = HashMap::new();
        tools.insert("read_file".into(), 2);
        tools.insert("write_file".into(), 1);
        let out = format_result(
            "kiro", "job-abcd", "completed", "ok", "",
            &tools, 2.0, 0, 0, 0, 0, 0, 0.0,
        );
        assert!(out.contains("read_file ×2"));
        assert!(out.contains("write_file"));
        assert!(!out.contains("write_file ×"));
    }

    #[test]
    fn truncates_job_id_to_8_chars() {
        let out = format_result(
            "a", "123456789012", "completed", "", "",
            &HashMap::new(), 0.1, 0, 0, 0, 0, 0, 0.0,
        );
        assert!(out.contains("12345678"));
        assert!(!out.contains("9012"));
    }

    #[test]
    fn short_job_id_not_padded() {
        let out = format_result(
            "a", "abc", "completed", "", "",
            &HashMap::new(), 0.1, 0, 0, 0, 0, 0, 0.0,
        );
        assert!(out.contains("abc"));
    }

    #[test]
    fn no_channel_specific_words() {
        let mut tools = HashMap::new();
        tools.insert("bash".into(), 1);
        let out = format_result(
            "agent", "jobid123", "completed", "result text", "",
            &tools, 5.5, 0, 0, 0, 0, 0, 0.0,
        );
        let lower = out.to_lowercase();
        assert!(!lower.contains("discord"));
        assert!(!lower.contains("telegram"));
        assert!(!lower.contains("whatsapp"));
        assert!(!lower.contains("slack"));
    }

    #[test]
    fn interrupted_shows_error() {
        let out = format_result(
            "a", "job-1234", "interrupted", "", "timed out",
            &HashMap::new(), 30.0, 0, 0, 0, 0, 0, 0.0,
        );
        assert!(out.contains("❌ timed out"));
    }

    #[test]
    fn agent_name_uses_bracket_format() {
        let out = format_result(
            "Claude", "abcd1234", "completed", "hello", "",
            &HashMap::new(), 5.0, 0, 0, 0, 0, 0, 0.0,
        );
        assert!(out.contains("[Claude]"));
        assert!(!out.contains("**Claude**"));
    }

    #[test]
    fn token_usage_displayed_when_present() {
        let out = format_result(
            "claude", "abcd1234", "completed", "done", "",
            &HashMap::new(), 15.0,
            3, 5, 4349, 3711, 8068, 0.025,
        );
        assert!(out.contains("📊"));
        assert!(out.contains("input: 3"));
        assert!(out.contains("output: 5"));
        assert!(out.contains("cache read: 4,349"));
        assert!(out.contains("cache write: 3,711"));
        assert!(out.contains("total: 8,068 tokens"));
        assert!(out.contains("💰 $0.0250"));
    }

    #[test]
    fn no_token_usage_when_zero() {
        let out = format_result(
            "claude", "abcd1234", "completed", "done", "",
            &HashMap::new(), 15.0, 0, 0, 0, 0, 0, 0.0,
        );
        assert!(!out.contains("📊"));
        assert!(!out.contains("💰"));
    }

    #[test]
    fn no_cost_when_zero() {
        let out = format_result(
            "claude", "abcd1234", "completed", "done", "",
            &HashMap::new(), 15.0, 10, 20, 0, 0, 30, 0.0,
        );
        assert!(out.contains("📊"));
        assert!(!out.contains("💰"));
    }

    #[test]
    fn format_token_count_under_1000() {
        assert_eq!(format_token_count(0), "0");
        assert_eq!(format_token_count(5), "5");
        assert_eq!(format_token_count(999), "999");
    }

    #[test]
    fn format_token_count_thousands() {
        assert_eq!(format_token_count(1000), "1,000");
        assert_eq!(format_token_count(4349), "4,349");
        assert_eq!(format_token_count(8068), "8,068");
        assert_eq!(format_token_count(999999), "999,999");
    }

    #[test]
    fn format_token_count_millions() {
        assert_eq!(format_token_count(1000000), "1,000,000");
        assert_eq!(format_token_count(1234567), "1,234,567");
    }

    #[test]
    fn tools_sorted_alphabetically() {
        let mut tools = HashMap::new();
        tools.insert("z_tool".into(), 1);
        tools.insert("a_tool".into(), 3);
        let out = format_result(
            "a", "job-abcd", "completed", "", "",
            &tools, 1.0, 0, 0, 0, 0, 0, 0.0,
        );
        let z_pos = out.find("z_tool").unwrap();
        let a_pos = out.find("a_tool").unwrap();
        assert!(a_pos < z_pos, "tools should be sorted alphabetically");
    }
}
