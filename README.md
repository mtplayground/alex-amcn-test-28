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

## Getting Started

```bash
cargo build
```

This validates the Rust workspace layout and the `server` crate scaffold.
