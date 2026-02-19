# pgx

Single-binary CLI that runs an embedded PostgreSQL 18 server locally. No PostgreSQL installation required.

Built on [`postgresql_embedded`](https://crates.io/crates/postgresql_embedded) by theseus-rs. On first run, PG binaries are downloaded and cached to `~/.theseus/postgresql/`. Subsequent starts reuse the cache.

## Usage

```bash
# Start a server (prints connection URL to stdout)
pgx start --data-dir ./my-data

# Check if a server is running
pgx status --data-dir ./my-data

# Print only the connection URL (fails if not running)
pgx url --data-dir ./my-data

# Stop a running server
pgx stop --data-dir ./my-data
```

You can also set `PGX_DATA_DIR` instead of passing `--data-dir`.
`PGX_DATA_DIR` takes precedence when both are provided.

### Start options

| Flag | Default | Description |
|------|---------|-------------|
| `--data-dir` | *(optional)* | Path to the PostgreSQL data directory (overridden by `PGX_DATA_DIR`) |
| `--port` | `0` (random) | Port to listen on |
| `--host` | `localhost` | Host to bind to |
| `--daemon` | `false` | Exit after startup, leaving the server running |

Ctrl+C (or SIGTERM) triggers a clean shutdown when running in the foreground.

## Build

```bash
cargo build --release
```

The binary is at `target/release/pgx`.

## State files

Alongside the data directory, `pgx` writes two sidecar files:

- `<data-dir>.pgx-state.json` — host and port metadata
- `<data-dir>.pgx-password` — managed password (chmod 600 on unix)

These let `status`, `stop`, and `url` work from separate processes.
