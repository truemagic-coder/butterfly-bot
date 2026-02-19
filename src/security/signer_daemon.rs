use crate::error::{ButterflyBotError, Result};
use crate::security::policy::{
    default_policy_engine, ensure_policy_allows, PolicyDecision, PolicyEngine, SigningIntent,
};
use rust_fsm::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
#[cfg(unix)]
use std::io::{BufRead, BufReader, Write};
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
#[cfg(unix)]
use std::path::Path;
use std::sync::{Arc, Mutex};
#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_PIPE_CONNECTED, HANDLE, INVALID_HANDLE_VALUE,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
    OPEN_EXISTING,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, PIPE_ACCESS_DUPLEX, PIPE_READMODE_MESSAGE,
    PIPE_TYPE_MESSAGE, PIPE_WAIT,
};

#[cfg(target_os = "windows")]
struct HandleGuard(HANDLE);

#[cfg(target_os = "windows")]
impl Drop for HandleGuard {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

state_machine! {
    signer_flow(Received)

    Received(PolicyChecked) => PolicyEvaluated,
    PolicyEvaluated(AutoApprove) => Approved,
    PolicyEvaluated(RequireApproval) => AwaitUserApproval,
    PolicyEvaluated(Deny) => Denied,
    AwaitUserApproval(Approve) => Approved,
    AwaitUserApproval(Deny) => Denied,
    Approved(Sign) => Signing,
    Signing(Signed) => Approved
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestState {
    Received,
    PolicyEvaluated,
    AwaitUserApproval,
    Approved,
    Signing,
    Denied,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SignerRequest {
    Preview { intent: Box<SigningIntent> },
    Approve { request_id: String },
    Sign { request_id: String },
    Deny { request_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignerResponse {
    pub status: String,
    pub reason_code: String,
    pub signature: Option<String>,
}

#[derive(Clone)]
pub struct SignerService {
    policy: PolicyEngine,
    states: Arc<Mutex<HashMap<String, RequestState>>>,
    machines: Arc<Mutex<HashMap<String, signer_flow::StateMachine>>>,
    intents: Arc<Mutex<HashMap<String, SigningIntent>>>,
}

#[cfg(unix)]
pub fn serve_one_unix_request(socket_path: &Path, service: &SignerService) -> Result<()> {
    if socket_path.exists() {
        let _ = std::fs::remove_file(socket_path);
    }

    let listener = UnixListener::bind(socket_path).map_err(|e| {
        ButterflyBotError::Runtime(format!(
            "failed to bind signer socket {}: {e}",
            socket_path.to_string_lossy()
        ))
    })?;

    let (stream, _) = listener
        .accept()
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
    crate::security::ipc::enforce_same_user_peer(&stream)?;

    let mut reader = BufReader::new(
        stream
            .try_clone()
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?,
    );
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

    let request: SignerRequest = serde_json::from_str(request_line.trim()).map_err(|e| {
        ButterflyBotError::Serialization(format!("failed to parse signer request: {e}"))
    })?;
    let response = service.process(request)?;
    let payload = serde_json::to_string(&response)
        .map_err(|e| ButterflyBotError::Serialization(e.to_string()))?;

    let mut write_stream = stream;
    write_stream
        .write_all(payload.as_bytes())
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
    write_stream
        .write_all(b"\n")
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

    Ok(())
}

#[cfg(unix)]
pub fn send_unix_request(socket_path: &Path, request: &SignerRequest) -> Result<SignerResponse> {
    let mut stream = {
        let mut connected = None;
        for _ in 0..20 {
            match UnixStream::connect(socket_path) {
                Ok(stream) => {
                    connected = Some(stream);
                    break;
                }
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(10)),
            }
        }

        connected.ok_or_else(|| {
            ButterflyBotError::Runtime(format!(
                "failed to connect signer socket {}",
                socket_path.to_string_lossy()
            ))
        })?
    };

    let payload = serde_json::to_string(request)
        .map_err(|e| ButterflyBotError::Serialization(e.to_string()))?;
    stream
        .write_all(payload.as_bytes())
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
    stream
        .write_all(b"\n")
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
    let response: SignerResponse = serde_json::from_str(line.trim())
        .map_err(|e| ButterflyBotError::Serialization(e.to_string()))?;
    Ok(response)
}

#[cfg(target_os = "windows")]
pub fn serve_one_windows_request(pipe_name: &str, service: &SignerService) -> Result<()> {
    let pipe_path = normalize_pipe_name(pipe_name);
    let wide_path = encode_wide_with_nul(&pipe_path);

    let pipe_handle = unsafe {
        CreateNamedPipeW(
            wide_path.as_ptr(),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT,
            1,
            4096,
            4096,
            0,
            std::ptr::null_mut(),
        )
    };

    if pipe_handle == INVALID_HANDLE_VALUE {
        return Err(ButterflyBotError::Runtime(format!(
            "failed to create signer pipe {} ({})",
            pipe_path,
            unsafe { GetLastError() }
        )));
    }

    let connected = unsafe { ConnectNamedPipe(pipe_handle, std::ptr::null_mut()) != 0 };
    if !connected {
        let err = unsafe { GetLastError() };
        if err != ERROR_PIPE_CONNECTED {
            unsafe {
                CloseHandle(pipe_handle);
            }
            return Err(ButterflyBotError::Runtime(format!(
                "failed to accept signer pipe client {} ({})",
                pipe_path, err
            )));
        }
    }

    let handle_guard = HandleGuard(pipe_handle);

    crate::security::ipc::enforce_same_user_named_pipe_client(pipe_handle)?;

    let request_line = read_pipe_line(pipe_handle)?;
    let request: SignerRequest = serde_json::from_str(request_line.trim()).map_err(|e| {
        ButterflyBotError::Serialization(format!("failed to parse signer request: {e}"))
    })?;

    let response = service.process(request)?;
    let payload = serde_json::to_string(&response)
        .map_err(|e| ButterflyBotError::Serialization(e.to_string()))?;

    write_pipe_line(pipe_handle, &payload)?;

    drop(handle_guard);
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn send_windows_request(pipe_name: &str, request: &SignerRequest) -> Result<SignerResponse> {
    let pipe_path = normalize_pipe_name(pipe_name);
    let wide_path = encode_wide_with_nul(&pipe_path);

    let pipe_handle = {
        let mut connected = None;
        for _ in 0..40 {
            let handle = unsafe {
                CreateFileW(
                    wide_path.as_ptr(),
                    FILE_GENERIC_READ | FILE_GENERIC_WRITE,
                    0,
                    std::ptr::null_mut(),
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                    0,
                )
            };

            if handle != INVALID_HANDLE_VALUE {
                connected = Some(handle);
                break;
            }

            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        connected.ok_or_else(|| {
            ButterflyBotError::Runtime(format!("failed to connect signer pipe {}", pipe_path))
        })?
    };

    let handle_guard = HandleGuard(pipe_handle);

    let payload = serde_json::to_string(request)
        .map_err(|e| ButterflyBotError::Serialization(e.to_string()))?;
    write_pipe_line(pipe_handle, &payload)?;
    let line = read_pipe_line(pipe_handle)?;

    drop(handle_guard);

    let response: SignerResponse = serde_json::from_str(line.trim())
        .map_err(|e| ButterflyBotError::Serialization(e.to_string()))?;
    Ok(response)
}

#[cfg(target_os = "windows")]
fn read_pipe_line(pipe_handle: HANDLE) -> Result<String> {
    let mut output = Vec::new();
    let mut chunk = [0u8; 256];

    loop {
        let mut read: u32 = 0;
        let ok = unsafe {
            ReadFile(
                pipe_handle,
                chunk.as_mut_ptr() as *mut _,
                chunk.len() as u32,
                &mut read,
                std::ptr::null_mut(),
            ) != 0
        };

        if !ok {
            let err = unsafe { GetLastError() };
            if output.is_empty() {
                return Err(ButterflyBotError::Runtime(format!(
                    "failed to read signer pipe ({})",
                    err
                )));
            }
            break;
        }

        if read == 0 {
            break;
        }

        output.extend_from_slice(&chunk[..read as usize]);
        if output.last() == Some(&b'\n') {
            break;
        }
    }

    String::from_utf8(output)
        .map_err(|e| ButterflyBotError::Serialization(format!("invalid utf8 pipe payload: {e}")))
}

#[cfg(target_os = "windows")]
fn write_pipe_line(pipe_handle: HANDLE, payload: &str) -> Result<()> {
    let mut wire = payload.as_bytes().to_vec();
    wire.push(b'\n');

    let mut written: u32 = 0;
    let ok = unsafe {
        WriteFile(
            pipe_handle,
            wire.as_ptr() as *const _,
            wire.len() as u32,
            &mut written,
            std::ptr::null_mut(),
        ) != 0
    };

    if !ok || written != wire.len() as u32 {
        return Err(ButterflyBotError::Runtime(format!(
            "failed to write signer pipe ({})",
            unsafe { GetLastError() }
        )));
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn normalize_pipe_name(pipe_name: &str) -> String {
    if pipe_name.starts_with(r"\\.\pipe\") {
        return pipe_name.to_string();
    }
    format!(r"\\.\pipe\{pipe_name}")
}

#[cfg(target_os = "windows")]
fn encode_wide_with_nul(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

impl SignerService {
    pub fn new(policy: PolicyEngine) -> Self {
        Self {
            policy,
            states: Arc::new(Mutex::new(HashMap::new())),
            machines: Arc::new(Mutex::new(HashMap::new())),
            intents: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn process(&self, request: SignerRequest) -> Result<SignerResponse> {
        match request {
            SignerRequest::Preview { intent } => self.preview(*intent),
            SignerRequest::Approve { request_id } => self.approve(&request_id),
            SignerRequest::Sign { request_id } => self.sign(&request_id),
            SignerRequest::Deny { request_id } => self.deny(&request_id),
        }
    }

    fn preview(&self, intent: SigningIntent) -> Result<SignerResponse> {
        let mut effective_intent = intent;
        {
            let intents = self
                .intents
                .lock()
                .map_err(|_| ButterflyBotError::Runtime("intent lock poisoned".to_string()))?;
            if let Some(previous) = intents.get(&effective_intent.request_id) {
                if material_change_requires_reapproval(previous, &effective_intent) {
                    effective_intent.context_requires_approval = true;
                }
            }
        }

        let decision = self.policy.evaluate(&effective_intent, 0);

        let mut machine = signer_flow::StateMachine::new();
        machine
            .consume(&signer_flow::Input::PolicyChecked)
            .map_err(|_| {
                ButterflyBotError::SecurityPolicy("DENY_INVALID_TRANSITION".to_string())
            })?;

        {
            let mut intents = self
                .intents
                .lock()
                .map_err(|_| ButterflyBotError::Runtime("intent lock poisoned".to_string()))?;
            intents.insert(
                effective_intent.request_id.clone(),
                effective_intent.clone(),
            );
        }

        {
            let mut machines = self
                .machines
                .lock()
                .map_err(|_| ButterflyBotError::Runtime("machine lock poisoned".to_string()))?;
            machines.insert(effective_intent.request_id.clone(), machine);
        }

        let mut states = self
            .states
            .lock()
            .map_err(|_| ButterflyBotError::Runtime("state lock poisoned".to_string()))?;

        states.insert(
            effective_intent.request_id.clone(),
            RequestState::PolicyEvaluated,
        );

        match decision {
            PolicyDecision::AutoApproved { reason_code } => {
                self.consume_transition(
                    &effective_intent.request_id,
                    signer_flow::Input::AutoApprove,
                )?;
                states.insert(effective_intent.request_id, RequestState::Approved);
                Ok(SignerResponse {
                    status: "approved".to_string(),
                    reason_code: reason_code.to_string(),
                    signature: None,
                })
            }
            PolicyDecision::NeedsApproval { reason_code } => {
                self.consume_transition(
                    &effective_intent.request_id,
                    signer_flow::Input::RequireApproval,
                )?;
                states.insert(effective_intent.request_id, RequestState::AwaitUserApproval);
                Ok(SignerResponse {
                    status: "await_user_approval".to_string(),
                    reason_code: reason_code.to_string(),
                    signature: None,
                })
            }
            PolicyDecision::Denied { reason_code } => {
                self.consume_transition(&effective_intent.request_id, signer_flow::Input::Deny)?;
                states.insert(effective_intent.request_id, RequestState::Denied);
                ensure_policy_allows(&PolicyDecision::Denied { reason_code })?;
                Ok(SignerResponse {
                    status: "denied".to_string(),
                    reason_code: reason_code.to_string(),
                    signature: None,
                })
            }
        }
    }

    fn approve(&self, request_id: &str) -> Result<SignerResponse> {
        self.consume_transition(request_id, signer_flow::Input::Approve)?;

        let mut states = self
            .states
            .lock()
            .map_err(|_| ButterflyBotError::Runtime("state lock poisoned".to_string()))?;

        states.insert(request_id.to_string(), RequestState::Approved);
        Ok(SignerResponse {
            status: "approved".to_string(),
            reason_code: "ALLOW_USER_INITIATED".to_string(),
            signature: None,
        })
    }

    fn sign(&self, request_id: &str) -> Result<SignerResponse> {
        self.consume_transition(request_id, signer_flow::Input::Sign)?;

        {
            let mut states = self
                .states
                .lock()
                .map_err(|_| ButterflyBotError::Runtime("state lock poisoned".to_string()))?;
            states.insert(request_id.to_string(), RequestState::Signing);
        }

        let signing_intent = {
            let intents = self
                .intents
                .lock()
                .map_err(|_| ButterflyBotError::Runtime("intent lock poisoned".to_string()))?;
            intents.get(request_id).cloned().ok_or_else(|| {
                ButterflyBotError::SecurityPolicy("DENY_INVALID_TRANSITION".to_string())
            })?
        };

        let signature = crate::security::solana_signer::sign_intent(&signing_intent)?;

        self.consume_transition(request_id, signer_flow::Input::Signed)?;

        {
            let mut states = self
                .states
                .lock()
                .map_err(|_| ButterflyBotError::Runtime("state lock poisoned".to_string()))?;
            states.insert(request_id.to_string(), RequestState::Approved);
        }

        Ok(SignerResponse {
            status: "signed".to_string(),
            reason_code: "ALLOW_AUTO_POLICY_OK".to_string(),
            signature: Some(signature),
        })
    }

    fn deny(&self, request_id: &str) -> Result<SignerResponse> {
        let _ = self.consume_transition(request_id, signer_flow::Input::Deny);
        let mut states = self
            .states
            .lock()
            .map_err(|_| ButterflyBotError::Runtime("state lock poisoned".to_string()))?;
        states.insert(request_id.to_string(), RequestState::Denied);
        Ok(SignerResponse {
            status: "denied".to_string(),
            reason_code: "DENY_USER_POLICY".to_string(),
            signature: None,
        })
    }

    fn consume_transition(&self, request_id: &str, input: signer_flow::Input) -> Result<()> {
        let mut machines = self
            .machines
            .lock()
            .map_err(|_| ButterflyBotError::Runtime("machine lock poisoned".to_string()))?;
        let machine = machines.get_mut(request_id).ok_or_else(|| {
            ButterflyBotError::SecurityPolicy("DENY_INVALID_TRANSITION".to_string())
        })?;
        machine.consume(&input).map_err(|_| {
            ButterflyBotError::SecurityPolicy("DENY_INVALID_TRANSITION".to_string())
        })?;
        Ok(())
    }
}

impl Default for SignerService {
    fn default() -> Self {
        Self::new(default_policy_engine())
    }
}

fn material_change_requires_reapproval(previous: &SigningIntent, next: &SigningIntent) -> bool {
    previous.amount_atomic != next.amount_atomic
        || previous.payee != next.payee
        || previous.scheme_id != next.scheme_id
        || previous.chain_id != next.chain_id
        || previous.payment_authority != next.payment_authority
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};
    #[cfg(unix)]
    use std::thread;

    fn test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn setup_signing_env() -> tempfile::TempDir {
        crate::security::tpm_provider::set_tpm_available_for_tests(Some(true));
        crate::security::tpm_provider::set_dek_passphrase_for_tests(Some(
            "signer-daemon-test-dek".to_string(),
        ));
        let temp = tempfile::tempdir().unwrap();
        crate::runtime_paths::set_app_root_override_for_tests(Some(temp.path().to_path_buf()));
        temp
    }

    fn teardown_signing_env() {
        crate::runtime_paths::set_app_root_override_for_tests(None);
        crate::security::tpm_provider::set_dek_passphrase_for_tests(None);
        crate::security::tpm_provider::set_tpm_available_for_tests(None);
    }

    fn intent(request_id: &str) -> SigningIntent {
        SigningIntent {
            request_id: request_id.to_string(),
            actor: "agent".to_string(),
            user_id: "user".to_string(),
            action_type: "x402_payment".to_string(),
            amount_atomic: 100,
            payee: "merchant.local".to_string(),
            context_requires_approval: false,
            scheme_id: Some("v2-solana-exact".to_string()),
            chain_id: Some("solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp".to_string()),
            payment_authority: Some("https://merchant.local".to_string()),
            idempotency_key: Some("idem-1".to_string()),
        }
    }

    #[test]
    fn preview_then_sign_auto_approved() {
        let _guard = test_lock().lock().expect("test lock poisoned");
        let _temp = setup_signing_env();
        let service = SignerService::default();

        let preview = service
            .process(SignerRequest::Preview {
                intent: Box::new(intent("req-1")),
            })
            .unwrap();
        assert_eq!(preview.status, "approved");

        let signed = service
            .process(SignerRequest::Sign {
                request_id: "req-1".to_string(),
            })
            .unwrap();
        assert_eq!(signed.status, "signed");
        assert!(signed.signature.is_some());
        teardown_signing_env();
    }

    #[test]
    fn sign_without_approval_is_denied_transition() {
        let service = SignerService::default();

        let err = service
            .process(SignerRequest::Sign {
                request_id: "missing".to_string(),
            })
            .unwrap_err();
        assert!(format!("{err}").contains("DENY_INVALID_TRANSITION"));
    }

    #[test]
    fn prompt_then_approve_then_sign() {
        let _guard = test_lock().lock().expect("test lock poisoned");
        let _temp = setup_signing_env();
        let service = SignerService::default();
        let mut approval_intent = intent("req-2");
        approval_intent.context_requires_approval = true;

        let preview = service
            .process(SignerRequest::Preview {
                intent: Box::new(approval_intent),
            })
            .unwrap();
        assert_eq!(preview.status, "await_user_approval");

        let approved = service
            .process(SignerRequest::Approve {
                request_id: "req-2".to_string(),
            })
            .unwrap();
        assert_eq!(approved.status, "approved");

        let signed = service
            .process(SignerRequest::Sign {
                request_id: "req-2".to_string(),
            })
            .unwrap();
        assert_eq!(signed.status, "signed");
        teardown_signing_env();
    }

    #[test]
    fn material_change_on_same_request_forces_reapproval() {
        let _guard = test_lock().lock().expect("test lock poisoned");
        let _temp = setup_signing_env();
        let service = SignerService::default();

        let first_preview = service
            .process(SignerRequest::Preview {
                intent: Box::new(intent("req-3")),
            })
            .unwrap();
        assert_eq!(first_preview.status, "approved");

        let mut changed = intent("req-3");
        changed.amount_atomic = 250_000;
        let second_preview = service
            .process(SignerRequest::Preview {
                intent: Box::new(changed),
            })
            .unwrap();
        assert_eq!(second_preview.status, "await_user_approval");
        teardown_signing_env();
    }

    #[cfg(unix)]
    #[test]
    fn unix_socket_preview_roundtrip() {
        let service = SignerService::default();
        let temp = tempfile::tempdir().unwrap();
        let socket_path = temp.path().join("signer.sock");

        let service_for_thread = service.clone();
        let socket_for_thread = socket_path.clone();
        let handle = thread::spawn(move || {
            serve_one_unix_request(&socket_for_thread, &service_for_thread).unwrap();
        });

        let response = send_unix_request(
            &socket_path,
            &SignerRequest::Preview {
                intent: Box::new(intent("req-sock")),
            },
        )
        .unwrap();

        handle.join().unwrap();
        assert!(response.status == "approved" || response.status == "await_user_approval");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_pipe_preview_roundtrip() {
        let service = SignerService::default();
        let pipe_name = format!("butterfly-bot-signer-{}", std::process::id());

        let service_for_thread = service.clone();
        let pipe_for_thread = pipe_name.clone();
        let handle = std::thread::spawn(move || {
            serve_one_windows_request(&pipe_for_thread, &service_for_thread).unwrap();
        });

        let response = send_windows_request(
            &pipe_name,
            &SignerRequest::Preview {
                intent: Box::new(intent("req-pipe")),
            },
        )
        .unwrap();

        handle.join().unwrap();
        assert!(response.status == "approved" || response.status == "await_user_approval");
    }
}
