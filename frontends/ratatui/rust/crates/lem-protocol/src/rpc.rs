//! The JSON-RPC 2.0 envelope.
//!
//! Lem talks to a display half almost entirely in notifications; the one
//! exception is `login`, which is a request and gets a response. See
//! `../../../docs/protocol-notes.md` section 11 for the handshake.

use serde::{Deserialize, Serialize};

/// A message arriving from Lem.
///
/// Untagged rather than keyed on a `type` field, because JSON-RPC
/// distinguishes the two shapes structurally: a response carries `id` and
/// `result`, a notification carries `method`. `Response` is listed first
/// so a message with both keys — which the protocol never produces — is
/// read as the more specific shape.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Incoming {
    Response {
        id: u64,
        #[serde(default)]
        result: serde_json::Value,
    },
    Notification {
        method: String,
        #[serde(default)]
        params: serde_json::Value,
    },
}

#[derive(Debug, Serialize)]
struct OutgoingNotification<'a, T> {
    jsonrpc: &'static str,
    method: &'a str,
    params: T,
}

#[derive(Debug, Serialize)]
struct OutgoingRequest<'a, T> {
    jsonrpc: &'static str,
    id: u64,
    method: &'a str,
    params: T,
}

/// Serialise a notification to send to Lem.
pub fn notification<T: Serialize>(method: &str, params: T) -> serde_json::Result<Vec<u8>> {
    serde_json::to_vec(&OutgoingNotification {
        jsonrpc: "2.0",
        method,
        params,
    })
}

/// Serialise a request to send to Lem.
pub fn request<T: Serialize>(id: u64, method: &str, params: T) -> serde_json::Result<Vec<u8>> {
    serde_json::to_vec(&OutgoingRequest {
        jsonrpc: "2.0",
        id,
        method,
        params,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real login response, copied from the committed capture.
    const CAPTURED_LOGIN_RESPONSE: &str = r#"{"jsonrpc":"2.0","result":{"views":[],"foreground":null,"background":null,"size":{"width":80,"height":24}},"id":1}"#;

    #[test]
    fn decodes_the_captured_login_response() {
        let Incoming::Response { id, result } =
            serde_json::from_str(CAPTURED_LOGIN_RESPONSE).unwrap()
        else {
            panic!("expected a response, not a notification");
        };
        assert_eq!(id, 1);
        assert_eq!(result["size"]["width"], 80);
        assert_eq!(result["size"]["height"], 24);
    }

    #[test]
    fn decodes_a_bulk_notification() {
        let json = r#"{"jsonrpc":"2.0","method":"bulk","params":[]}"#;
        let Incoming::Notification { method, params } = serde_json::from_str(json).unwrap() else {
            panic!("expected a notification");
        };
        assert_eq!(method, "bulk");
        assert!(params.is_array());
    }

    #[test]
    fn a_notification_without_params_still_decodes() {
        // `update-display` is notified with a null argument.
        let json = r#"{"jsonrpc":"2.0","method":"update-display","params":null}"#;
        let Incoming::Notification { method, .. } = serde_json::from_str(json).unwrap() else {
            panic!("expected a notification");
        };
        assert_eq!(method, "update-display");
    }

    #[test]
    fn outgoing_messages_carry_the_jsonrpc_version() {
        let bytes = notification("input", serde_json::json!({"kind": "abort"})).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["jsonrpc"], "2.0");
        assert_eq!(value["method"], "input");
        assert!(value.get("id").is_none(), "a notification has no id");
    }

    #[test]
    fn requests_carry_an_id() {
        let bytes = request(1, "login", serde_json::json!({"size": {}})).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["id"], 1);
        assert_eq!(value["method"], "login");
    }
}
