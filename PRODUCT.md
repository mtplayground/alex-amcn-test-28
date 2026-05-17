# Product Snapshot

`alex-amcn-test-28` is a small monorepo for a graph-oriented web application. It currently ships a Rust backend, a browser frontend, PostgreSQL schema/migrations, and seed scaffolding.

# What It Does

The backend runs an Axum server that serves a built single-page app from `frontend/dist` and exposes `GET /api/health` under `/api`. Runtime configuration comes from `DATABASE_URL`, optional `PORT`, and optional `SEED_ON_STARTUP`.

The data model is graph-shaped:

- `Node`: `id`, `labels`, and JSON properties
- `Relationship`: `id`, `type`, `start_id`, `end_id`, and JSON properties

PostgreSQL storage is in place for both through `NodeRepo` and `RelRepo`, and the backend now builds an in-memory `petgraph` index from those rows at startup.

The query layer has started taking shape: the server now includes a mini-Cypher lexer that tokenizes query text into parser-ready tokens with source positions.

# Current Features

- Axum backend with request tracing
- SPA static file serving with `index.html` fallback for client-side routes
- Health endpoint at `/api/health`
- PostgreSQL connection pool setup
- Migrations that create `nodes` and `relationships` tables plus indexes
- Repository operations for nodes:
  insert, get, list, delete, find by label + property
- Repository operations for relationships:
  insert, get, list, list by type, delete, delete by node
- In-memory `GraphIndex` backed by `petgraph::Graph<NodeId, RelId>`
- Startup graph load from PostgreSQL with node/relationship ID mappings
- Mini-Cypher lexer for identifiers, string/number/bool literals, keywords, and comparison/pattern punctuation
- Real Postgres-backed tests covering repository operations and graph-index sync after mutations
- Lexer unit tests covering tokenization, escaping, case-insensitive keywords, and error positions

# Architecture Decisions

- Monorepo layout with a Rust workspace root and `server` as the current workspace member
- Backend-first shape: HTTP server, domain types, and database layer are implemented before higher-level API routes for graph data
- Graph metadata is modeled flexibly with PostgreSQL `JSONB` properties and text labels/types
- The server treats PostgreSQL as the source of truth and derives an in-memory graph index from persisted rows on boot
- Query parsing is being introduced incrementally, starting with a standalone lexer module before parser and executor work
- Frontend and backend are intended to ship together, with the Rust server serving the built SPA in production

# Conventions

- Domain types live in `server/src/domain.rs`
- Database access lives in `server/src/db.rs`
- In-memory graph indexing lives in `server/src/graph.rs`
- Mini-Cypher lexing lives in `server/src/lexer.rs`
- SQL schema changes go through `migrations/`
- `main` is the default branch
- `PRODUCT.md` should describe only what is already merged
