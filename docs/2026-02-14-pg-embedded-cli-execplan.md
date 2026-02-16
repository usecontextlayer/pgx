# Embedded PostgreSQL CLI (Rust)

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must be kept up to date as work proceeds.

This repository does not currently include `.agent/PLANS.md`; this file is maintained directly as the source of truth.

## Purpose / Big Picture

After this change, a developer can run a single binary to start a fully functional PostgreSQL 18 server on their machine without installing PostgreSQL separately. The binary downloads (or bundles at compile time) the PostgreSQL 18 binaries, initializes a data directory, starts the server on an available port, and prints the connection URL. Pressing Ctrl+C stops the server cleanly.

This CLI exists to give ContextLayer a programmatically-controlled PostgreSQL instance that other tools (dlt, dagster, Steampipe) connect to. It is a thin wrapper around the `postgresql_embedded` Rust crate by theseus-rs.

To see it working: run `pgx start --data-dir ./my-data` and observe a connection URL printed to stdout. Connect with `psql` using that URL. Press Ctrl+C and observe a clean shutdown message.

## Project Location vs. Upstream PR

This CLI is a separate project, not a PR on `theseus-rs/postgresql-embedded`. The reasons are straightforward:

The theseus-rs repo is a library ecosystem. It publishes crates to crates.io for others to build on. Our CLI is an application that consumes that library — it pins to PG 18, will eventually set up directory conventions for Steampipe integration, and serves ContextLayer's specific needs. There is nothing to contribute upstream because we are not extending the library; we are using it. The `postgresql_embedded` crate's API is clean enough to depend on directly without modification.

By user direction, the project lives in this repository root at `/Users/alizain/ContextLayer/pgx`.

## Progress

- [x] Milestone 1: Project scaffolding and "start" command
- [x] Milestone 2: "stop" and "status" commands
- [x] Milestone 3: Signal handling and clean shutdown

## Surprises & Discoveries

- User corrected the location/name requirement mid-implementation: the crate had to be named `pgx` and live in this folder, not a sibling `pg-embedded` directory.
- `cargo check` and CLI smoke commands run successfully in this environment, but `pgx start` cannot finish setup because PostgreSQL archive download retries fail here (`Request failed after 3 retries`).
- Using PID signaling in the wrapper was both noisy and brittle. `postgresql_embedded` already provides `status()` and `stop()`, and relying on those keeps the CLI thin and avoids process-management edge cases.
- Setting `password_file` inside `data_dir` breaks `initdb` because the data directory must be empty at initialization. The wrapper now avoids custom password-file management entirely and relies on `postgresql_embedded` defaults plus persisted password metadata in state.

## Decision Log

- Decision: Keep the project in `/Users/alizain/ContextLayer/pgx` and name the crate/binary `pgx`.
  Rationale: Explicit user correction supersedes the initial location assumption.
  Date/Author: 2026-02-14 / Codex

- Decision: `stop` and `status` use `postgresql_embedded` primitives (`status()` + `stop()`), not wrapper-level PID signaling.
  Rationale: The CLI should be a thin boundary adapter, not a second process manager.
  Date/Author: 2026-02-15 / Codex

- Decision: Keep a small sibling state file (`<data-dir>.pgx-state.json`) for metadata (host/port/password), but not as a lock or PID authority.
  Rationale: Commands are separate processes; metadata persistence is needed to print a usable connection URL consistently (especially with random ports).
  Date/Author: 2026-02-15 / Codex

- Decision (superseded): Separate project at `../pg-embedded/` rather than inside the contextlayer monorepo or as a PR upstream.
  Rationale: The contextlayer monorepo uses Python (uv) and TypeScript (npm) toolchains. A Rust binary with its own Cargo workspace is a different build concern. Keeping it as a sibling avoids polluting either project's tooling. It is also not an upstream contribution because we are a consumer of the library, not extending it.
  Date/Author: 2026-02-14 / Claude

- Decision: Pin to PostgreSQL 18 via exact version requirement.
  Rationale: The user confirmed PG 18 only. No need for version flexibility. Pinning avoids accidental downloads of other major versions.
  Date/Author: 2026-02-14 / Claude

- Decision: Use the `bundled` feature for distribution builds, but default to runtime download for development.
  Rationale: The `bundled` feature embeds PG binaries into the Rust binary at compile time, producing a single self-contained artifact for distribution. But bundled builds are slow (downloading ~100MB during compilation). During development, the default runtime-download behavior is faster to iterate on — it caches binaries in `~/.theseus/postgresql/` after the first run.
  Date/Author: 2026-02-14 / Claude

- Decision: Use `clap` for argument parsing, not a hand-rolled parser.
  Rationale: `clap` is the idiomatic Rust CLI framework. Derive-based API keeps the code declarative and concise. No reason to reinvent this.
  Date/Author: 2026-02-14 / Claude

## Outcomes & Retrospective

