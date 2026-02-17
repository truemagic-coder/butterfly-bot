# Heartbeat: Morning Briefings & Proactive Digests

On scheduled morning tick:

1. Collect inputs via Zapier integrations:
   - Weather
   - Calendar events
   - Priority tasks
   - News/topic monitoring
   - Optional portfolio/business metrics
2. Deduplicate and rank by user impact.
3. Generate one digest with:
   - What matters now
   - What changed since yesterday
   - Top 3 recommended actions
4. Deliver digest to target channel (email/chat) through Zapier.
5. Create follow-up reminders for time-bound items.

Fallback behavior:
- If a source fails, continue with remaining sources and mark the section as partial.
- Never block full digest delivery because one provider is unavailable.
