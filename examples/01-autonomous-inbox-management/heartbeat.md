# Heartbeat: Autonomous Inbox Management

On each heartbeat tick:

1. Check for new inbox items through Zapier-connected email actions.
2. Triage by urgency and category (urgent/important/routine/promotional).
3. For urgent items:
   - Produce a short summary.
   - Draft a reply.
   - Escalate to user for approval if risk is high.
4. For important routine items:
   - Draft response or apply label/archive workflow.
5. For newsletters/promotions/spam:
   - Archive or unsubscribe where policy allows.
6. Create reminders for unanswered high-priority threads.
7. Emit a compact status digest: processed count, escalations, pending approvals.

Guardrails:
- Do not send high-risk outbound replies without explicit user approval.
- Prefer deterministic, repeatable workflows over one-off actions.
