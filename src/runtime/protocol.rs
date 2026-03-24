//! ACP JSON-RPC 2.0 protocol types.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize)]
pub struct RpcRequest {
    pub jsonrpc: &'static str,
    pub id: u64,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl RpcRequest {
    pub fn new(id: u64, method: &str, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            method: method.to_string(),
            params,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct RpcMessage {
    pub id: Option<u64>,
    pub method: Option<String>,
    pub params: Option<Value>,
    pub result: Option<Value>,
    pub error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

pub fn build_initialize(id: u64, version: &str) -> RpcRequest {
    RpcRequest::new(
        id,
        "initialize",
        Some(serde_json::json!({
            "protocolVersion": 1,
            "clientCapabilities": {},
            "clientInfo": {"name": "agent-gateway", "version": version}
        })),
    )
}

pub fn build_session_new(id: u64, cwd: &str) -> RpcRequest {
    RpcRequest::new(
        id,
        "session/new",
        Some(serde_json::json!({
            "cwd": cwd,
            "mcpServers": []
        })),
    )
}

pub fn build_session_load(id: u64, session_id: &str, cwd: &str) -> RpcRequest {
    RpcRequest::new(
        id,
        "session/load",
        Some(serde_json::json!({
            "sessionId": session_id,
            "cwd": cwd,
            "mcpServers": []
        })),
    )
}

pub fn build_prompt(id: u64, session_id: &str, text: &str) -> RpcRequest {
    RpcRequest::new(
        id,
        "session/prompt",
        Some(serde_json::json!({
            "sessionId": session_id,
            "prompt": [{"type": "text", "text": text}]
        })),
    )
}

pub fn build_permission_reply(id: u64) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {"optionId": "allow_always"}
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rpc_request_serializes_correctly() {
        let req = RpcRequest::new(1, "test/method", Some(serde_json::json!({"key": "val"})));
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["jsonrpc"], "2.0");
        assert_eq!(json["id"], 1);
        assert_eq!(json["method"], "test/method");
        assert_eq!(json["params"]["key"], "val");
    }

    #[test]
    fn rpc_request_skips_none_params() {
        let req = RpcRequest::new(1, "test", None);
        let s = serde_json::to_string(&req).unwrap();
        assert!(!s.contains("params"));
    }

    #[test]
    fn initialize_request_structure() {
        let req = build_initialize(1, "0.1.0");
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["method"], "initialize");
        assert_eq!(json["params"]["protocolVersion"], 1);
        assert_eq!(json["params"]["clientInfo"]["name"], "agent-gateway");
        assert_eq!(json["params"]["clientInfo"]["version"], "0.1.0");
    }

    #[test]
    fn session_new_request_structure() {
        let req = build_session_new(2, "/home/user/project");
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["method"], "session/new");
        assert_eq!(json["params"]["cwd"], "/home/user/project");
    }

    #[test]
    fn prompt_request_structure() {
        let req = build_prompt(3, "sess-abc", "hello world");
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["method"], "session/prompt");
        assert_eq!(json["params"]["sessionId"], "sess-abc");
        assert_eq!(json["params"]["prompt"][0]["type"], "text");
        assert_eq!(json["params"]["prompt"][0]["text"], "hello world");
    }

    #[test]
    fn permission_reply_structure() {
        let reply = build_permission_reply(42);
        assert_eq!(reply["jsonrpc"], "2.0");
        assert_eq!(reply["id"], 42);
        assert_eq!(reply["result"]["optionId"], "allow_always");
    }

    #[test]
    fn rpc_message_deserializes_result() {
        let json = r#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#;
        let msg: RpcMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.id, Some(1));
        assert!(msg.result.is_some());
        assert!(msg.error.is_none());
    }

    #[test]
    fn rpc_message_deserializes_error() {
        let json = r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32600,"message":"Invalid Request"}}"#;
        let msg: RpcMessage = serde_json::from_str(json).unwrap();
        let err = msg.error.unwrap();
        assert_eq!(err.code, -32600);
        assert_eq!(err.message, "Invalid Request");
    }

    #[test]
    fn rpc_message_deserializes_notification() {
        let json = r#"{"jsonrpc":"2.0","method":"session/update","params":{"data":"x"}}"#;
        let msg: RpcMessage = serde_json::from_str(json).unwrap();
        assert!(msg.id.is_none());
        assert_eq!(msg.method.as_deref(), Some("session/update"));
    }
}
