# Product Snapshot

`alex-amcn-test-28` is a monorepo for a graph-oriented web application. The merged backend is a Rust/Axum service backed by PostgreSQL, with an in-process mini-Cypher engine and an in-memory graph index used for traversal.

# What It Does

The backend serves a built SPA from `frontend/dist` and exposes a small graph API under `/api`. Runtime configuration comes from `DATABASE_URL`, optional `PORT`, and optional `SEED_ON_STARTUP`.

The graph model is:

- `Node`: `id`, `labels`, and JSON properties
- `Relationship`: `id`, `type`, `start_id`, `end_id`, and JSON properties

PostgreSQL is the source of truth. At startup the server loads nodes and relationships into a `petgraph`-backed `GraphIndex`, and query-side mutations keep that index synchronized.

The mini-Cypher layer supports:

- `CREATE` for nodes and directed relationships
- `MATCH` traversal over the in-memory graph
- `WHERE` boolean predicates over properties
- `RETURN` of bound entities or `var.prop` projections
- `LIMIT`
- `DELETE` and `DETACH DELETE`

# Current Features

- Axum backend with request tracing
- SPA static file serving with `index.html` fallback
- `GET /api/health`
- `GET /api/graph` returning `{ nodes, relationships }` with optional `?limit=`
- `POST /api/query` accepting `{ query }` and returning `QueryResult` or structured `{ error: { message, line, column } }`
- PostgreSQL pool setup plus migrations for `nodes` and `relationships`
- Repository layer for node and relationship CRUD-style access
- In-memory `GraphIndex` with incident and outgoing relationship lookup
- Mini-Cypher lexer, parser, and executor
- Real Postgres-backed tests covering repositories, graph index behavior, query execution, and HTTP API paths

# Architecture Decisions

- `server` is still the only Rust workspace member; the backend owns HTTP, domain, storage, graph index, parser, and executor concerns
- Postgres remains the source of truth; `GraphIndex` is a derived runtime structure used for traversal and query execution
- The HTTP app shares database state plus a mutex-protected `GraphIndex` so mutation queries can keep in-memory traversal state aligned with persisted data
- Query execution is split into stages: lexer -> parser -> executor
- Graph metadata stays flexible through JSONB properties plus explicit labels and relationship types
- The frontend exists in the repo, but current graph interactions are exposed primarily through backend API endpoints rather than a completed browser workflow

# Conventions

- Domain types live in `server/src/domain.rs`
- Database access lives in `server/src/db.rs`
- In-memory graph indexing lives in `server/src/graph.rs`
- Query parsing and execution live in `server/src/parser.rs`, `server/src/lexer.rs`, and `server/src/executor.rs`
- HTTP routes are assembled in `server/src/lib.rs`
- SQL schema changes go through `migrations/`
- `main` is the default branch
- `PRODUCT.md` should describe only merged state
