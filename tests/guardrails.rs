use serde_json::json;

use butterfly_bot::guardrails::pii::{NoopGuardrail, PiiGuardrail};
use butterfly_bot::interfaces::guardrails::InputGuardrail;

#[tokio::test]
async fn guardrails_work() {
    let noop = NoopGuardrail;
    assert_eq!(noop.process("hi").await.unwrap(), "hi");
    let out = <NoopGuardrail as butterfly_bot::interfaces::guardrails::OutputGuardrail>::process(
        &noop, "out",
    )
    .await
    .unwrap();
    assert_eq!(out, "out");

    let pii = PiiGuardrail::new(None);
    let scrubbed = pii.process("email test@example.com").await.unwrap();
    assert!(scrubbed.contains("[REDACTED]"));
    let scrubbed =
        <PiiGuardrail as butterfly_bot::interfaces::guardrails::OutputGuardrail>::process(
            &pii,
            "call +1 555 123 4567",
        )
        .await
        .unwrap();
    assert!(scrubbed.contains("[REDACTED]"));

    let custom = PiiGuardrail::new(Some(json!({"replacement":"X"})));
    let scrubbed = custom.process("call +1 555 123 4567").await.unwrap();
    assert!(scrubbed.contains("X"));
}
