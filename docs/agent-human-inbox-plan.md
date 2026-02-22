# Butterfly Bot Agent → Human Inbox Plan

## Objective

Shift Butterfly Bot from chat-first interaction to execution-first collaboration:

- Agent creates and maintains actionable work for humans.
- Human sees a clear inbox of what requires attention.
- macOS app icon badge reflects actionable human work.

## Product Principles

1. **Action over conversation**: chat explains, inbox drives execution.
2. **Single source of truth**: reminders/todos/tasks/plans converge into one actionable list.
3. **Reliable delivery**: no item is silently dropped if notification delivery fails.
4. **Low cognitive load**: status and ownership are explicit.
5. **Human trust**: app always shows what needs user action now.

## Scope

- New inbox concept in UI for Agent → Human handoff.
- Unified work-item model across reminders/todos/tasks/plans.
- Notification semantics tied to actionable items.
- macOS dock badge count for actionable human work.
- Minimum diagnostics to prove dispatch, delivery, and acknowledgment paths.

## Work Item Model

### Canonical item fields

- `id`
- `source_type`: `reminder | todo | task | plan_step`
- `title`
- `details` (optional)
- `owner`: `human | agent`
- `status`: `new | acknowledged | in_progress | blocked | done | dismissed`
- `priority`: `low | normal | high | urgent`
- `due_at` (optional)
- `created_at`, `updated_at`
- `requires_human_action`: boolean
- `origin_ref` (link back to tool/entity)

### Status semantics

- `new`: newly assigned to human and unseen.
- `acknowledged`: user saw it but has not started.
- `in_progress`: user started.
- `blocked`: waiting on context/approval/dependency.
- `done`: completed.
- `dismissed`: intentionally ignored/archived.

## Workstream A — Inbox UX (Agent → Human)

### A1. Inbox tab

Add a first-class Inbox tab to the desktop UI:

- Default sort: urgent/high first, then due soon, then newest.
- Visual grouping: `Needs Action`, `In Progress`, `Blocked`, `Done`.
- Minimal row actions: `Acknowledge`, `Start`, `Done`, `Snooze`.

### A2. Human workload clarity

Show concise summary chips:

- `Actionable now`
- `Overdue`
- `Blocked`
- `Awaiting human`

### A3. Agent handoff format

Agent responses that imply work must emit structured items instead of only chat text.

Acceptance:

- Any “you should do X” can appear as an inbox item with owner/status.

## Workstream B — Data Unification

### B1. Adapter layer

Map existing tools/entities into canonical work items:

- reminders → item with due time
- todos → checklist-style items
- tasks (scheduled) → generated follow-up items when human action needed
- plans → plan steps represented as assignable items

### B2. Non-destructive migration

- Keep existing tables and APIs initially.
- Build derived inbox view/model first.
- Defer full table consolidation until behavior is stable.

Acceptance:

- Existing reminders/todos/tasks remain functional.
- Inbox renders combined actionable list without duplicating completed entries.

## Workstream C — Notification Reliability

### C1. Delivery contract

A reminder/work item is marked fired/completed only after successful delivery/dispatch state transition.

### C2. Retry behavior

If notification delivery fails:

- Keep item actionable.
- Retry on next scheduler cycle with backoff.
- Emit diagnostic event for visibility.

### C3. Event telemetry

Record per-attempt state:

- `queued`
- `delivery_attempted`
- `delivered` or `delivery_failed`

Acceptance:

- No “lost” reminders due to transient notification failures.

## Workstream D — macOS Badge Model

### D1. Badge count definition

Badge count = count of items where:

- `owner = human`
- `requires_human_action = true`
- `status in (new, acknowledged, blocked, in_progress)`
- optionally include overdue weighting in UI, not count inflation

### D2. Badge lifecycle

- Increment on new actionable assignment.
- Decrement on `done`/`dismissed`.
- Recompute from source of truth at startup and periodically.

### D3. macOS integration

- Set dock badge label from computed count.
- Clear badge when count is zero.

