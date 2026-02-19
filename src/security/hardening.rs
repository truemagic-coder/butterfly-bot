use crate::error::{ButterflyBotError, Result};
use std::fmt;
use zeroize::Zeroize;

#[derive(Debug, Clone)]
pub struct StartupComplianceReport {
    pub strict_profile: bool,
    pub page_locking_ready: bool,
    pub core_dump_protection_ready: bool,
    pub ptrace_protection_ready: bool,
}

impl StartupComplianceReport {
    pub fn is_compliant(&self) -> bool {
        self.strict_profile
            && self.page_locking_ready
            && self.core_dump_protection_ready
            && self.ptrace_protection_ready
    }
}

impl fmt::Display for StartupComplianceReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "strict_profile={}, page_locking_ready={}, core_dump_protection_ready={}, ptrace_protection_ready={}, compliant={}",
            self.strict_profile,
            self.page_locking_ready,
            self.core_dump_protection_ready,
            self.ptrace_protection_ready,
            self.is_compliant()
        )
    }
}

pub struct SensitiveBuffer {
    bytes: Vec<u8>,
    locked: bool,
}

impl SensitiveBuffer {
    pub fn from_vec(bytes: Vec<u8>) -> Result<Self> {
        if !bytes.is_empty() {
            lock_bytes(&bytes)?;
        }
        Ok(Self {
            bytes,
            locked: true,
        })
    }

    pub fn expose<R>(&self, f: impl FnOnce(&[u8]) -> R) -> R {
        f(&self.bytes)
    }
}

impl Drop for SensitiveBuffer {
    fn drop(&mut self) {
        if self.locked && !self.bytes.is_empty() {
            let _ = unlock_bytes(&self.bytes);
        }
        self.bytes.zeroize();
        self.locked = false;
    }
}

pub fn with_sensitive_string<T, F>(secret: String, f: F) -> Result<T>
where
    F: FnOnce(&str) -> Result<T>,
{
    let buffer = SensitiveBuffer::from_vec(secret.into_bytes())?;
    let text = std::str::from_utf8(buffer.bytes.as_slice())
        .map_err(|e| ButterflyBotError::SecurityPolicy(format!("invalid utf-8 in sensitive string: {e}")))?;
    f(text)
}

pub fn run_startup_self_check() -> Result<StartupComplianceReport> {
    let report = StartupComplianceReport {
        strict_profile: true,
        page_locking_ready: verify_page_locking()?,
        core_dump_protection_ready: enforce_core_dump_protection()?,
        ptrace_protection_ready: enforce_ptrace_protection()?,
    };

    tracing::info!(
        target: "security",
        event = "strict_profile_startup_self_check",
        strict_profile = report.strict_profile,
        page_locking_ready = report.page_locking_ready,
        core_dump_protection_ready = report.core_dump_protection_ready,
        ptrace_protection_ready = report.ptrace_protection_ready,
        compliant = report.is_compliant(),
        "security startup self-check complete"
    );

    if !report.is_compliant() {
        return Err(ButterflyBotError::SecurityPolicy(
            "Strict profile compliance self-check failed".to_string(),
        ));
    }

    Ok(report)
}

fn verify_page_locking() -> Result<bool> {
    let sample = vec![0u8; 4096];
    lock_bytes(&sample)?;
    unlock_bytes(&sample)?;
    Ok(true)
}

#[cfg(target_os = "linux")]
fn enforce_core_dump_protection() -> Result<bool> {
    let mut current = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };

    let get_before = unsafe { libc::getrlimit(libc::RLIMIT_CORE, &mut current) };
    if get_before != 0 {
        return Err(ButterflyBotError::SecurityPolicy(
            "failed to read RLIMIT_CORE".to_string(),
        ));
    }

    let hardened = libc::rlimit {
        rlim_cur: 0,
        rlim_max: current.rlim_max,
    };

    let set_result = unsafe { libc::setrlimit(libc::RLIMIT_CORE, &hardened) };
    if set_result != 0 {
        return Err(ButterflyBotError::SecurityPolicy(
            "failed to disable core dumps".to_string(),
        ));
    }

    let mut after = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    let get_after = unsafe { libc::getrlimit(libc::RLIMIT_CORE, &mut after) };
    if get_after != 0 || after.rlim_cur != 0 {
        return Err(ButterflyBotError::SecurityPolicy(
            "core dump protection verification failed".to_string(),
        ));
    }

    Ok(true)
}

#[cfg(not(target_os = "linux"))]
fn enforce_core_dump_protection() -> Result<bool> {
    Err(ButterflyBotError::SecurityPolicy(
        "strict profile requires Linux core dump protection controls".to_string(),
    ))
}

#[cfg(target_os = "linux")]
fn enforce_ptrace_protection() -> Result<bool> {
    let deny_ptrace = unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) };
    if deny_ptrace != 0 {
        return Err(ButterflyBotError::SecurityPolicy(
            "failed to set ptrace protection (PR_SET_DUMPABLE=0)".to_string(),
        ));
    }

    let dumpable = unsafe { libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0) };
    if dumpable != 0 {
        return Err(ButterflyBotError::SecurityPolicy(
            "ptrace protection verification failed".to_string(),
        ));
    }

    Ok(true)
}

#[cfg(not(target_os = "linux"))]
fn enforce_ptrace_protection() -> Result<bool> {
    Err(ButterflyBotError::SecurityPolicy(
        "strict profile requires Linux ptrace protection controls".to_string(),
    ))
}

#[cfg(target_os = "linux")]
fn lock_bytes(value: &[u8]) -> Result<()> {
    if value.is_empty() {
        return Ok(());
    }
    let rc = unsafe { libc::mlock(value.as_ptr() as *const libc::c_void, value.len()) };
    if rc != 0 {
        return Err(ButterflyBotError::SecurityPolicy(
            "strict profile requires page locking for sensitive buffers".to_string(),
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn lock_bytes(_value: &[u8]) -> Result<()> {
    Err(ButterflyBotError::SecurityPolicy(
        "strict profile requires page locking on this platform".to_string(),
    ))
}

#[cfg(target_os = "linux")]
fn unlock_bytes(value: &[u8]) -> Result<()> {
    if value.is_empty() {
        return Ok(());
    }
    let rc = unsafe { libc::munlock(value.as_ptr() as *const libc::c_void, value.len()) };
    if rc != 0 {
        return Err(ButterflyBotError::Runtime(
            "failed to unlock sensitive buffer".to_string(),
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn unlock_bytes(_value: &[u8]) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensitive_string_scopes_plaintext_to_closure() {
        let result = with_sensitive_string("phase-g-secret".to_string(), |secret| {
            assert_eq!(secret, "phase-g-secret");
            Ok(secret.len())
        })
        .unwrap();

        assert_eq!(result, 14);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn startup_self_check_reports_strict_compliance() {
        let report = run_startup_self_check().unwrap();
        assert!(report.strict_profile);
        assert!(report.page_locking_ready);
        assert!(report.core_dump_protection_ready);
        assert!(report.ptrace_protection_ready);
        assert!(report.is_compliant());
    }
}
