use clap::{Args, Parser, Subcommand};
use postgresql_embedded::{PostgreSQL, Settings, Status, VersionReq};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process;
use tokio::time::{Duration, interval};
use tracing_subscriber::EnvFilter;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

type AppResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

const PG_VERSION_REQ: &str = "=18";
const PGX_DATA_DIR_ENV: &str = "PGX_DATA_DIR";
const CONNECTION_DETAILS_UNAVAILABLE_ERROR: &str =
    "connection details unavailable (missing state or password metadata)";

#[derive(Debug, Parser)]
#[command(name = "pgx")]
#[command(about = "Run embedded PostgreSQL 18 locally.")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Start(StartArgs),
    Stop(DataDirArgs),
    Status(DataDirArgs),
    Url(DataDirArgs),
}

#[derive(Debug, Args)]
struct StartArgs {
    #[arg(long)]
    data_dir: Option<PathBuf>,
    #[arg(long, default_value_t = 0)]
    port: u16,
    #[arg(long, default_value = "localhost")]
    host: String,
    #[arg(long, default_value_t = false)]
    daemon: bool,
}

#[derive(Debug, Args)]
struct DataDirArgs {
    #[arg(long)]
    data_dir: Option<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct StateFile {
    port: u16,
    host: String,
}

struct RuntimeConnectionDetails {
    host: String,
    port: u16,
    password: String,
}

struct RuntimeContext {
    connection: RuntimeConnectionDetails,
    postgresql: PostgreSQL,
}

enum ShutdownOutcome {
    Signal,
    ServerStopped,
}

#[tokio::main]
async fn main() {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn,pgx=info"));
    tracing_subscriber::fmt().with_env_filter(env_filter).init();

    let cli = Cli::parse();
    let result = match cli.command {
        Commands::Start(args) => handle_start(args).await,
        Commands::Stop(args) => handle_stop(args).await,
        Commands::Status(args) => handle_status(args).await,
        Commands::Url(args) => handle_url(args).await,
    };

    if let Err(error) = result {
        eprintln!("error: {error}");
        process::exit(1);
    }
}

async fn handle_start(args: StartArgs) -> AppResult<()> {
    let data_dir = resolve_data_dir(args.data_dir)?;
    fs::create_dir_all(&data_dir)?;

    let password = resolve_start_password(&data_dir)?;
    let mut postgresql = PostgreSQL::new(build_settings(
        &data_dir,
        Some(args.host),
        Some(args.port),
        password,
    )?);

    if postgresql.status() == Status::Started {
        return Err(
            io::Error::other(format!("server already running for {}", data_dir.display())).into(),
        );
    }

    postgresql.setup().await?;
    postgresql.start().await?;

    let running = postgresql.settings();
    let password = managed_password_for_connection(&data_dir, running)?;
    let state = StateFile {
        host: running.host.clone(),
        port: running.port,
    };
    write_state_file(&data_dir, &state)?;
    println!("{}", connection_url(&running.host, running.port, &password));

    if args.daemon {
        std::mem::forget(postgresql);
        return Ok(());
    }

    let shutdown_outcome = wait_for_shutdown_signal_or_server_stop(&postgresql).await?;
    let should_stop = matches!(shutdown_outcome, ShutdownOutcome::Signal)
        && postgresql.status() == Status::Started;

    if should_stop {
        postgresql.stop().await?;
        println!("PostgreSQL stopped cleanly.");
    } else {
        println!("PostgreSQL is no longer running.");
    }

    Ok(())
}

async fn handle_stop(args: DataDirArgs) -> AppResult<()> {
    let mut runtime = load_runtime_context(args.data_dir)?;

    if runtime.postgresql.status() != Status::Started {
        println!("not running");
        return Ok(());
    }

    runtime.postgresql.setup().await?;
    runtime.postgresql.stop().await?;
    println!("stopped");
    Ok(())
}

async fn handle_status(args: DataDirArgs) -> AppResult<()> {
    let runtime = load_runtime_context(args.data_dir)?;

    if runtime.postgresql.status() == Status::Started {
        println!("running");
        println!(
            "{}",
            connection_url(
                &runtime.connection.host,
                runtime.connection.port,
                &runtime.connection.password
            )
        );
        return Ok(());
    }

    println!("not running");
    Ok(())
}

async fn handle_url(args: DataDirArgs) -> AppResult<()> {
    let runtime = load_runtime_context(args.data_dir)?;

    if runtime.postgresql.status() != Status::Started {
        return Err(io::Error::other("not running").into());
    }

    println!(
        "{}",
        connection_url(
            &runtime.connection.host,
            runtime.connection.port,
            &runtime.connection.password
        )
    );
    Ok(())
}

fn resolve_data_dir(cli_data_dir: Option<PathBuf>) -> AppResult<PathBuf> {
    if let Some(env_data_dir_raw) = std::env::var_os(PGX_DATA_DIR_ENV) {
        if env_data_dir_raw.is_empty() {
            return Err(io::Error::other(format!("{PGX_DATA_DIR_ENV} is set but empty")).into());
        }

        let env_data_dir = PathBuf::from(env_data_dir_raw);
        if let Some(cli_data_dir) = cli_data_dir
            && cli_data_dir != env_data_dir
        {
            eprintln!("warning: --data-dir is ignored because PGX_DATA_DIR is set");
        }

        return Ok(env_data_dir);
    }

    if let Some(cli_data_dir) = cli_data_dir {
        return Ok(cli_data_dir);
    }

    Err(io::Error::other(format!(
        "missing data directory: set {PGX_DATA_DIR_ENV} or pass --data-dir"
    ))
    .into())
}

fn metadata_error() -> io::Error {
    io::Error::other(CONNECTION_DETAILS_UNAVAILABLE_ERROR)
}

fn load_runtime_connection_details(data_dir: &Path) -> AppResult<RuntimeConnectionDetails> {
    let state = read_state_file(data_dir).map_err(|_| metadata_error())?;
    let password = read_managed_password_file(data_dir).map_err(|_| metadata_error())?;

    let state = state.ok_or_else(metadata_error)?;
    let password = password.ok_or_else(metadata_error)?;

    Ok(RuntimeConnectionDetails {
        host: state.host,
        port: state.port,
        password,
    })
}

fn load_runtime_context(cli_data_dir: Option<PathBuf>) -> AppResult<RuntimeContext> {
    let data_dir = resolve_data_dir(cli_data_dir)?;
    let connection = load_runtime_connection_details(&data_dir)?;
    let settings = build_settings(
        &data_dir,
        Some(connection.host.clone()),
        Some(connection.port),
        Some(connection.password.clone()),
    )?;

    Ok(RuntimeContext {
        connection,
        postgresql: PostgreSQL::new(settings),
    })
}

#[cfg(unix)]
async fn wait_for_shutdown_signal_or_server_stop(
    postgresql: &PostgreSQL,
) -> AppResult<ShutdownOutcome> {
    use tokio::signal::unix::{SignalKind, signal};

    let mut ticker = interval(Duration::from_millis(250));
    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    loop {
        tokio::select! {
            _ = sigint.recv() => return Ok(ShutdownOutcome::Signal),
            _ = sigterm.recv() => return Ok(ShutdownOutcome::Signal),
            _ = ticker.tick() => {
                if postgresql.status() != Status::Started {
                    return Ok(ShutdownOutcome::ServerStopped);
                }
            }
        }
    }
}

#[cfg(not(unix))]
async fn wait_for_shutdown_signal_or_server_stop(
    postgresql: &PostgreSQL,
) -> AppResult<ShutdownOutcome> {
    let mut ticker = interval(Duration::from_millis(250));

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => return Ok(ShutdownOutcome::Signal),
            _ = ticker.tick() => {
                if postgresql.status() != Status::Started {
                    return Ok(ShutdownOutcome::ServerStopped);
                }
            }
        }
    }
}