Acceptance:

- Badge always matches actionable human work count after restart and during runtime.

## Workstream E — Agent Behavior Policy

### E1. Planner output contract

For plan generation, require:

- explicit human-owned steps
- due recommendations where relevant
- blockers/dependencies captured as metadata

### E2. Chat fallback policy

- If user asks conversationally, agent can respond in chat.
- If response contains actionable directives, agent should offer/create inbox items.

Acceptance:

- Agent naturally produces execution artifacts, not just prose.

### E3. Conversation rules and tone

- Chat tab is reserved for conversational messages only (`human` ↔ `agent`).
- Activity tab is reserved for operational telemetry and status updates.
- Audit tab remains the authoritative ledger for lifecycle/state evidence.

Tone guidelines for agent-initiated chat:

- concise and action-oriented
- explicit ask when human decision is required
- include one clear next step
- avoid noisy low-signal status chatter

Baseline trigger policy (implemented):

- Trigger proactive chat only for human-owned actionable items that are `blocked` or overdue.
- Deduplicate by `origin_ref` so each active blocker/overdue item is nudged once until state changes.
- Throttle proactive nudges to avoid spam bursts.
- Policy controls live in Config tab (`proactive_chat.enabled`, `proactive_chat.min_interval_seconds`).
- Policy controls live in Config tab (`proactive_chat.enabled`, `proactive_chat.min_interval_seconds`, `proactive_chat.severity`, quiet-hours window).

## Workstream F — Singularity & Symbiosis Workflow (Bidirectional Transparency)

### F1. Unified Agent ↔ Human model with clear surface boundaries

- Chat = conversation (human prompts + agent replies + deliberate agent-initiated asks).
- Activity = operational timeline (status, transitions, retries, deliveries, failures).
- Audit = authoritative evidence stream with lifecycle metadata.

### F2. Agent → Human proactive updates (without being asked)

Agent must publish concise updates for:

- work started
- tool/action completed
- blocked/waiting state
- retries/failures/recovery
- plan changes and reprioritization

### F3. UI surface for operational transparency

Add a dedicated visibility surface (either):

- a new `Timeline`/`Ops` tab, or
- an `Inbox + Timeline` composite view, or
- enriched Chat with filterable system updates.

Minimum UX requirements:

- clear separation of `human`, `agent`, and `system` events
- timestamped event feed
- link from each event to related `origin_ref` work item when available
- quick filters: `All`, `Agent updates`, `Human chat`, `Errors/Blocked`

### F5. Two explicit tabs: Audit + Timeline

Current implementation status:

- ✅ Timeline and Audit tabs are live in UI.
- ✅ Blocker-first cards and owner lane split are implemented.
- ✅ Critical-path dependency chains are surfaced from `dependency_refs` when available.
- ✅ Audit feed includes transition context (`from`, `to`, `actor`, `reason`) for inbox transitions.
- ✅ Timeline blocker cards drill down to source Inbox item and filtered Audit context.
- ✅ Audit event rows deep-link back to source Inbox row.
- ✅ Timeline and Audit blockers deep-link into Chat with prefilled context prompt.
- ✅ New audit/ops events are bridged into Activity as system timeline updates.
- ✅ Exact historical chat message-id anchor is resolved from Audit event time and highlighted in Chat.
- ✅ Viewport auto-scroll jump now snaps directly to anchored message card.
- ✅ Baseline proactive Agent→Human chat nudges now fire for blocked/overdue human-action items.
- ✅ Todo tickets now carry optional sizing (`t_shirt_size`, `story_points`) and 3-point estimates.
- ✅ Read-only Gantt tab is available for estimate-based timeline visualization.

Operational UI policy now enforced:

- Chat tab displays only conversational messages.
- Activity tab displays operational/system updates.

#### Audit tab (authoritative event/state ledger)


Purpose: full transparency and traceability of work lifecycle.

Show an append-only, filterable log of:

