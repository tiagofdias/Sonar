//! Decrypted conversation state. Behaviour mirrors the web client's
//! lib/history.js, lib/reactions.js and lib/messageContent.js.
use crate::api::MessageEnvelope;
use crate::crypto::{self, Envelope};
use crate::message_content::{self, Content};
use p256::SecretKey;
use std::collections::BTreeMap;

pub const REACTION_EMOJI: [&str; 4] = ["\u{1f44d}", "\u{2764}\u{fe0f}", "\u{1f602}", "\u{1f389}"];

#[derive(Clone, Debug)]
pub struct Reaction { pub target: String, pub emoji: String, pub action: String }

#[derive(Clone, Debug)]
pub struct Message {
    pub id: String,
    pub sender: String,
    pub receiver: String,
    pub channel: Option<String>,
    pub created_at: String,
    pub created: i64,
    pub expires: Option<i64>,
    pub pinned: bool,
    pub text: String,
    pub reply_to: Option<String>,
    pub reaction: Option<Reaction>,
    pub missed_call: bool,
    pub file: Option<message_content::FileMeta>,
    pub unreadable: bool,
    pub edited: bool,
    pub mentions: Vec<String>,
}

pub fn timestamp(value: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value).ok().map(|date| date.timestamp_millis())
}
pub fn now() -> i64 { chrono::Utc::now().timestamp_millis() }

impl Message {
    pub fn live(&self, now: i64) -> bool { self.pinned || self.expires.is_some_and(|expires| expires > now) }
    /// "14:05" like toLocaleTimeString([], {hour: "2-digit", minute: "2-digit"}).
    pub fn time(&self) -> String {
        chrono::DateTime::from_timestamp_millis(self.created).map(|date| date.with_timezone(&chrono::Local).format("%H:%M").to_string()).unwrap_or_default()
    }
    /// "Nov 7, 14:05" like the community view's toLocaleString.
    pub fn date_time(&self) -> String {
        chrono::DateTime::from_timestamp_millis(self.created).map(|date| date.with_timezone(&chrono::Local).format("%b %-d, %H:%M").to_string()).unwrap_or_default()
    }
}

/// Decrypts one envelope. `peer_key` is the other participant's public key:
/// for direct messages the contact, for channels the sender.
pub fn open(key: &SecretKey, peer_key: Option<&str>, envelope: &MessageEnvelope, trusted: bool) -> Message {
    let mut message = Message {
        id: envelope.id.clone(),
        sender: envelope.sender_id.clone(),
        receiver: envelope.receiver_id.clone(),
        channel: envelope.channel_id.clone(),
        created_at: envelope.created_at.clone(),
        created: timestamp(&envelope.created_at).unwrap_or_default(),
        expires: envelope.expires_at.as_deref().and_then(timestamp),
        pinned: envelope.pinned,
        text: String::new(),
        reply_to: None,
        reaction: None,
        missed_call: false, file: None,
        unreadable: false,
        edited: envelope.edited_at.is_some(),
        mentions: Vec::new(),
    };
    let plain = (|| {
        if !trusted { return None; }
        let context = crypto::context(&envelope.sender_id, &envelope.receiver_id, envelope.channel_id.as_deref()).ok()?;
        crypto::decrypt(key, peer_key?, &Envelope { iv: envelope.iv.clone(), ciphertext: envelope.ciphertext.clone() }, &context).ok()
    })();
    if let Some(value) = &plain { message.mentions = message_content::mentions(value); }
    match plain.map(|plain| message_content::decode(&plain)) {
        Some(Content::Text { text, reply_to }) => { message.text = text; message.reply_to = reply_to; }
        Some(Content::Reaction { target, emoji, action }) => message.reaction = Some(Reaction { target, emoji, action }),
        Some(Content::File(file)) => { message.text = file.label(); message.file = Some(file); }
        Some(Content::MissedCall) => { message.text = message_content::MISSED_CALL_TEXT.into(); message.missed_call = true; }
        None => { message.text = "Unable to decrypt on this device.".into(); message.unreadable = true; }
    }
    message
}

/// Merges by id and keeps the web's stable (createdAt, _id) order.
pub fn merge(list: &mut Vec<Message>, items: impl IntoIterator<Item = Message>) {
    for item in items {
        match list.iter_mut().find(|existing| existing.id == item.id) {
            Some(existing) => *existing = item,
            None => list.push(item),
        }
    }
    list.sort_by(|a, b| a.created.cmp(&b.created).then_with(|| a.id.cmp(&b.id)));
}

/// target -> emoji -> senders, reduced like reactionSummary().
pub fn reactions(list: &[Message], now: i64) -> BTreeMap<String, Vec<(String, Vec<String>)>> {
    let mut choices: Vec<(String, String, String, String)> = Vec::new();
    for message in list {
        let Some(reaction) = &message.reaction else { continue };
        if !message.live(now) { continue; }
        let key = (reaction.target.clone(), message.sender.clone(), reaction.emoji.clone());
        choices.retain(|(target, sender, emoji, _)| (target, sender, emoji) != (&key.0, &key.1, &key.2));
        choices.push((key.0, key.1, key.2, reaction.action.clone()));
    }
    let mut result: BTreeMap<String, Vec<(String, Vec<String>)>> = BTreeMap::new();
    for (target, sender, emoji, action) in choices {
        if action != "add" || !REACTION_EMOJI.contains(&emoji.as_str()) { continue; }
        let entry = result.entry(target).or_default();
        match entry.iter_mut().find(|(value, _)| *value == emoji) {
            Some((_, senders)) => senders.push(sender),
            None => entry.push((emoji, vec![sender])),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn message(id: &str, sender: &str, created: i64, reaction: Option<(&str, &str, &str)>) -> Message {
        Message { id: id.into(), sender: sender.into(), receiver: "r".into(), channel: None, created_at: String::new(), created, expires: Some(i64::MAX), pinned: false, text: String::new(), reply_to: None,
            reaction: reaction.map(|(target, emoji, action)| Reaction { target: target.into(), emoji: emoji.into(), action: action.into() }), missed_call: false, file: None, unreadable: false, edited: false, mentions: Vec::new() }
    }
    #[test]
    fn merge_orders_and_replaces_like_the_web() {
        let mut list = vec![message("b", "x", 2, None)];
        merge(&mut list, [message("a", "x", 2, None), message("c", "x", 1, None)]);
        assert_eq!(list.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["c", "a", "b"]);
        let mut pinned = message("a", "x", 2, None); pinned.pinned = true;
        merge(&mut list, [pinned]);
        assert_eq!(list.len(), 3);
        assert!(list[1].pinned);
    }
    #[test]
    fn reactions_use_the_last_event_per_sender_and_emoji() {
        let like = REACTION_EMOJI[0];
        let list = vec![
            message("1", "alice", 1, Some(("t", like, "add"))),
            message("2", "bob", 2, Some(("t", like, "add"))),
            message("3", "alice", 3, Some(("t", like, "remove"))),
        ];
        let summary = reactions(&list, 0);
        assert_eq!(summary["t"], vec![(like.to_string(), vec!["bob".to_string()])]);
    }
}
