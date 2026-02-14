## Security Audit (UI-first)

Butterfly Bot provides a **Security Audit** action in the desktop app:

- **Path:** `Config → Security Audit → Run Security Audit`
- **Transport:** daemon endpoint `POST /security_audit`
- **Output:** ranked findings (`critical`, `high`, `medium`, `low`) with status and remediation guidance

## Scope

The current security audit focuses on local configuration and runtime posture checks, including:

- daemon auth token presence
- inline secret hygiene in config
- sandbox mode posture (`off`, `non_main`, `all`)
- runtime posture for high-risk tools (`coding`, `mcp`, `http_call`)
- global network `default_deny` posture

## Why no auto-fix

Automatic security rewrites are intentionally disabled in the app-first direction.

Rationale:

- Safe-looking changes can break local workflows or required network access.
- Determining a truly safe fix often needs user intent and environment context.
- Accidental lockouts are worse than actionable, reviewable findings.

Because of this, findings provide **manual remediation steps** rather than mutating config.

## Operating recommendations

- Keep a daemon token configured; do not run with an empty token.
- Keep sandbox mode at `non_main` (default) or `all` for stricter isolation.
- Keep high-risk tools on WASM runtime unless there is a deliberate exception.
- Use `tools.settings.permissions.default_deny = true` and explicit allowlists.
- Keep provider API keys in OS keychain secrets, not inline in config JSON.

## Known limits

- The audit is static posture analysis; it does not prove exploit resistance.
- It does not run active penetration tests or external attack simulation.
- Findings are based on current local config and daemon runtime context.
- Severity is heuristic and should be interpreted alongside your deployment model.

## Response shape

The security audit response includes:

- `overall`: highest non-pass severity found
- `findings[]`:
  - `id`
  - `severity`
  - `status`
  - `message`
  - `fix_hint`
  - `auto_fixable`
