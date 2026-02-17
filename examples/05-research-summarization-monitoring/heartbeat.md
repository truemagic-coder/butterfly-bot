# Heartbeat: Research, Summarization & Monitoring

On each heartbeat tick:

1. Query monitored sources through Zapier-connected workflows.
2. Aggregate and deduplicate findings by topic.
3. Detect meaningful changes against prior memory/baselines.
4. Produce a summary with:
   - New signals
   - Why they matter
   - Recommended next actions
5. Create tasks/reminders for action-required findings.
6. Notify user only for threshold-triggered events; batch lower-priority items into digest form.

Guardrails:
- Distinguish facts from inference.
- Mark incomplete or uncertain data explicitly before recommending action.