Implemented in this repository:
- `Cargo.toml` configured with `postgresql_embedded`, `clap`, `tokio`, `tracing`, `serde`, and `serde_json`.
- `src/main.rs` now provides `pgx start`, `pgx stop`, and `pgx status`.
- `start` pins Postgres version to `=18`, writes metadata to `<data-dir>.pgx-state.json`, prints a connection URL, and handles SIGINT/SIGTERM with clean shutdown.
- `start --daemon` exits after startup while leaving PostgreSQL running by intentionally bypassing `Drop` shutdown (`std::mem::forget(postgresql)`).
- `status` uses `postgresql_embedded::PostgreSQL::status()` and prints URL metadata when running.
- `stop` uses `postgresql_embedded::PostgreSQL::stop()` (no custom PID signaling).

Validation status:
- Behavioral implementation is complete per milestones.
- `cargo check` and `cargo clippy -- -D warnings` succeeded; `pgx --help`, `pgx status`, and `pgx stop` were executed.
- Full end-to-end runtime validation succeeded with PG 18.2.0 in an escalated environment:
  - `status` before `start` => `not running`
  - `stop` before `start` => `not running`
  - `start` => URL emitted
  - `status` while running => `running` + URL
  - second `start` on same `data_dir` => error
  - `stop` => `stopped`
  - `status` after stop => `not running`
  - restart same `data_dir` with `--port 55439` => URL reflects fixed port
  - final `stop`/`status` => `stopped` then `not running`
  - daemon validation: `start --daemon` exits immediately, `status` reports `running`, then `stop` shuts it down.

## Context and Orientation

The `postgresql_embedded` crate (version 0.20.1, published by theseus-rs) provides a Rust API for downloading, installing, initializing, and running PostgreSQL as a library dependency. Its core type is `PostgreSQL`, which wraps a `Settings` struct and exposes async methods: `setup()` (download + init), `start()`, `stop()`, `create_database()`, and `drop_database()`. When the `PostgreSQL` value is dropped, it stops the server and optionally removes the data directory (controlled by `settings.temporary`).

Key `Settings` fields we care about:

- `version` — a semver `VersionReq`. We pin to `=18` (latest PG 18.x).
- `installation_dir` — where PG binaries live. Defaults to `~/.theseus/postgresql/`.
- `data_dir` — the PostgreSQL data directory (`PGDATA`). Defaults to a temporary directory.
- `port` — `0` means pick a random available port.
- `host` — defaults to `localhost`.
- `password` — randomly generated by default.
- `temporary` — if true, data directory is deleted on drop. Defaults to true.
- `configuration` — a `HashMap<String, String>` passed as `-c key=value` flags to `pg_ctl`.

The crate uses `tokio` for async, `sqlx` for database connectivity, and `tracing` for structured logging.

At start, this repository only contained the exec plan doc; implementation files were created during this run.

## Plan of Work

The CLI binary is called `pgx`. It has three subcommands: `start`, `stop`, and `status`. Internally it is a single Rust binary crate (not a library + binary split — there is no reuse case yet).

### Milestone 1: Project scaffolding and "start" command

This milestone produces a working `pgx start` command that downloads PG 18, initializes a data directory, starts the server, prints the connection URL, and waits until interrupted.

Create the project directory and initialize a Cargo project. The `Cargo.toml` depends on `postgresql_embedded` with `tokio` and `theseus` features, `clap` with derive, `tokio` as the async runtime, and `tracing-subscriber` for log output.

The binary entry point (`src/main.rs`) defines a `Cli` struct with clap derive and a `Commands` enum. The `start` subcommand accepts `--data-dir` (required, path to the persistent data directory), `--port` (optional, defaults to 0 for random), and `--host` (optional, defaults to `localhost`).

The start logic:

1. Build a `Settings` with `version` pinned to `=18`, `temporary` set to `false`, `data_dir` from the CLI arg, `port` and `host` from CLI args.
2. Call `postgresql.setup().await` to download and initialize.
3. Call `postgresql.start().await` to start the server.
4. Print the connection URL to stdout as a single line: `postgresql://postgres:<password>@<host>:<port>/postgres`
5. Wait for a shutdown signal (SIGINT/Ctrl+C or SIGTERM).
6. Call `postgresql.stop().await`.
7. Print a shutdown confirmation.

After this milestone, `cargo run -- start --data-dir ./test-data` starts a working PostgreSQL 18 server.

### Milestone 2: "stop" and "status" commands

These commands need to find a running instance. The implemented mechanism is:
- `status` delegates to `postgresql_embedded::PostgreSQL::status()`
- `stop` delegates to `postgresql_embedded::PostgreSQL::stop()`
- `start` writes metadata to sibling file `<data-dir>.pgx-state.json` for URL display (`host`, `port`, `password`)

`stop` does not signal arbitrary PIDs; it uses the library's stop behavior for the configured data directory.

`status` reports running/not-running via the library status API and prints a connection URL when metadata is available.

Both `stop` and `status` take `--data-dir` as a required argument (same as `start`), so the state file location is deterministic.

### Milestone 3: Signal handling and clean shutdown