fn build_settings(
    data_dir: &Path,
    host: Option<String>,
    port: Option<u16>,
    password: Option<String>,
) -> AppResult<Settings> {
    let mut settings = Settings {
        version: VersionReq::parse(PG_VERSION_REQ)?,
        data_dir: data_dir.to_path_buf(),
        password_file: password_file_path(data_dir),
        temporary: false,
        ..Settings::default()
    };

    if let Some(host) = host {
        settings.host = host;
    }
    if let Some(port) = port {
        settings.port = port;
    }

    if let Some(password) = password
        && !password.trim().is_empty()
    {
        settings.password = password;
    }

    Ok(settings)
}

fn sidecar_file_path(data_dir: &Path, suffix: &str) -> PathBuf {
    let parent = data_dir.parent().unwrap_or_else(|| Path::new("."));
    let base = data_dir
        .file_name()
        .unwrap_or_else(|| OsStr::new("pgx-data"))
        .to_string_lossy();

    parent.join(format!("{base}.{suffix}"))
}

fn state_file_path(data_dir: &Path) -> PathBuf {
    sidecar_file_path(data_dir, "pgx-state.json")
}

fn password_file_path(data_dir: &Path) -> PathBuf {
    sidecar_file_path(data_dir, "pgx-password")
}

fn read_state_file(data_dir: &Path) -> AppResult<Option<StateFile>> {
    let state_path = state_file_path(data_dir);
    if !state_path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(state_path)?;
    let state = serde_json::from_str::<StateFile>(&raw)?;
    Ok(Some(state))
}

fn write_state_file(data_dir: &Path, state: &StateFile) -> AppResult<()> {
    let state_path = state_file_path(data_dir);
    let raw = serde_json::to_string_pretty(state)?;
    fs::write(state_path, raw)?;
    Ok(())
}

fn read_managed_password_file(data_dir: &Path) -> AppResult<Option<String>> {
    let password_path = password_file_path(data_dir);
    if !password_path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(password_path)?;
    let password = raw.trim().to_string();
    if password.is_empty() {
        return Ok(None);
    }

    Ok(Some(password))
}

#[cfg(unix)]
fn set_password_file_permissions(path: &Path) -> AppResult<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_password_file_permissions(_path: &Path) -> AppResult<()> {
    Ok(())
}

fn cluster_is_initialized(data_dir: &Path) -> bool {
    data_dir.join("postgresql.conf").exists()
}

fn resolve_start_password(data_dir: &Path) -> AppResult<Option<String>> {
    if let Some(password) = read_managed_password_file(data_dir)? {
        return Ok(Some(password));
    }

    if cluster_is_initialized(data_dir) {
        return Err(io::Error::other(format!(
            "missing managed password file for initialized data directory {}. reset the postgres password and write it to {}",
            data_dir.display(),
            password_file_path(data_dir).display(),
        ))
        .into());
    }

    Ok(None)
}

fn managed_password_for_connection(data_dir: &Path, running: &Settings) -> AppResult<String> {
    let password = read_managed_password_file(data_dir)?
        .ok_or_else(|| io::Error::other("managed password file missing after startup"))?;
    if running.password.trim().is_empty() {
        return Err(io::Error::other("database started with an empty password").into());
    }
    set_password_file_permissions(&password_file_path(data_dir))?;
    Ok(password)
}

fn connection_url(host: &str, port: u16, password: &str) -> String {
    format!(
        "postgresql://postgres:{}@{}:{}/postgres",
        password, host, port
    )
}
