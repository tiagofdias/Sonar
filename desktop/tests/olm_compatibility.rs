// This is an isolated feasibility test, not an integrated messaging protocol.
// All accounts and messages are synthetic and exist only during the test.
use vodozemac::olm::{Account, OlmMessage, SessionConfig};

#[test]
fn synthetic_devices_establish_a_ratchet_and_reject_replayed_messages() {
    let alice = Account::new();
    let mut bob = Account::new();
    bob.generate_one_time_keys(1);
    let bob_one_time_key = *bob.one_time_keys().values().next().unwrap();
    let mut alice_session = alice.create_outbound_session(
        SessionConfig::version_1(), bob.curve25519_key(), bob_one_time_key,
    ).unwrap();
    bob.mark_keys_as_published();
    let initial = alice_session.encrypt("synthetic first message").unwrap();
    let OlmMessage::PreKey(prekey) = initial else { panic!("initial message must contain a prekey"); };
    let inbound = bob.create_inbound_session(SessionConfig::version_1(), alice.curve25519_key(), &prekey).unwrap();
    assert_eq!(inbound.plaintext, b"synthetic first message");
    let mut bob_session = inbound.session;
    assert_eq!(alice_session.session_id(), bob_session.session_id());
    let response = bob_session.encrypt("synthetic response").unwrap();
    assert_eq!(alice_session.decrypt(&response).unwrap(), b"synthetic response");
    assert!(alice_session.decrypt(&response).is_err());
    let next = alice_session.encrypt("synthetic next message").unwrap();
    assert_eq!(bob_session.decrypt(&next).unwrap(), b"synthetic next message");
    assert!(bob_session.decrypt(&next).is_err());
}
