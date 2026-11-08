// Parsing happens only after authenticated decryption. Invalid envelopes remain text,
// matching the web client and avoiding a silent loss of legacy messages.
#![allow(dead_code)]
use serde::Deserialize;

const REPLY: &str = "sonar-reply-v1:";
const REACTION: &str = "sonar-reaction-v1:";
const CALL: &str = "sonar-call-v1:";
pub const MISSED_CALL_TEXT: &str = "Missed voice call";

#[derive(Debug, PartialEq)]
pub enum Content {
    Text { text: String, reply_to: Option<String> },
    Reaction { target: String, emoji: String, action: String },
    MissedCall,
}

#[derive(Deserialize)]
struct Reaction { target: String, emoji: String, action: String }

fn valid_id(id: &str) -> bool {
    id.len() == 24 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn encode_text(text: &str, reply_to: Option<&str>) -> Result<String, &'static str> {
    if text.encode_utf16().count() > 4000 { return Err("message too long"); }
    if reply_to.is_some_and(|id| !valid_id(id)) { return Err("invalid reply target"); }
    if reply_to.is_some() || text.starts_with(REPLY) || text.starts_with(REACTION) || text.starts_with(CALL) {
        let mut data = serde_json::json!({ "text": text });
        if let Some(id) = reply_to { data["replyTo"] = id.into(); }
        Ok(format!("{REPLY}{data}"))
    } else { Ok(text.to_owned()) }
}

/// A call that rang and was never answered, sent by the caller's app.
pub fn encode_missed_call() -> String { format!("{CALL}{}", serde_json::json!({ "event": "missed" })) }

pub fn encode_reaction(target: &str, emoji: &str, action: &str) -> Result<String, &'static str> {
    if !valid_id(target) || !["\u{1f44d}", "\u{2764}\u{fe0f}", "\u{1f602}", "\u{1f389}"].contains(&emoji) || !["add", "remove"].contains(&action) {
        return Err("invalid reaction");
    }
    Ok(format!("{REACTION}{}", serde_json::json!({ "target": target, "emoji": emoji, "action": action })))
}

pub fn decode(value: &str) -> Content {
    if let Some(payload) = value.strip_prefix(CALL) {
        if serde_json::from_str::<serde_json::Value>(payload).is_ok_and(|data| data.get("event").and_then(|e| e.as_str()) == Some("missed")) { return Content::MissedCall; }
    } else if let Some(payload) = value.strip_prefix(REACTION) {
        if let Ok(reaction) = serde_json::from_str::<Reaction>(payload) {
            if valid_id(&reaction.target)
                && ["\u{1f44d}", "\u{2764}\u{fe0f}", "\u{1f602}", "\u{1f389}"].contains(&reaction.emoji.as_str())
                && ["add", "remove"].contains(&reaction.action.as_str()) {
                return Content::Reaction { target: reaction.target, emoji: reaction.emoji, action: reaction.action };
            }
        }
    } else if let Some(payload) = value.strip_prefix(REPLY) {
        if let Ok(reply) = serde_json::from_str::<serde_json::Value>(payload) {
            let text = reply.get("text").and_then(|value| value.as_str());
            let target = reply.get("replyTo");
            if let Some(text) = text.filter(|text| text.encode_utf16().count() <= 4000) {
                if target.is_none() || target.and_then(|value| value.as_str()).is_some_and(valid_id) {
                    return Content::Text { text: text.to_owned(), reply_to: target.and_then(|value| value.as_str()).map(str::to_owned) };
                }
            }
        }
    }
    Content::Text { text: value.to_owned(), reply_to: None }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decodes_web_message_content_and_preserves_invalid_payloads() {
        let id = "0123456789abcdef01234567";
        let reply = format!(r#"sonar-reply-v1:{{"text":"Hello","replyTo":"{id}"}}"#);
        assert_eq!(decode(&reply), Content::Text { text: "Hello".into(), reply_to: Some(id.into()) });
        let reaction = format!("sonar-reaction-v1:{{\"target\":\"{id}\",\"emoji\":\"\u{1f44d}\",\"action\":\"add\"}}");
        assert_eq!(decode(&reaction), Content::Reaction { target: id.into(), emoji: "\u{1f44d}".into(), action: "add".into() });
        for invalid in ["sonar-reply-v1:{", "sonar-reply-v1:{\"text\":\"hi\",\"replyTo\":\"bad\"}", "sonar-reaction-v1:{\"target\":\"bad\",\"emoji\":\"x\",\"action\":\"add\"}"] {
            assert_eq!(decode(invalid), Content::Text { text: invalid.into(), reply_to: None });
        }
        let oversized = format!("sonar-reply-v1:{{\"text\":\"{}\"}}", "\u{1f44d}".repeat(2001));
        assert_eq!(decode(&oversized), Content::Text { text: oversized.clone(), reply_to: None });
    }
    #[test]
    fn missed_calls_round_trip_and_cannot_be_typed_as_plain_text() {
        assert_eq!(decode(&encode_missed_call()), Content::MissedCall);
        for invalid in ["sonar-call-v1:{", "sonar-call-v1:{\"event\":\"other\"}"] { assert_eq!(decode(invalid), Content::Text { text: invalid.into(), reply_to: None }); }
        let typed = encode_missed_call();
        assert_eq!(decode(&encode_text(&typed, None).unwrap()), Content::Text { text: typed.clone(), reply_to: None });
    }
    #[test]
    fn encodes_replies_reactions_and_reserved_prefixes() {
        let id = "0123456789abcdef01234567";
        assert_eq!(encode_text("hello", None).unwrap(), "hello");
        assert_eq!(decode(&encode_text("hello", Some(id)).unwrap()), Content::Text { text: "hello".into(), reply_to: Some(id.into()) });
        let reserved = "sonar-reaction-v1:literal";
        assert_eq!(decode(&encode_text(reserved, None).unwrap()), Content::Text { text: reserved.into(), reply_to: None });
        assert_eq!(decode(&encode_reaction(id, "\u{1f44d}", "remove").unwrap()), Content::Reaction { target: id.into(), emoji: "\u{1f44d}".into(), action: "remove".into() });
        assert!(encode_text("text", Some("invalid")).is_err());
        assert!(encode_text(&"\u{1f44d}".repeat(2001), None).is_err());
        assert!(encode_reaction(id, "x", "add").is_err());
    }
}
