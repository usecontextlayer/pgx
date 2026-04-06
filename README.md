<div align="center">

# pgx

Single-binary PostgreSQL 17 with built-in full-text search. No installation, no Docker, no setup.

[![CI](https://github.com/usecontextlayer/pgx/actions/workflows/release.yml/badge.svg)](https://github.com/usecontextlayer/pgx/actions/workflows/release.yml) [![npm](https://img.shields.io/npm/v/@usecontextlayer/pgx)](https://www.npmjs.com/package/@usecontextlayer/pgx) [![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/usecontextlayer/pgx/blob/main/LICENSE)

</div>

```bash
# Start PostgreSQL — downloads the binary on first run, then starts in seconds
pgx start --data-dir ./my-data
# => postgresql://postgres:secret@localhost:5432/postgres

# BM25 full-text search is already enabled — no extensions to install
psql $(pgx url --data-dir ./my-data)
```

```sql
-- pg_search is built in. Create a BM25 index and query it immediately.
CREATE INDEX idx_docs ON docs USING bm25 (title, body);
SELECT * FROM docs WHERE docs @@@ 'distributed systems' LIMIT 10;
```

- **Full Postgres, zero infrastructure** — downloads and runs a real PostgreSQL 17 server via [postgresql-embedded](https://github.com/theseus-rs/postgresql-embedded). No Docker, no Homebrew postgres, no cloud account.
- **BM25 search out of the box** — ships with [pg_search](https://github.com/paradedb/paradedb) from [ParadeDB](https://www.paradedb.com/), giving you ranked full-text search as a Postgres index. No Elasticsearch sidecar.
- **One data directory, portable state** — all data, config, and passwords live in `--data-dir`. Move it, back it up, delete it. Nothing global to clean up.
- **Foreground or daemon** — runs in the foreground by default (Ctrl-C to stop). Pass `--daemon` to background it, then `pgx stop` when you're done.
- **Cross-platform** — macOS (ARM, x86), Linux (ARM, x86), Windows.

## Installation

```bash
# npm
npm install -g @usecontextlayer/pgx
```

```bash
# Homebrew
brew install usecontextlayer/pgx/pgx
```

```bash
# Shell (macOS, Linux)
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/usecontextlayer/pgx/releases/latest/download/pgx-installer.sh | sh
```

```powershell
# PowerShell (Windows)
powershell -ExecutionPolicy ByPass -c "irm https://github.com/usecontextlayer/pgx/releases/latest/download/pgx-installer.ps1 | iex"
```

## Usage

### As a CLI

```bash
# Start on a specific port
pgx start --data-dir ./my-data --port 5433

# Start as a background daemon
pgx start --data-dir ./my-data --daemon

# Check if it's running
pgx status --data-dir ./my-data
# => running
# => postgresql://postgres:secret@localhost:5433/postgres

# Get just the connection URL
pgx url --data-dir ./my-data

# Stop a running instance
pgx stop --data-dir ./my-data
```

Set `PGX_DATA_DIR` to skip `--data-dir` on every command:

```bash
export PGX_DATA_DIR=./my-data
pgx start
pgx status
pgx stop
```

### In your code

pgx gives you a standard Postgres connection URL. Use it with any client library.

```javascript
// Node.js — use the URL from pgx start or pgx url
import postgres from "postgres";
const sql = postgres(process.env.DATABASE_URL);

// BM25 search — works like any other query
const results = await sql`
  SELECT title, body FROM docs WHERE docs @@@ ${query} LIMIT 10
`;
```

```python
# Python
import psycopg2
conn = psycopg2.connect(os.environ["DATABASE_URL"])
cur = conn.cursor()
cur.execute("SELECT * FROM docs WHERE docs @@@ %s LIMIT 10", (query,))
```

## How it works

pgx wraps [postgresql-embedded](https://github.com/theseus-rs/postgresql-embedded) (from [theseus-rs](https://github.com/theseus-rs)) to download, configure, and run a real PostgreSQL 17 binary. On first `pgx start`, it fetches the correct binary for your platform, initializes a cluster in your data directory, installs the pg_search extension, and starts the server. Subsequent starts reuse the existing data directory and skip the download.

Passwords are auto-generated and stored in a sidecar file next to the data directory (mode 0600 on Unix). State is tracked in a JSON sidecar so that `pgx stop`, `pgx status`, and `pgx url` can reconnect to a running instance without global state.

MIT License
