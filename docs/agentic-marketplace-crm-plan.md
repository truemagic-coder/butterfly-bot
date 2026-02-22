# Agentic Marketplace + CRM Evolution Plan

## Goal
Build an MCP-first, Amazon-style agent marketplace with embedded CRM, where paid discovery and x402-powered purchases mint Solana NFT contracts on-chain.

## Core Thesis
- Marketplace discovery is the primary product surface; chat is optional and can be integrated later.
- CRM is embedded into MCP workflows as relationship, commitment, and revenue state.
- The winning stack is:
  1. Paid MCP discovery + listing/ranking + trust signals
  2. x402 purchase rail with you as facilitator
  3. Solana NFT contract receipts for on-chain proof of agreement
  4. MCP-native CRM state (parties, opportunities, commitments, fulfillment, renewals)

---

## Success Criteria (12-month)
- 30%+ reduction in manual coordinator/admin work
- 20%+ faster cycle time from opportunity -> fulfilled outcome
- 95%+ traceability for commitments, decisions, and handoffs
- <1% settlement failure rate for on-chain payments/escrow flows
- 2+ pilot organizations running mixed human/agent teams in production

---

## Product Scope

### In Scope
- Marketplace discovery and ranking (Amazon-style)
- Paid listing/discovery mechanics for providers
- MCP-based offer/accept/purchase/fulfill lifecycle
- x402 payment facilitation for purchases
- Solana NFT mint as contract/receipt per purchase
- MCP-native CRM model (accounts, counterparties, opportunities, commitments, renewals)
- Full audit/event trail for decisions, actions, purchases, and fulfillment

### Out of Scope (initial)
- Full CPQ parity with enterprise incumbents
- Broad no-code app-builder parity
- Deep omnichannel call-center feature set
- Hard dependency on `butterfly-chat` for launch milestones

---

## Phased Roadmap

## Phase 0 — Foundation Hardening (0-4 weeks)
### Objectives
- Stabilize current workflow engine, dependencies, and inbox/kanban/gantt coherence.
- Establish canonical IDs and event contracts for future marketplace objects.

### Deliverables
- Canonical object ID scheme for participants, threads, work items, contracts, payouts
- Event schema v1 (action requested, accepted, blocked, fulfilled, disputed, settled)
- Dependency graph reliability checks + regression tests
- Operational dashboards for queue depth, latency, and failure classes

### Exit Criteria
- End-to-end workflow reproducibility for seeded test scenarios
- No silent dependency loss in UI/API paths

---

## Phase 1 — Agent Communication OS (4-10 weeks)
### Objectives
- Ship robust communication workflows between humans and agents.
- Support clear state transitions and handoffs across roles.

### Deliverables
- Threaded conversations with structured intents and context attachments
- Ownership and delegation semantics (`human`, `agent`, team role)
- SLA-aware inbox views (action due, blocked, waiting, done)
- Explainability artifacts per automated action (why/inputs/tools/result)

### Exit Criteria
- Pilot users complete cross-agent workflows without external tools
- 90%+ of handoffs tracked with explicit owner + next action

---

## Phase 2 — Agent-Native CRM Core (8-16 weeks)
### Objectives
- Replace "CRM UI-first" model with "state-and-commitments-first" model.

### Data Model
- `Party` (human, team, org, agent)
- `Relationship` (trust tier, permissions, policy)
- `Opportunity` (intent, value, stage, confidence)
- `Commitment` (deliverable, due, owner, dependencies, SLA)
- `Engagement` (messages, meetings, outcomes)
- `RevenueEvent` (invoice, payment, refund, fee)

### Deliverables
- Relationship graph + opportunity/commitment lifecycle
- Audit-safe state transitions and immutable event append log
- KPI layer: conversion, cycle time, fulfillment rate, retention risk
- Admin controls for policy, permissions, and compliance boundaries

### Exit Criteria
- Sales/service/revops workflows executable without Salesforce for pilot scope
- Forecast and delivery metrics trusted by operators

---

## Phase 3 — Solana Marketplace Primitives (12-24 weeks)
### Objectives
- Add trust-minimized coordination and payment rails.

### Deliverables
- On-chain identity/linking strategy (wallet <-> party mapping)
- Escrow contracts for milestone-based delivery
- Dispute and arbitration workflow integration
- Reputation signals from completion quality + dispute outcomes
- Settlement ledger integrated with off-chain CRM state

### Exit Criteria
- First paid agent-human workflows settled on Solana in production
- Settlement and dispute lifecycle visible in same operational UI

---

## Phase 4 — Open Agentic Marketplace (20-36 weeks)
### Objectives
- Enable discoverability and safe interaction across organizations.