- item creation
- ownership changes (`human` / `agent`)
- lifecycle transitions (`new → acknowledged → in_progress → blocked → done/dismissed`)
- retries, delivery outcomes, and failures
- actor metadata (`agent`, `human`, `system`)

Minimum table/view fields:

- `timestamp`
- `origin_ref`
- `item title`
- `actor`
- `event/action`
- `previous_state`
- `new_state`
- `reason/details`

#### Timeline tab (execution flow and dependency visibility)

Purpose: operational planning + immediate blocker discovery for both human and agent.

Render a time-aware plan/work view (Gantt-like but workflow-first):

- grouped by owner lane (`human lane`, `agent lane`)
- bars/cards for active and upcoming work
- dependency and blocker edges
- overdue and risk highlighting
- compact “Now / Next / Blocked” summary strip

Blocker-first requirements:

- blocked items pinned at top with severity/age
- separate counts for `human-blocked` and `agent-blocked`
- one-click drilldown from blocker to chat context + originating work item

Acceptance:

- User can identify current blockers for human and agent in < 5 seconds.
- Audit tab can answer “what changed, when, and why” without ambiguity.
- Timeline tab shows critical path and blocked path at a glance.

### F4. One-model principle

No one-way surfaces:

- Inbox, chat, and telemetry are different views over the same underlying lifecycle.
- Agent can speak into chat; human can act from chat/inbox; both see the same state.

Acceptance:

- Human can see what the agent is doing in real time without asking.
- Every significant agent action is visible in UI as a timeline/chat event.
- Work item transitions are visible both in Inbox and shared conversation timeline.

## Delivery Phases

### Phase 1 (MVP)

- Inbox tab (read + basic status actions)
- Derived unified view from reminders/todos/tasks/plans
- Actionable count in UI header

### Phase 2

- macOS dock badge integration
- retry + event telemetry for delivery
- agent handoff conventions for work item creation
- proactive Agent → Human operational updates in shared stream

### Phase 3

- richer filtering/grouping
- ownership reassignment
- approval/blocking workflows
- dedicated Timeline/Ops UI with cross-links to inbox items
- explicit `Audit` tab + `Timeline` tab with blocker-first views

## Risks & Mitigations

- **Risk:** Duplicate items from multiple sources.
  - **Mitigation:** stable `origin_ref` and dedupe rules.
- **Risk:** Badge drift after crashes/restarts.
  - **Mitigation:** deterministic recompute at daemon startup.
- **Risk:** Too much inbox noise.
  - **Mitigation:** only badge actionable human-owned items.

## Definition of Done

1. User can rely on Inbox as the default place to see what they must do.
2. Agent-generated plans produce explicit human-action items.
3. Reminder/work notifications are retriable and not silently lost.
4. macOS badge count reflects actionable human work accurately.
5. Chat remains available but is no longer the only execution surface.
6. Agent and human share a transparent, bidirectional operational timeline.

## Implementation Status (2026-02-21)

- ✅ Inbox tab in desktop UI with grouped sections and row actions.
- ✅ Unified derived inbox list across reminders/todos/tasks/plans.
- ✅ Actionable count surfaced in UI header.
- ✅ Daemon inbox APIs:
  - `GET /inbox`
  - `GET /inbox/actionable_count`
- ✅ Reminder delivery telemetry states emitted (`queued`, `delivery_attempted`, `delivered`, `delivery_failed`).
- ✅ Reminder delivery diagnostics endpoint:
  - `GET /reminders/delivery_events`
- ✅ Diagnostics tab rendering of reminder delivery events.
- ✅ macOS dock badge best-effort sync from actionable inbox count.
- ✅ Inbox lifecycle transitions enforced via `rust-fsm` (`src/inbox_fsm.rs`).
- ✅ Persisted inbox lifecycle transitions via daemon endpoint and SQLCipher store:
  - `POST /inbox/transition`
  - `inbox_item_states` table

Open follow-up:

- ⏳ Add optional transition history timeline (who/when/action metadata) for audit UX.
