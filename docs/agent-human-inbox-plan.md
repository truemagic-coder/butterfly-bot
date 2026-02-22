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

## Delivery Phases

### Phase 1 (MVP)

- Inbox tab (read + basic status actions)
- Derived unified view from reminders/todos/tasks/plans
- Actionable count in UI header

### Phase 2

- macOS dock badge integration
- retry + event telemetry for delivery
- agent handoff conventions for work item creation

### Phase 3

- richer filtering/grouping
- ownership reassignment
- approval/blocking workflows

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