### Deliverables
- Marketplace listing model (skills, SLAs, pricing, policy requirements)
- Offer negotiation protocol and machine-readable work contracts
- Policy enforcement (allowed tools, spending caps, jurisdiction/compliance rules)
- Multi-party fulfillment (subcontracted agents/humans with dependency graph)

### Exit Criteria
- External providers onboarded and transacting with measurable quality
- Network effects visible (repeat engagements, referral loops, trust scores)

---

## Architecture Tracks (Cross-Cutting)

## A) Protocol + APIs
- Define stable schemas for conversation, commitments, payments, audit events.
- Version API contracts and build compatibility tests.

## B) Security + Compliance
- Signed action intents, policy engine, least privilege by default.
- Audit chain with tamper-evident hashes and evidence export.

## C) Reliability + Observability
- Retry semantics, idempotency keys, dead-letter handling.
- Metrics: action latency, timeout rates, tool failure classes.

## D) Governance + Human Control
- Human override checkpoints for high-risk or high-value operations.
- Approval flows for payment release and policy exceptions.

---

## CRM Displacement Strategy (Practical)

## Wedge 1
- Agentic deal execution + commitment tracking + fulfillment handoffs.

## Wedge 2
- Renewal and account health automations with explicit human-in-the-loop controls.

## Wedge 3
- Full opportunity-to-fulfillment traceability with Solana settlement evidence.

### Migration Pattern
1. Mirror from Salesforce (read/sync)
2. Run dual-write for selected objects
3. Cut over team-by-team by workflow domain
4. Keep export/backfill safety nets

---

## KPI Framework

### Product KPIs
- Time-to-first-fulfilled outcome
- Handoff failure rate
- Dependency resolution time

### Revenue KPIs
- Opportunity conversion rate
- Cycle time by stage
- Gross retention / net retention proxies

### Marketplace KPIs
- Match-to-accept ratio
- Escrow completion rate
- Dispute frequency and resolution time

### Trust KPIs
- Policy violation rate
- Explainability coverage
- Audit completeness score

---

## Risks and Mitigations
- **Regulatory/payment risk** -> progressive rollout by jurisdiction + clear custody boundaries
- **Agent quality variance** -> reputation + sandboxing + policy guards + staged autonomy
- **Enterprise adoption friction** -> coexistence connectors + migration toolkits + governance controls
- **Data integrity drift** -> event sourcing, reconciliation jobs, invariant checks

---

## Immediate Next 30 Days
1. Finalize MCP marketplace schema: `listing`, `offer`, `purchase`, `fulfillment`, `reputation_event`.
2. Implement x402 purchase flow with facilitator settlement metadata.
3. Implement Solana NFT mint pipeline for contract receipts tied to purchase IDs.
4. Implement CRM-core objects in MCP domain: `party`, `relationship`, `opportunity`, `commitment`, `renewal`.
5. Ship v1 marketplace analytics: discovery impressions, conversion, take rate, fulfillment success.

---

## Decision Gates
- **Gate A (Week 4):** paid MCP discovery + purchase flow stable in pilot?
- **Gate B (Week 8-10):** x402 facilitation + NFT contract minting safe and auditable?
- **Gate C (Week 12-16):** CRM-core metrics trusted directly from MCP event stream?
- **Gate D (Week 20+):** external provider onboarding, quality, and monetization targets met?

---

## Summary
This roadmap does not discard CRM; it reframes CRM as the relationship and commitment state layer behind an agentic marketplace. If execution quality is high, Salesforce becomes optional because your system captures what enterprises actually pay CRM for: trusted coordination, revenue visibility, accountability, and repeatable outcomes.

---

## MCP-First Execution Mapping (No `butterfly-chat` Dependency)

Launch sequence is explicitly independent of chat readiness. Chat can integrate later as another client surface.

### Sprint 1 (discovery + listing economy)
- Define listing schema and ranking inputs:
  - capability tags, SLA, jurisdiction, trust score, historical fulfillment
- Add paid discovery products:
  - sponsored listings
  - category boosts
  - verification badge placement
- Add provider onboarding + KYC/attestation workflow.
- Add pre-approved artist registry and edition inventory tracking for listing NFTs.

### Sprint 2 (purchase flow + x402 facilitation)
- Implement MCP purchase APIs:
  - `create_offer`, `accept_offer`, `initiate_purchase`, `confirm_fulfillment`
- Integrate x402 as required payment rail for purchases.
- Persist facilitator metadata for every settlement event.

### Sprint 3 (on-chain contract receipts)
- On successful x402 purchase, mint Solana NFT contract receipt containing:
  - purchase ID, provider ID, buyer ID, scope hash, timestamp, fulfillment terms hash
- Store NFT mint address in CRM commitment and purchase records.
- Add dispute reference linking from CRM to NFT contract receipt.

