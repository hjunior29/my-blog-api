# Blog API

A modular Rust HTTP backend with persistent SQLite storage.

## Stack

- Axum 0.8 and Tokio for HTTP and asynchronous execution.
- Serde and Axum's serde_json integration for typed JSON.
- SQLx 0.9 with only the SQLite driver and Tokio runtime enabled.
- Tower HTTP for request tracing; tracing-subscriber for structured JSON logs.

These are established, interoperable choices, not a claim of universally superior
performance. Benchmark representative workloads before replacing JSON parsers or
adding cache layers. SQLx executes SQLite operations on dedicated worker threads.

## Run

Install Rust 1.94 or newer, then run from this directory:

```sh
cargo run --bin my-blog-api
curl --fail http://127.0.0.1:3000/health
curl --fail http://127.0.0.1:3000/ready
```

Environment variables are read from the process. `.env.example` documents them;
`.env` files are not loaded automatically.

| Variable | Default | Meaning |
| --- | --- | --- |
| `BIND_ADDRESS` | `127.0.0.1:3000` | IP address and port |
| `DATABASE_URL` | `sqlite://blog.db` | SQLx SQLite connection URL |
| `DATABASE_MAX_CONNECTIONS` | `4` | Pool size, from 1 to 16 |
| `RUST_LOG` | `info` | tracing filter |
| `APP_ORIGIN` | `http://localhost:5173` | Exact browser origin for mutation checks |
| `OWNER_ONLY` | `true` | Restrict authentication to active owners |

The database file is created automatically. Its parent directory must exist and
be writable. Startup fails on connection or migration errors, before serving HTTP.
Use `sqlite::memory:` with one connection for isolated temporary tests.

## Architecture

```text
src/
  main.rs          Startup, logging, HTTP server and cleanup
  config.rs        Validated environment configuration
  database.rs      SQLite pool and embedded migrations
  shutdown.rs      SIGINT and SIGTERM handling
  http/            Router, error envelope and typed JSON extractor
  health/          Handler -> service -> repository
migrations/        Versioned SQL migrations embedded in the binary
tests/             HTTP and real SQLite integration tests
```

Organize future business features like `health`: handlers own HTTP concerns,
services own use cases, and repositories own SQL with bound parameters. Introduce
traits only when multiple implementations or a test boundary justify them. Shared
pool handles are cheap to clone and do not need another Arc or a global mutex.
Keep source, SQL, and documentation files below 500 lines. `Cargo.lock` is a
generated dependency manifest; it must stay intact regardless of its line count.

The initial migration establishes the migration history without creating a
speculative business schema. Add numbered SQL migrations for actual domain
features. Applied migrations are immutable. `build.rs` rebuilds the binary when
migration files change.

## HTTP contract

- `GET /health`: liveness, independent of database state.
- `GET /ready`: reads SQLite migration history; returns 503 when the pool is unavailable.
- Unknown routes and unsupported methods return JSON errors.
- Error envelope: `{"error":{"code":"not_found","message":"Route not found"}}`.
- Stable error codes can be translated by Portuguese and English clients.
- Handlers accepting JSON should use `http::json::Json<T>` for consistent
  rejection responses. DTOs should explicitly declare their schema with Serde.
- JSON extraction is limited to 64 KiB, including bodies without Content-Length.
- Handlers have a 10-second deadline. SQLite acquisition waits at most 3 seconds;
  lock contention waits at most 5 seconds.

Public `/api/v1/posts`, `/api/v1/posts/search`, `/api/v1/posts/{slug}` and
`/api/v1/tags` expose published content. `/api/v1/auth` handles cookie sessions;
`/api/v1/users/me` handles profiles and `/api/v1/admin/posts` handles editorial
writes with CSRF and version checks. Owner-only authentication is enabled by
default. No public registration endpoint exists. CORS is disabled: serve the
frontend and API behind a shared origin, including the Vite proxy locally. Request tracing is available with
`RUST_LOG=info,tower_http=debug`; avoid logging sensitive request data.

## SQLite and deployment

SQLite is bundled into the binary. The native binding is constrained to 0.37
(SQLite 3.51.3) to include the WAL-reset fix; a regression test checks the engine
version. Only the SQLite bundled feature is enabled, without extension loading.
Connections enable foreign keys, WAL, and synchronous FULL. FULL favors committed
data durability over maximum write throughput. The default pool is intentionally
small: SQLite allows concurrent readers but only one writer at a time. WAL is for
local storage, not a network filesystem shared across multiple instances.

Build with `cargo build --release --locked`. Run a single application instance with
a persistent writable volume, for example:

```sh
BIND_ADDRESS=0.0.0.0:3000 DATABASE_URL=sqlite:///data/blog.db ./target/release/my-blog-api
```

Configure a reverse proxy for TLS, connection limits, and header/body timeouts.
The application's handler deadline does not replace transport-level slow-client
protection. Use `/ready` for readiness and `/health` for liveness checks.
SIGINT/SIGTERM stop accepting requests, drain in-flight work, and close the pool.

Back up using SQLite's online backup facilities or `VACUUM INTO` and test restores.
Do not copy only the main database file while WAL writes are active. Store backups
outside the instance and provision monitoring for disk space. Ephemeral serverless
filesystems lose data; multi-instance writes require a different deployment or
storage strategy. No hosting provider or deployment has been configured here.

## Validation

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
```

Tests exercise persistent file storage, migration reuse, per-connection pragmas,
foreign-key enforcement, parameter binding, liveness/readiness, error sanitization,
JSON parsing, body limits, handler deadlines and migration checksum failures. They use temporary directories or isolated in-memory
connections and never mutate a developer database.

## References

- [Axum](https://docs.rs/axum/latest/axum/)
- [Serde](https://serde.rs/)
- [SQLx SQLite options](https://docs.rs/sqlx/latest/sqlx/sqlite/struct.SqliteConnectOptions.html)
- [SQLite WAL](https://www.sqlite.org/wal.html)
