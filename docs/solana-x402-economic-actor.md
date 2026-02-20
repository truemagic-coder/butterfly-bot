# Solana + x402 Economic Actor

Butterfly Bot supports an economic-agent model using Solana integrations and `x402-rs` protocol support.

## What this means

- The assistant can participate in payment/signing workflows as a policy-bounded actor.
- High-risk or policy-required actions are escalated for user approval.
- Lower-risk actions can run autonomously when policy permits.

## Trust and Safety Model

- Signing is isolated behind signer/security boundaries.
- Policy precedence is explicit:
  1. runtime/context policy
  2. user policy
  3. global defaults
- If a higher-priority layer requires approval, auto-sign is blocked.

## x402 Support Direction

- Integrates with `x402-rs` flows, including Solana scheme handling.
- Payment-required responses can be normalized to internal intents.
- Approval, deny, and expiry states are first-class outcomes.

## Recommended Operator Posture

- Start with conservative limits and allowlists.
- Require approval for unfamiliar counterparties.
- Keep audit and doctor checks green before enabling broad autonomy.

## Related Docs

- [docs/tpm-cocoon-security-plan.md](tpm-cocoon-security-plan.md)
- [docs/threat-model.md](threat-model.md)
- [docs/security-audit.md](security-audit.md)