### Sprint 3.5 (Art NFT listing layer)
- Introduce **Art NFT Mint Service** using pre-approved artist collections only.
- Require each paid listing to include an Art NFT assignment:
  - AI matches listing category/style/brand to approved artist inventory
  - system reserves a limited-edition token before listing publish
- Charge art fee as part of listing checkout:
  - `listing_base_fee + art_nft_fee + discovery_boost_fee`
- Mint metadata must include:
  - artist ID, collection ID, edition number, listing ID, lister ID, fee receipt refs
- Enforce scarcity and edition controls:
  - hard cap per collection
  - no edition reuse
  - deterministic reservation -> mint flow

### Sprint 4 (CRM in MCP core)
- Implement CRM objects and state machine in MCP service:
  - `Party`, `Relationship`, `Opportunity`, `Commitment`, `Renewal`
- Auto-create/update CRM records from discovery, purchase, fulfillment, and dispute events.
- Ship dashboards computed from MCP event log (no chat dependency).

### Non-negotiable technical constraints
- Idempotency keys on all mutable MCP endpoints.
- Immutable append-only event log for marketplace + CRM transitions.
- Deterministic mapping from purchase event -> NFT contract mint metadata.
- Policy checks and jurisdiction rules before purchase acceptance.
- Transparent fee schedule and facilitator disclosures.
- Only allow minting from approved artist allowlist and active limited-edition inventory.

### Definition of Done for MCP launch milestone
- A buyer can discover providers, purchase through x402, receive on-chain NFT contract proof, and track fulfillment/renewal in CRM views.
- Every purchase and fulfillment state is queryable by `purchase_id` and `commitment_id`.
- Revenue and trust dashboards are generated from MCP + on-chain-linked events.
- `butterfly-chat` remains optional and does not block launch.
- Paid listings include AI-matched Art NFTs and listing invoices explicitly show bundled art fees.

---

## Art NFT Policy for Listings

### Artist Governance
- Maintain a curated allowlist of approved artists and collections.
- Store rights metadata per artist/collection:
  - commercial usage permissions
  - royalty split rules
  - jurisdiction constraints

### Edition Economics
- Collections are limited edition by policy (`max_supply` per collection).
- Pricing model supports:
  - fixed art fee by tier
  - dynamic fee by category demand and edition scarcity
- Revenue split example:
  - artist royalty
  - facilitator/platform fee
  - optional curator fee

### AI Matching Logic (MCP side)
- Inputs:
  - listing taxonomy, category, quality tier, brand tone, jurisdiction, budget
- Outputs:
  - ranked artist candidates
  - selected collection + edition reservation
  - fee quote attached to listing draft
- Guardrails:
  - never bypass allowlist
  - fail closed if no compliant art inventory is available

### Required Events
- `art_match_requested`
- `art_match_scored`
- `art_edition_reserved`
- `listing_fee_quoted`
- `listing_fee_paid`
- `art_nft_minted`
- `art_edition_consumed`

### Required IDs
- `artist_id`
- `collection_id`
- `edition_id`
- `art_match_id`
- `listing_id`
- `fee_invoice_id`
- `mint_tx_id`

---

## Local-First Monetization Model (No Hosted Service)

If products stay desktop/local, revenue comes from software rights, enterprise controls, and network participation fees rather than cloud hosting.

### 1) Paid Pro Desktop License
- Sell per-user annual licenses for advanced capabilities:
  - multi-agent orchestration packs
  - advanced analytics/forecasting
  - compliance/audit exports
  - policy engine templates

### 2) Team/Enterprise Pack (Self-Hosted Optional, Still Local-Controlled)
- Charge for business features that work in private environments:
  - SSO/SAML integration
  - fine-grained RBAC
  - encryption key management and HSM integrations
  - regulated retention + legal hold workflows

### 3) Commercial Connector Pack
- Keep core open source.
- Sell premium connector bundles (ERP, accounting, ticketing, data warehouse, enterprise identity).

### 4) Marketplace Protocol Fees
- Even with local clients, charge protocol/network fees on completed deals:
  - settlement fee
  - escrow/dispute fee
  - optional verification/reputation attestation fee

### 5) Signed Agent/Plugin Marketplace
- Curate and sign trusted agent/plugin packages.
- Revenue via listing fees, rev-share, or paid verification tiers.

### 6) Support + Implementation
- Paid migration programs from legacy CRM.
- Paid enterprise support SLAs and architecture guidance.

### 7) Dual Licensing Strategy
- Core remains open source for adoption.
- Offer a commercial license for orgs needing proprietary redistribution rights, support guarantees, and enterprise add-ons.

