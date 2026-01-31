# ButterFly Bot — Memory System Plan (SQLite + LanceDB + Graphiti‑style Graph)

## Objectives
- **Local‑only** memory with no external services.
- **Hybrid retrieval**: fast keyword search + semantic recall + graph traversal.
- **Explainable** memory: show why a memory was retrieved.
- **Low‑friction**: embedded storage, no server setup.

## Architecture Overview
```
            ┌───────────────────────────────┐
            │        Agent Runtime          │
            └──────────────┬────────────────┘
                           │
           ┌───────────────┴────────────────┐
           │          Memory Layer          │
           └───────────────┬────────────────┘
                           │
        ┌──────────────────┼──────────────────┐
        │                  │                  │
   SQLite (Diesel)      LanceDB          Graph Layer
 (logs + metadata)    (embeddings)    (entities/edges)
```

## Storage Strategy
### 1) SQLite (Diesel)
**Purpose:** durable logs, metadata, graph edges, summaries, and indexing.

**Tables (core):**
- `sessions`: session identity, model, created_at.
- `messages`: role, content, token_count, created_at, session_id.
- `memories`: summary text, tags, salience, created_at, source_message_id.
- `entities`: name, type, canonical_id, created_at.
- `events`: event_type, payload, occurred_at, created_at.
- `facts`: predicate, object, confidence, source, created_at.
- `edges`: src_node_type/id, dst_node_type/id, edge_type, weight, created_at.
- `memory_links`: memory_id ↔ entity/event/fact relationships.

**Indexes:**
- B‑tree on `session_id`, `created_at` for fast paging.
- FTS5 virtual table for `memories.summary` and `messages.content`.
- Composite index on `edges (src_node_type, src_node_id, edge_type)`.

### 2) LanceDB
**Purpose:** semantic retrieval with embeddings for memory snippets.

**Collections:**
- `memory_vectors`: `id`, `embedding`, `memory_id`, `summary`, `tags`, `created_at`.
- Optionally `message_vectors`: `id`, `embedding`, `message_id`, `content`, `created_at`.

**Embedding strategy:**
- Use **Ollama Qwen3 embedding** for all vector writes.
- Keep embeddings and metadata in LanceDB; raw text remains in SQLite.

### 3) Graph Layer (Graphiti‑style)
**Goal:** capture relationships for better recall and explainability.

**Node types:**
- `Entity` (person, org, place, product, wallet, program, etc.)
- `Event` (meeting, transaction, message, decision)
- `Fact` (relationship: subject‑predicate‑object)
- `Memory` (summary‑level nodes)

**Edge types:**
- `MENTIONED_IN`, `RELATED_TO`, `CAUSED_BY`, `OWNS`, `TRANSFERS`, `WORKS_WITH`, `BELONGS_TO`, `AUTHORS`, `AFFECTS`.

**Graph storage:**
- SQLite `edges` table with weights and timestamps.
- Graph traversal in code with bounded depth (e.g., 1–3 hops).

## Ingestion Flow
1. **Message ingest**
   - Store raw messages in `messages`.
   - Count tokens, capture model info.

2. **Summarization**
   - When session exceeds threshold, summarize and store in `memories`.
   - Tag with entity mentions and topics.

3. **Entity/Fact extraction**
   - Extract entities + relationships from new messages/summaries.
   - Insert into `entities`, `facts`, `events`, `edges`.

4. **Embedding**
   - Create embedding for new `memories` (and optionally messages).
   - Upsert into LanceDB.

## Retrieval Flow
1. **Candidate generation**
   - Keyword candidates from SQLite FTS5.
   - Semantic candidates from LanceDB ANN.

2. **Graph expansion**
   - For top K nodes, traverse edges (1–2 hops) to include linked facts/events/entities.

3. **Rerank**
- Use **Ollama Qwen3 reranking** model on the candidate set.
- Final score = rerank score + recency + salience + graph proximity.
- Return top N with explanation (which signal matched).

4. **Explainability**
   - Provide a “memory trace” object:
     - `source`: FTS5 or ANN
     - `edges`: traversed relationships
     - `weights`: ranking factors

## Config Surface (JSON)
```json
{
  "memory": {
    "enabled": true,
   "sqlite_path": "./data/butterfly-bot.db",
    "lancedb_path": "./data/lancedb",
    "summary_model": "gpt-oss:20b",
   "embedding_model": "ollama:qwen3-embedding",
   "rerank_model": "ollama:qwen3-reranker",
    "retention_days": 365,
    "max_session_tokens": 24000,
    "graph": {
      "max_depth": 2,
      "edge_decay": 0.85
    }
  }
}
```

## Diesel Setup Plan
- Add Diesel + migrations with SQLite.
- Embedded migrations run at startup.
- Diesel models for each table and simple query helpers.
- Use `r2d2` + `spawn_blocking` for DB calls.

## LanceDB Setup Plan
- Add `lancedb` crate (Rust).
- Initialize local DB directory on startup.
- Upsert rows on memory insert.
- Query top‑K with cosine distance.

## Graphiti‑Style Memory Details
- Use a consistent schema for `Entity` → `Event` → `Fact` chains.
- Store edges with `edge_type` + `weight` + `created_at`.
- Provide a debug endpoint/CLI command to show graph neighbors.

## Safety & Privacy
- Local‑only by default; no remote DB connections.
- Optional encryption at rest (future phase).
- Redact secrets before storing in memory.

## Implementation Phases
1. **Phase A**: SQLite + Diesel schema + FTS5.
2. **Phase B**: LanceDB embeddings + ANN retrieval.
3. **Phase C**: Graph extraction + edge storage.
4. **Phase D**: Qwen3 reranker integration + explainability.
5. **Phase E**: Combined retrieval + scoring calibration.

## Deliverables
- Migrations + models
- Storage trait + SQLite implementation
- LanceDB integration
- Graph extraction + traversal
- Retrieval pipeline + scoring
- CLI command: `butterfly-bot memory debug`