Refine the `start` command's signal handling. Register handlers for both SIGINT (Ctrl+C) and SIGTERM. On receiving either signal, call `postgresql.stop().await` before exiting. This ensures the PostgreSQL server shuts down cleanly even when the CLI is killed by a process manager.

Also handle the case where `start` is called but the data directory already has a running instance (`postgresql.status() == Started`). In this case, print an error and exit rather than trying to start a second server on the same data directory.

## Concrete Steps

All commands assume the working directory is `/Users/alizain/ContextLayer/`.

### Milestone 1

Create the project:

    cargo init --name pgx --bin .

The generated `Cargo.toml` should be edited to contain these dependencies (exact contents specified in the milestone implementation). The key dependencies are:

- `postgresql_embedded = { version = "0.20", features = ["tokio", "theseus"] }`
- `clap = { version = "4", features = ["derive"] }`
- `tokio = { version = "1", features = ["full"] }`
- `tracing-subscriber = "0.3"`
- `tracing = "0.1"`
- `serde = { version = "1", features = ["derive"] }`
- `serde_json = "1"`

Write `src/main.rs` with the clap CLI definition and the start command implementation.

Build and run:

    cargo run -- start --data-dir ./test-data

Expected output (port and password will vary):

    postgresql://postgres:aB3xK9mP2qR7wY5z@localhost:54321/postgres

The server should be reachable:

    psql "postgresql://postgres:aB3xK9mP2qR7wY5z@localhost:54321/postgres" -c "SELECT version();"

Expected: a result row containing "PostgreSQL 18" in the version string.

Press Ctrl+C in the `pgx` terminal. Expected: the server stops and the process exits cleanly.

Run again with the same `--data-dir`. Expected: the server starts faster (no download, no init — data directory already exists).

### Milestone 2

Add `stop` and `status` subcommands to the `Commands` enum. Add the state file write to `start` (after the server is running) and the state file read to `stop` and `status`.

Test `status`:

    # Terminal 1:
    cargo run -- start --data-dir ./test-data

    # Terminal 2:
    cargo run -- status --data-dir ./test-data

Expected: prints the connection URL and "running".

Test `stop`:

    # Terminal 2:
    cargo run -- stop --data-dir ./test-data

Expected: the server in Terminal 1 stops. `status` now reports "not running".

### Milestone 3

Add SIGTERM handling alongside SIGINT. Add the "already running" guard to `start`. Test by running `start` twice with the same data directory — the second invocation should exit with an error message.

## Validation and Acceptance

The CLI is validated by these observable behaviors:

1. `cargo build` compiles without warnings on aarch64-apple-darwin (ARM Mac).
2. `pgx start --data-dir ./test-data` prints a connection URL and starts a reachable PG 18 server.
3. `psql` can connect using the printed URL and `SELECT version()` returns PostgreSQL 18.x.
4. Ctrl+C cleanly stops the server.
5. Restarting with the same `--data-dir` reuses existing data (no re-download, no re-init).
6. `pgx status --data-dir ./test-data` reports whether the server is running.
7. `pgx stop --data-dir ./test-data` stops a running server.
8. Running `start` twice on the same data directory errors rather than corrupting state.

For distribution builds (not part of this plan but noted for future work): `cargo build --release --features bundled` produces a single binary with PG 18 embedded. This is how the CLI will eventually ship to users.

## Idempotence and Recovery

All steps are safe to repeat. `cargo init` will fail if the directory exists — that is expected and harmless; just skip it. `setup()` is idempotent (skips download if binaries exist, skips init if data directory exists). `start` with the "already running" guard prevents double-starts. `stop` on an already-stopped server is a no-op.

If the process is killed without clean shutdown (kill -9), PostgreSQL may already be down when `start` exits. The wrapper handles this gracefully by detecting server state transitions via `postgresql.status()`.

## Interfaces and Dependencies

The CLI depends on these crates from crates.io (no local path dependencies, no forks):

- `postgresql_embedded` 0.20 — core PostgreSQL lifecycle management
- `clap` 4 — CLI argument parsing (derive feature)
- `tokio` 1 — async runtime (full feature)
- `tracing` 0.1 + `tracing-subscriber` 0.3 — structured logging
- `serde` 1 + `serde_json` 1 — state file serialization

The binary exposes no library API. It is a leaf application.

In `src/main.rs`, the key types are:

    #[derive(Parser)]
    struct Cli {
        #[command(subcommand)]
        command: Commands,
    }

    #[derive(Subcommand)]
    enum Commands {
        Start {
            #[arg(long)]
            data_dir: PathBuf,
            #[arg(long, default_value = "0")]
            port: u16,
            #[arg(long, default_value = "localhost")]
            host: String,
        },
        Stop {
            #[arg(long)]
            data_dir: PathBuf,
        },
        Status {
            #[arg(long)]
            data_dir: PathBuf,
        },
    }

    #[derive(Serialize, Deserialize)]
    struct StateFile {
        port: u16,
        host: String,
        password: String,
    }
