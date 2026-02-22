# Inbox / Audit / Timeline Implementation Checklist

_Last updated: 2026-02-22_

## Core Inbox

- [x] Inbox tab with grouped sections and summary chips
- [x] Inbox daemon APIs: `/inbox` and `/inbox/actionable_count`
- [x] Inbox lifecycle state machine (`rust-fsm`)
- [x] Persisted inbox item state overrides (`inbox_item_states`)
- [x] Transition endpoint (`/inbox/transition`) with FSM validation
- [x] UI actions routed through daemon transitions (no local-only overrides)

## Telemetry & Transparency

- [x] Reminder delivery telemetry stream
- [x] Reminder delivery diagnostics endpoint
- [x] UI event emission for inbox transitions
- [x] Audit events endpoint (`/audit/events`)
- [x] Correlated transition history (old→new state + reason) in event payloads

## New Tabs

- [x] Timeline tab shell and rendering
- [x] Audit tab shell and rendering
- [x] Audit refresh flow (startup + periodic + manual)
- [x] Human/Agent lane split in Timeline
- [x] Blocker-first chips in Timeline
- [x] Timeline and Audit visual hierarchy refresh (accent headers, alert cards, status badges)
- [x] Timeline critical-path grouping (blocked by dependency chain)
- [x] Timeline blocker drilldown to Inbox + Audit context
- [x] Timeline/Audit blocker drilldown to chat context (pre-filled prompt)
- [x] Chat anchor banner + matched message highlighting by `origin_ref`
- [x] Audit/ops events bridged into Activity timeline updates
- [x] Exact chat message-id anchor resolution from Audit event timestamp
- [x] Auto-scroll viewport jump to anchored message-id in chat list
- [x] Baseline proactive Agent→Human nudges for blocked/overdue human-action items
- [x] Proactive-chat policy controls in Config tab (enable + min interval + severity + quiet hours)
- [x] Inner scrollbar-content spacing polish for Inbox/Timeline/Audit
- [x] Optimistic inbox status transitions with per-row in-flight feedback
- [x] Trash action clears user work data (not chat-only)
- [x] Inbox unread/seen dot affordance + clarified Seen semantics
- [x] Read-only Kanban board tab
- [x] Inbox unread dot spacing + blue visibility contrast
- [x] Kanban fixed-width columns/cards (stable layout for empty columns)
- [x] Todo tickets include optional sizing and 3-point time estimates
- [x] Read-only Gantt tab from todo estimates
- [x] Read-only Burndown tab from todo points/estimates
- [x] AI sizing heuristic calibrated with complexity multipliers
- [x] Plan-step due-date parsing from structured fields + text metadata
- [x] Timeline/Kanban badge UI for due date, sizing, points, and estimates
- [x] Inbox badges color-coded for due urgency, size, story points, and estimates
- [x] Inbox due badge fallback parses due date from title/details metadata text
- [x] Todo sizing inference honors explicit metadata tokens (size/points/time estimate)
- [x] Planning tool accepts structured step objects with estimate/sizing fields
- [x] Planning create/update auto-materializes plan steps into todo items
- [x] Inbox todo-specific Block action button
- [x] Inbox Undo action to reopen Done items (DoD correction)

## Quality Gates

- [x] Daemon tests for inbox and actionable count
- [x] Daemon tests for persisted transition behavior
- [x] Daemon test for audit events endpoint
- [x] Full daemon integration suite run after audit endpoint work
- [ ] Full test suite run after latest UI tab additions
- [ ] UX polish pass for copy, spacing, and visual hierarchy

## Next up

1. Add timeline edge visualization (lineage graph) for multi-dependency chains.
2. Run full test suite and fix regressions.
3. Polish blocker cards for immediate at-a-glance triage.
4. Add keyboard navigation between previous/next anchor matches.
5. Add channel routing policy toggle for proactive updates (Chat vs Activity) in Config tab.
