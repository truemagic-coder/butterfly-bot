use crate::error::{ButterflyBotError, Result};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use rust_fsm::*;
use std::collections::HashSet;
#[cfg(unix)]
use std::os::unix::net::UnixStream;
#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE};
#[cfg(target_os = "windows")]
use windows_sys::Win32::Security::{
    EqualSid, GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::System::Pipes::{ImpersonateNamedPipeClient, RevertToSelf};
#[cfg(target_os = "windows")]
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThread, OpenProcessToken, OpenThreadToken,
};

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

#[cfg(target_os = "macos")]
pub fn enforce_same_user_peer(stream: &UnixStream) -> Result<()> {
    use std::os::fd::AsRawFd;

    let fd = stream.as_raw_fd();
    let mut euid: libc::uid_t = 0;
    let mut egid: libc::gid_t = 0;
    let rc = unsafe { libc::getpeereid(fd, &mut euid as *mut _, &mut egid as *mut _) };

    if rc != 0 {
        return Err(ButterflyBotError::SecurityPolicy(
            "DENY_UNAUTHORIZED_IPC_CALLER: peer credential lookup failed".to_string(),
        ));
    }

    let current_uid = unsafe { libc::geteuid() };
    if euid != current_uid {
        return Err(ButterflyBotError::SecurityPolicy(
            "DENY_UNAUTHORIZED_IPC_CALLER: uid mismatch".to_string(),
        ));
    }

    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn enforce_same_user_peer(_stream: &UnixStream) -> Result<()> {
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn enforce_same_user_named_pipe_client(pipe_handle: HANDLE) -> Result<()> {
    unsafe {
        if ImpersonateNamedPipeClient(pipe_handle) == 0 {
            return Err(ButterflyBotError::SecurityPolicy(
                "DENY_UNAUTHORIZED_IPC_CALLER: client impersonation failed".to_string(),
            ));
        }

        let mut client_token: HANDLE = 0;
        let thread_token_opened =
            OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut client_token) != 0;
        let _ = RevertToSelf();

        if !thread_token_opened {
            return Err(ButterflyBotError::SecurityPolicy(
                "DENY_UNAUTHORIZED_IPC_CALLER: failed to open client token".to_string(),
            ));
        }

        let mut process_token: HANDLE = 0;
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut process_token) == 0 {
            let _ = CloseHandle(client_token);
            return Err(ButterflyBotError::SecurityPolicy(
                "DENY_UNAUTHORIZED_IPC_CALLER: failed to open process token".to_string(),
            ));
        }

        let client_user_buffer = match read_token_user_buffer(client_token) {
            Ok(value) => value,
            Err(err) => {
                let _ = CloseHandle(client_token);
                let _ = CloseHandle(process_token);
                return Err(err);
            }
        };
        let process_user_buffer = match read_token_user_buffer(process_token) {
            Ok(value) => value,
            Err(err) => {
                let _ = CloseHandle(client_token);
                let _ = CloseHandle(process_token);
                return Err(err);
            }
        };

        let _ = CloseHandle(client_token);
        let _ = CloseHandle(process_token);

        let client_user = &*(client_user_buffer.as_ptr() as *const TOKEN_USER);
        let process_user = &*(process_user_buffer.as_ptr() as *const TOKEN_USER);
        let same_sid = EqualSid(client_user.Sid, process_user.Sid) != 0;
        if !same_sid {
            return Err(ButterflyBotError::SecurityPolicy(
                "DENY_UNAUTHORIZED_IPC_CALLER: user sid mismatch".to_string(),
            ));
        }
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn read_token_user_buffer(token: HANDLE) -> Result<Vec<u8>> {
    unsafe {
        let mut required_len: u32 = 0;
        let _ = GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut required_len);
        if required_len == 0 {
            return Err(ButterflyBotError::SecurityPolicy(format!(
                "DENY_UNAUTHORIZED_IPC_CALLER: token size query failed ({})",
                GetLastError()
            )));
        }

        let mut buffer = vec![0u8; required_len as usize];
        let ok = GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr() as *mut _,
            required_len,
            &mut required_len,
        ) != 0;

        if !ok {
            return Err(ButterflyBotError::SecurityPolicy(format!(
                "DENY_UNAUTHORIZED_IPC_CALLER: token info query failed ({})",
                GetLastError()
            )));
        }

        Ok(buffer)
    }
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
