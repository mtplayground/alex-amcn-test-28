# alex-amcn-test-28

ZeroClaw initializes this repository as a monorepo for a Rust backend and a
web frontend.

## Repository Layout

- `server/`: Rust workspace member for backend services and shared server code.
- `frontend/`: web application workspace for the browser client.
- `migrations/`: SQL migration files for PostgreSQL schema changes.
- `seed/`: seed data and import assets used to initialize the application.

## Rust Workspace

The root `Cargo.toml` defines a workspace. The initial member is `server`,
which is intentionally minimal in issue #1 so later issues can add the HTTP
server and database integration without restructuring the repository.

## Environment

Copy `.env.example` and provide a PostgreSQL connection string through
`DATABASE_URL`. The backend also reads:

- `PORT`: optional, defaults to `8080`
- `SEED_ON_STARTUP`: optional boolean flag, defaults to `false`

## Getting Started

```bash
cargo build
```

This validates the Rust workspace layout and the `server` crate.

To run the HTTP server locally:

```bash
PORT=8080 cargo run -p zeroclaw-server
```

The server binds to `0.0.0.0:$PORT` and exposes `GET /api/health`, which
returns:

```json
{"status":"ok"}
```