## Recommended Packaging
- **Community (free):** core desktop workflow + basic local automations.
- **Pro (paid):** advanced agent orchestration, dependency intelligence, reporting, premium local UX.
- **Enterprise (paid):** governance, identity/compliance packs, premium connectors, contractual support.
- **Marketplace fees:** transaction-based monetization independent of hosting.

## Practical Conclusion
You do not need to close source. With local-first products, monetize:
- rights (licenses)
- trust (signed/verified packages)
- operations (enterprise controls)
- economic activity (marketplace/settlement fees)

---

## Hybrid Revenue Design: Paid MCP + Smart-Contract Fees

This model combines recurring software revenue with transaction revenue:

- **Paid MCP Revenue (SaaS-like):** recurring subscription for managed MCP capabilities
  - policy distribution + updates
  - identity/attestation verification
  - compliance packs and audit pipelines
  - premium routing, guardrails, and analytics
- **Smart-Contract Revenue (Protocol):** fee share on escrow, settlement, and dispute flows
  - flat execution fee
  - percentage settlement fee
  - optional dispute/arbitration fee

### Why this works
- Predictable baseline revenue from MCP subscriptions
- Upside revenue proportional to marketplace volume
- Keeps local desktop workflow intact while monetizing network coordination and trust

### Suggested fee architecture
1. **MCP Subscription Tiers**
   - Starter: single org, limited policy packs
   - Pro: multi-org, advanced controls, premium analytics
   - Enterprise: private policy bundles, signed attestations, contractual SLA
2. **On-Chain Fee Splits**
   - settlement fee (e.g., small % or min fee)
   - escrow creation/release fee
   - dispute resolution fee
3. **Verification Fees**
   - paid attestations for agents/providers
   - boosted trust score visibility in marketplace discovery

### Critical implementation choices
- Keep contract logic minimal and auditable; push complexity to off-chain policy engine.
- Make fee schedules transparent and versioned.
- Use clear separation between:
  - open protocol contracts
  - paid MCP trust/control services

### Risk controls
- Fee extraction risk: cap effective take rate and publish schedule.
- Regulatory risk: avoid custody where possible; use explicit jurisdiction controls.
- UX risk: hide crypto complexity behind clear status states and plain-language receipts.

### KPI additions for this model
- MCP MRR / ARR
- Protocol fee revenue
- take rate by segment
- settlement success and dispute rate
- verified-provider conversion lift

---

## Butterfly Bot NFT Curation Experience (Plan-Only, No Current Code Dependency)

### Product Intent
Use NFT curation inside Butterfly Bot to humanize the product, build culture, and increase sustained human engagement.

### UX Scope
- Add an `NFT` tab in Butterfly Bot product roadmap.
- Display **master collections** and **child NFTs** in a portfolio-centric view.
- Support first-party and approved third-party master collections.

### Collection Policy
- Default first-party master collection: `butterfly`.
- Whitelisted third-party master collection support, starting with:
  - `walletbubbles` (Solana cNFT marketplace)
- Maintain allowlist state per master collection:
  - active/inactive
  - source type (`nft`, `cNFT`)
  - trust/compliance metadata

### Curation Controls
- Add tags to NFTs (multi-tag, user-defined, normalized).
- Filter by:
  - tag
  - collection
  - artist
  - format (`nft`, `cNFT`)
- Manual ordering controls for storytelling portfolios:
  - drag/drop or move up/down
  - persistent order index
- Create custom named portfolios from selected/ordered NFTs.

### Mirror + Share Experience
- Mirror each user portfolio to a web profile page.
- Shareable URL per portfolio (public or scoped visibility).
- Mirrored payload must include:
  - portfolio metadata
  - collection hierarchy
  - ordered NFT list
  - tags + cNFT/NFT format flags
- Update policy:
  - near-real-time or scheduled sync
  - conflict-safe versioning with last-write metadata

### Marketplace + CRM Linkage
- Portfolio interactions become CRM engagement signals:
  - profile views
  - saves/follows
  - inquiry conversions
- Discovery ranking can include cultural relevance signals from portfolio activity.
- Optional monetization hooks:
  - featured portfolio placements
  - premium curation themes
  - artist spotlight bundles

### Data/Event Model Requirements
Core entities:
- `master_collection`
- `nft_asset`
- `portfolio`
- `portfolio_item`
- `portfolio_tag`
- `portfolio_mirror`

Core events:
- `collection_whitelisted`
- `portfolio_created`
- `portfolio_item_ordered`
- `portfolio_tag_added`
- `portfolio_tag_removed`
- `portfolio_mirror_published`
- `portfolio_mirror_updated`

### Delivery Phasing
1. MVP: read-only master/child NFT explorer + allowlist controls.
2. Curation v1: tags, filters, manual ordering, named portfolios.
3. Mirror v1: per-user web sync and share links.
4. Growth v1: engagement analytics + discovery weighting + monetization experiments.