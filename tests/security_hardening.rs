#[cfg(target_os = "linux")]
use butterfly_bot::security::hardening::run_startup_self_check;
use butterfly_bot::security::hardening::{with_sensitive_string, StartupComplianceReport};

#[test]
fn sensitive_string_executes_in_bounded_scope() {
    let output = with_sensitive_string("strict-profile-passphrase".to_string(), |secret| {
        assert_eq!(secret, "strict-profile-passphrase");
        Ok(secret.len())
    })
    .unwrap();

    assert_eq!(output, 25);
}

#[test]
fn compliance_report_requires_all_controls() {
    let report = StartupComplianceReport {
        strict_profile: true,
        page_locking_ready: true,
        core_dump_protection_ready: true,
        ptrace_protection_ready: true,
    };
    assert!(report.is_compliant());

    let not_compliant = StartupComplianceReport {
        strict_profile: true,
        page_locking_ready: false,
        core_dump_protection_ready: true,
        ptrace_protection_ready: true,
    };
    assert!(!not_compliant.is_compliant());
}

#[cfg(target_os = "linux")]
#[test]
fn strict_profile_self_check_passes_on_linux() {
    let report = run_startup_self_check().unwrap();
    assert!(report.is_compliant());
}
