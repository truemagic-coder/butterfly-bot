use crate::error::{ButterflyBotError, Result};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use rust_fsm::*;
use std::collections::HashSet;
#[cfg(unix)]
use std::os::unix::net::UnixStream;

state_machine! {
    ipc_session(Init)

    Init(HandshakeOk) => Established,
    Established(MessageAccepted) => Established,
    Established(Rekeyed) => Established,
    Established(Expired) => Expired,
    Expired(Rekeyed) => Established
}

pub fn validate_session_transition(
    machine: &mut ipc_session::StateMachine,
    input: ipc_session::Input,
) -> Result<()> {
    machine
        .consume(&input)
        .map_err(|_| ButterflyBotError::SecurityPolicy("DENY_INVALID_TRANSITION".to_string()))?;
    Ok(())
}

pub struct ReplayGuard {
    highest_seen: u64,
    seen: HashSet<u64>,
}

impl ReplayGuard {
    pub fn new() -> Self {
        Self {
            highest_seen: 0,
            seen: HashSet::new(),
        }
    }

    pub fn validate_counter(&mut self, counter: u64) -> Result<()> {
        if counter == 0 {
            return Err(ButterflyBotError::SecurityPolicy(
                "DENY_REPLAY: counter must start at 1".to_string(),
            ));
        }

        if self.seen.contains(&counter) {
            return Err(ButterflyBotError::SecurityPolicy(
                "DENY_REPLAY: duplicate counter".to_string(),
            ));
        }

        if counter + 1024 < self.highest_seen {
            return Err(ButterflyBotError::SecurityPolicy(
                "DENY_REPLAY: stale counter".to_string(),
            ));
        }

        self.seen.insert(counter);
        if counter > self.highest_seen {
            self.highest_seen = counter;
        }
        Ok(())
    }
}

impl Default for ReplayGuard {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "linux")]
pub fn enforce_same_user_peer(stream: &UnixStream) -> Result<()> {
    use std::os::fd::AsRawFd;

    let fd = stream.as_raw_fd();
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;

    let rc = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };

    if rc != 0 {
        return Err(ButterflyBotError::SecurityPolicy(
            "DENY_UNAUTHORIZED_IPC_CALLER: peer credential lookup failed".to_string(),
        ));
    }

    let current_uid = unsafe { libc::geteuid() };
    if cred.uid != current_uid {
        return Err(ButterflyBotError::SecurityPolicy(
            "DENY_UNAUTHORIZED_IPC_CALLER: uid mismatch".to_string(),
        ));
    }

    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn enforce_same_user_peer(_stream: &UnixStream) -> Result<()> {
    Ok(())
}

fn nonce_from_counter(counter: u64, direction: u8) -> [u8; 24] {
    let mut nonce = [0u8; 24];
    nonce[0] = direction;
    nonce[8..16].copy_from_slice(&counter.to_be_bytes());
    nonce
}

pub fn encrypt_payload(
    session_key: &[u8; 32],
    counter: u64,
    direction: u8,
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>> {
    if counter == 0 {
        return Err(ButterflyBotError::SecurityPolicy(
            "DENY_REPLAY: counter must start at 1".to_string(),
        ));
    }

    let cipher = XChaCha20Poly1305::new(Key::from_slice(session_key));
    let nonce_bytes = nonce_from_counter(counter, direction);
    let nonce = XNonce::from_slice(&nonce_bytes);

    cipher
        .encrypt(
            nonce,
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| ButterflyBotError::SecurityPolicy("DENY_AEAD_INTEGRITY".to_string()))
}

pub fn decrypt_payload(
    session_key: &[u8; 32],
    replay_guard: &mut ReplayGuard,
    counter: u64,
    direction: u8,
    aad: &[u8],
    ciphertext: &[u8],
) -> Result<Vec<u8>> {
    replay_guard.validate_counter(counter)?;

    let cipher = XChaCha20Poly1305::new(Key::from_slice(session_key));
    let nonce_bytes = nonce_from_counter(counter, direction);
    let nonce = XNonce::from_slice(&nonce_bytes);

    cipher
        .decrypt(
            nonce,
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| ButterflyBotError::SecurityPolicy("DENY_AEAD_INTEGRITY".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::net::UnixStream;

    fn test_key() -> [u8; 32] {
        [7u8; 32]
    }

    #[test]
    fn roundtrip_encrypt_decrypt() {
        let key = test_key();
        let aad = b"v1:session-a:signer:preview:1";
        let plaintext = b"hello-signer";
        let mut guard = ReplayGuard::new();

        let ciphertext = encrypt_payload(&key, 1, 1, aad, plaintext).unwrap();
        let decoded = decrypt_payload(&key, &mut guard, 1, 1, aad, &ciphertext).unwrap();

        assert_eq!(decoded, plaintext);
    }

    #[test]
    fn tamper_fails_integrity() {
        let key = test_key();
        let aad = b"v1:session-a:signer:preview:1";
        let mut guard = ReplayGuard::new();

        let mut ciphertext = encrypt_payload(&key, 1, 1, aad, b"hello").unwrap();
        ciphertext[0] ^= 0xFF;

        let err = decrypt_payload(&key, &mut guard, 1, 1, aad, &ciphertext).unwrap_err();
        assert!(format!("{err}").contains("DENY_AEAD_INTEGRITY"));
    }

    #[test]
    fn replay_counter_is_rejected() {
        let key = test_key();
        let aad = b"v1:session-a:signer:preview:1";
        let mut guard = ReplayGuard::new();

        let ciphertext = encrypt_payload(&key, 2, 1, aad, b"hello").unwrap();
        let _ = decrypt_payload(&key, &mut guard, 2, 1, aad, &ciphertext).unwrap();
        let err = decrypt_payload(&key, &mut guard, 2, 1, aad, &ciphertext).unwrap_err();

        assert!(format!("{err}").contains("DENY_REPLAY"));
    }

    #[cfg(unix)]
    #[test]
    fn peer_identity_check_accepts_same_user() {
        let (left, _right) = UnixStream::pair().unwrap();
        enforce_same_user_peer(&left).unwrap();
    }

    #[test]
    fn session_fsm_enforces_valid_transitions() {
        let mut machine = ipc_session::StateMachine::new();
        validate_session_transition(&mut machine, ipc_session::Input::HandshakeOk).unwrap();
        validate_session_transition(&mut machine, ipc_session::Input::MessageAccepted).unwrap();
        validate_session_transition(&mut machine, ipc_session::Input::Expired).unwrap();

        let err = validate_session_transition(&mut machine, ipc_session::Input::MessageAccepted)
            .unwrap_err();
        assert!(format!("{err}").contains("DENY_INVALID_TRANSITION"));
    }
}
