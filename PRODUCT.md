# Product Snapshot

`alex-amcn-test-28` is a monorepo for a graph-oriented web application. Today it contains a Rust backend, a frontend workspace, PostgreSQL schema/migrations, and an in-process mini-Cypher query engine that works over graph data stored in Postgres and mirrored into an in-memory graph index.

# What It Does

The backend runs an Axum server that serves a built SPA from `frontend/dist` and currently exposes `GET /api/health` under `/api`. Runtime configuration comes from `DATABASE_URL`, optional `PORT`, and optional `SEED_ON_STARTUP`.

The graph model is:

- `Node`: `id`, `labels`, and JSON properties
- `Relationship`: `id`, `type`, `start_id`, `end_id`, and JSON properties

PostgreSQL is the source of truth. At startup the server loads nodes and relationships into a `petgraph`-backed `GraphIndex`, and the query executor updates that index after graph mutations.

The query layer now supports a focused mini-Cypher subset:

- `CREATE` for nodes and directed relationships
- `MATCH` traversal over the in-memory graph
- `WHERE` boolean predicates over properties
- `RETURN` of bound entities or `var.prop` projections
- `LIMIT`
- `DELETE` and `DETACH DELETE`

# Current Features

- Axum backend with request tracing
- SPA static file serving with `index.html` fallback
- Health endpoint at `/api/health`
- PostgreSQL pool setup plus migrations for `nodes` and `relationships`
- Repository layer for node and relationship CRUD-style access
- In-memory `GraphIndex` with incident and outgoing relationship lookup
- Mini-Cypher lexer with line/column error reporting
- Recursive-descent parser producing AST nodes for graph patterns, predicates, projections, and limits
- `CREATE` executor with same-statement variable reuse and transactional writes
- `MATCH`/`WHERE`/`RETURN`/`LIMIT` executor over graph traversal
- `DELETE`/`DETACH DELETE` executor with database and index synchronization
- Real Postgres-backed tests covering repositories, graph index behavior, and query execution paths

# Architecture Decisions

- `server` is still the only Rust workspace member; the backend owns HTTP, domain, storage, graph index, parser, and executor concerns for now
- Postgres remains the source of truth; `GraphIndex` is a derived runtime structure used for traversal
- Query execution is split into stages: lexer -> parser -> executor
- Graph metadata stays flexible through JSONB properties plus explicit labels and relationship types
- The frontend is present in the repo, but the graph/query workflow is not yet exposed through HTTP endpoints beyond health

# Conventions

- Domain types live in `server/src/domain.rs`
- Database access lives in `server/src/db.rs`
- In-memory graph indexing lives in `server/src/graph.rs`
- Query parsing and execution live in `server/src/parser.rs`, `server/src/lexer.rs`, and `server/src/executor.rs`
- SQL schema changes go through `migrations/`
- `main` is the default branch
- `PRODUCT.md` should describe only merged state
