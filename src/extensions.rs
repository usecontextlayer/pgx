use async_trait::async_trait;
use postgresql_archive::extractor::{zip_extract, ExtractDirectories};
use postgresql_extensions::repository::model::Repository;
use regex_lite::Regex;
use semver::{Version, VersionReq};
use std::path::PathBuf;

const REPO_URL: &str = "https://github.com/usecontextlayer/pg_search_compiled";

#[derive(Debug)]
pub struct PgSearchRepository;

impl PgSearchRepository {
    fn new() -> postgresql_extensions::Result<Box<dyn Repository>> {
        Ok(Box::new(Self))
    }
}

fn supports(url: &str) -> postgresql_archive::Result<bool> {
    Ok(url.starts_with(REPO_URL))
}

/// Register our custom repository in both the postgresql_archive and
/// postgresql_extensions registries. Must be called once before install().
pub fn initialize() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    postgresql_archive::matcher::registry::register(
        supports,
        postgresql_extensions::zip_matcher,
    )?;
    postgresql_archive::repository::registry::register(
        supports,
        Box::new(|url: &str| {
            postgresql_archive::repository::github::repository::GitHub::new(url)
        }),
    )?;
    postgresql_extensions::repository::registry::register(
        "contextlayer",
        Box::new(|| PgSearchRepository::new()),
    )?;
    Ok(())
}

/// Install pg_search into the PostgreSQL installation managed by the given settings.
pub async fn install_pg_search(
    settings: &postgresql_embedded::Settings,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let version_req = VersionReq::parse(">=0.17, <0.18")?;
    postgresql_extensions::install(settings, "contextlayer", "pg_search", &version_req).await?;
    Ok(())
}

/// Connect to the running database and enable pg_search.
pub async fn enable_pg_search(
    settings: &postgresql_embedded::Settings,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let database_url = settings.url("postgres");
    let pool = sqlx::postgres::PgPool::connect(&database_url).await?;
    sqlx::query("CREATE EXTENSION IF NOT EXISTS pg_search")
        .execute(&pool)
        .await?;
    pool.close().await;
    Ok(())
}

#[async_trait]
impl Repository for PgSearchRepository {
    fn name(&self) -> &str {
        "contextlayer"
    }

    async fn get_available_extensions(
        &self,
    ) -> postgresql_extensions::Result<Vec<postgresql_extensions::AvailableExtension>> {
        Ok(vec![postgresql_extensions::AvailableExtension::new(
            "contextlayer",
            "pg_search",
            "ParadeDB pg_search — BM25 full-text search for PostgreSQL",
        )])
    }

    async fn get_archive(
        &self,
        postgresql_version: &str,
        _name: &str,
        version: &VersionReq,
    ) -> postgresql_extensions::Result<(Version, Vec<u8>)> {
        let url = format!("{}?postgresql_version={}", REPO_URL, postgresql_version);
        let (version, bytes) = postgresql_archive::get_archive(&url, version).await?;
        Ok((version, bytes))
    }

    async fn install(
        &self,
        _name: &str,
        library_dir: PathBuf,
        extension_dir: PathBuf,
        archive: &[u8],
    ) -> postgresql_extensions::Result<Vec<PathBuf>> {
        let archive_vec = archive.to_vec();
        let mut dirs = ExtractDirectories::default();
        dirs.add_mapping(Regex::new(r"\.(dll|dylib|so)$")?, library_dir);
        dirs.add_mapping(Regex::new(r"\.(control|sql)$")?, extension_dir);
        let files = zip_extract(&archive_vec, &dirs)?;
        Ok(files)
    }
}
