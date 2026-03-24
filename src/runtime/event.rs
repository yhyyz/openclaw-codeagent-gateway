//! ACP session/update event transformation.

use serde_json::Value;

#[derive(Debug, Clone)]
pub enum AgentEvent {
    TextChunk(String),
    ThinkingChunk(String),
    ToolStart { id: String, title: String },
    ToolDone { id: String, title: String, status: String },
    Plan(String),
    PermissionRequest { rpc_id: u64, title: String },
    PromptDone { stop_reason: String },
    UsageUpdate { used: u64, cost_usd: f64 },
    Error(String),
}

impl AgentEvent {
    pub fn from_notification(msg: &Value) -> Option<Self> {
        if msg.get("method")?.as_str()? == "session/request_permission" {
            let id = msg.get("id")?.as_u64()?;
            let title = msg
                .pointer("/params/toolCall/title")?
                .as_str()
                .unwrap_or("")
                .to_string();
            return Some(Self::PermissionRequest { rpc_id: id, title });
        }

        let params = msg.get("params")?;
        let update = params.get("update")?;
        let session_update = update.get("sessionUpdate")?.as_str()?;

        match session_update {
            "agent_message_chunk" => {
                let text = update.pointer("/content/text")?.as_str()?.to_string();
                Some(Self::TextChunk(text))
            }
            "agent_thought_chunk" => {
                let text = update.pointer("/content/text")?.as_str()?.to_string();
                Some(Self::ThinkingChunk(text))
            }
            "tool_call" => Some(Self::ToolStart {
                id: update.get("toolCallId")?.as_str()?.to_string(),
                title: update
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            }),
            "tool_call_update" => {
                let status = update
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let id = update.get("toolCallId")?.as_str()?.to_string();
                let title = update
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if matches!(status, "completed" | "failed") {
                    Some(Self::ToolDone {
                        id,
                        title,
                        status: status.to_string(),
                    })
                } else {
                    Some(Self::ToolStart { id, title })
                }
            }
            "plan" => {
                let entries = update.get("entries")?.as_array()?;
                let text: Vec<&str> = entries
                    .iter()
                    .filter_map(|e| e.get("content")?.as_str())
                    .collect();
                Some(Self::Plan(text.join("; ")))
            }
            "usage_update" => {
                let used = update.get("used").and_then(|v| v.as_u64()).unwrap_or(0);
                let cost_usd = update
                    .pointer("/cost/amount")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                Some(Self::UsageUpdate { used, cost_usd })
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_text_chunk() {
        let msg = serde_json::json!({
            "method": "session/update",
            "params": {
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "content": {"text": "Hello world"}
                }
            }
        });
        match AgentEvent::from_notification(&msg) {
            Some(AgentEvent::TextChunk(t)) => assert_eq!(t, "Hello world"),
            other => panic!("expected TextChunk, got {:?}", other),
        }
    }

    #[test]
    fn parse_thinking_chunk() {
        let msg = serde_json::json!({
            "method": "session/update",
            "params": {
                "update": {
                    "sessionUpdate": "agent_thought_chunk",
                    "content": {"text": "Let me think..."}
                }
            }
        });
        match AgentEvent::from_notification(&msg) {
            Some(AgentEvent::ThinkingChunk(t)) => assert_eq!(t, "Let me think..."),
            other => panic!("expected ThinkingChunk, got {:?}", other),
        }
    }

    #[test]
    fn parse_tool_call() {
        let msg = serde_json::json!({
            "method": "session/update",
            "params": {
                "update": {
                    "sessionUpdate": "tool_call",
                    "toolCallId": "tc-1",
                    "title": "Read file"
                }
            }
        });
        match AgentEvent::from_notification(&msg) {
            Some(AgentEvent::ToolStart { id, title }) => {
                assert_eq!(id, "tc-1");
                assert_eq!(title, "Read file");
            }
            other => panic!("expected ToolStart, got {:?}", other),
        }
    }

    #[test]
    fn parse_tool_call_update_completed() {
        let msg = serde_json::json!({
            "method": "session/update",
            "params": {
                "update": {
                    "sessionUpdate": "tool_call_update",
                    "toolCallId": "tc-2",
                    "title": "Write file",
                    "status": "completed"
                }
            }
        });
        match AgentEvent::from_notification(&msg) {
            Some(AgentEvent::ToolDone { id, title, status }) => {
                assert_eq!(id, "tc-2");
                assert_eq!(title, "Write file");
                assert_eq!(status, "completed");
            }
            other => panic!("expected ToolDone, got {:?}", other),
        }
    }

    #[test]
    fn parse_tool_call_update_in_progress() {
        let msg = serde_json::json!({
            "method": "session/update",
            "params": {
                "update": {
                    "sessionUpdate": "tool_call_update",
                    "toolCallId": "tc-3",
                    "title": "Search",
                    "status": "running"
                }
            }
        });
        match AgentEvent::from_notification(&msg) {
            Some(AgentEvent::ToolStart { id, .. }) => assert_eq!(id, "tc-3"),
            other => panic!("expected ToolStart for in-progress, got {:?}", other),
        }
    }

    #[test]
    fn parse_plan() {
        let msg = serde_json::json!({
            "method": "session/update",
            "params": {
                "update": {
                    "sessionUpdate": "plan",
                    "entries": [
                        {"content": "Step 1"},
                        {"content": "Step 2"}
                    ]
                }
            }
        });
        match AgentEvent::from_notification(&msg) {
            Some(AgentEvent::Plan(text)) => assert_eq!(text, "Step 1; Step 2"),
            other => panic!("expected Plan, got {:?}", other),
        }
    }

    #[test]
    fn parse_permission_request() {
        let msg = serde_json::json!({
            "id": 42,
            "method": "session/request_permission",
            "params": {
                "toolCall": {
                    "title": "Execute bash"
                }
            }
        });
        match AgentEvent::from_notification(&msg) {
            Some(AgentEvent::PermissionRequest { rpc_id, title }) => {
                assert_eq!(rpc_id, 42);
                assert_eq!(title, "Execute bash");
            }
            other => panic!("expected PermissionRequest, got {:?}", other),
        }
    }

    #[test]
    fn unknown_event_returns_none() {
        let msg = serde_json::json!({
            "method": "session/update",
            "params": {
                "update": {
                    "sessionUpdate": "some_future_event"
                }
            }
        });
        assert!(AgentEvent::from_notification(&msg).is_none());
    }

    #[test]
    fn garbage_input_returns_none() {
        let msg = serde_json::json!({"foo": "bar"});
        assert!(AgentEvent::from_notification(&msg).is_none());
    }
}
