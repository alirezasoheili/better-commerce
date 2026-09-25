use std::{
    error::Error,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use better_commerce_core::{
    composition::compose_modules,
    database::{ReadinessDatabase, migrate_installation, scoped_role_names},
    http::router_with_readiness,
    manifest::{parse_and_validate, supported_release_metadata},
};
use sqlx::{PgPool, postgres::PgPoolOptions};
use tower::ServiceExt;
use url::Url;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;
static NEXT_ID: AtomicU64 = AtomicU64::new(0);
const MANIFEST: &str = "release: 0.1.0\ndeployment_mode: self_hosted\nmodules:\n  example:\n    version: 0.1.0\n    configuration:\n      label: Demo\n";

struct Fixture {
    admin: PgPool,
    operations_url: String,
    example_url: String,
    readiness_url: String,
    database: String,
    example_role: String,
    readiness_role: String,
}

impl Fixture {
    async fn new() -> TestResult<Self> {
        let admin_url = std::env::var("BC_TEST_ADMIN_DATABASE_URL")
            .expect("BC_TEST_ADMIN_DATABASE_URL must point to an isolated PostgreSQL test server");
        let admin = PgPoolOptions::new()
            .max_connections(2)
            .connect(&admin_url)
            .await?;
        let unique = format!(
            "{}_{}_{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        );
        let database = format!("bc_test_{unique}");
        let (example_role, readiness_role) = scoped_role_names(&database);
        let password = format!("test_{unique}");

        sqlx::query(&format!("CREATE DATABASE \"{database}\""))
            .execute(&admin)
            .await?;
        let mut url = Url::parse(&admin_url)?;
        url.set_path(&database);
        let operations_url = url.to_string();
        url.set_username(&example_role)
            .map_err(|_| "invalid test username")?;
        url.set_password(Some(&password))
            .map_err(|_| "invalid test password")?;
        let example_url = url.to_string();
        url.set_username(&readiness_role)
            .map_err(|_| "invalid test username")?;
        let readiness_url = url.to_string();
        Ok(Self {
            admin,
            operations_url,
            example_url,
            readiness_url,
            database,
            example_role,
            readiness_role,
        })
    }

    async fn migrate(&self) -> TestResult {
        migrate_installation(
            &self.operations_url,
            true,
            Some(&self.example_url),
            &self.readiness_url,
        )
        .await
    }

    async fn cleanup(self) -> TestResult {
        sqlx::query(&format!("DROP DATABASE \"{}\" WITH (FORCE)", self.database))
            .execute(&self.admin)
            .await?;
        for role in [&self.example_role, &self.readiness_role] {
            sqlx::query(&format!("DROP ROLE \"{role}\""))
                .execute(&self.admin)
                .await?;
        }
        self.admin.close().await;
        Ok(())
    }
}

async fn composition(
    runtime_url: &str,
) -> TestResult<better_commerce_core::composition::Composition> {
    let mut composition =
        compose_modules(parse_and_validate(MANIFEST, &supported_release_metadata())?)?;
    composition.attach_example_database(runtime_url).await?;
    Ok(composition)
}

async fn request_status(router: axum::Router, path: &str) -> TestResult<StatusCode> {
    Ok(router
        .oneshot(Request::builder().uri(path).body(Body::empty())?)
        .await?
        .status())
}

async fn status(
    composition: &better_commerce_core::composition::Composition,
    database: &ReadinessDatabase,
    path: &str,
) -> TestResult<StatusCode> {
    request_status(
        router_with_readiness(composition.clone(), Some(database.clone())),
        path,
    )
    .await
}

#[tokio::test]
async fn module_migrations_have_independent_histories_and_rerun_cleanly() -> TestResult {
    let fixture = Fixture::new().await?;
    fixture.migrate().await?;
    fixture.migrate().await?;
    let operations = PgPool::connect(&fixture.operations_url).await?;
    let example_versions: (i64,) =
        sqlx::query_as("SELECT count(*) FROM bc_example._sqlx_migrations")
            .fetch_one(&operations)
            .await?;
    let shared_versions: (i64,) = sqlx::query_as("SELECT count(*) FROM bc_shared._sqlx_migrations")
        .fetch_one(&operations)
        .await?;
    let table_exists: (bool,) =
        sqlx::query_as("SELECT to_regclass('bc_example.example_records') IS NOT NULL")
            .fetch_one(&operations)
            .await?;
    operations.close().await;
    fixture.cleanup().await?;
    assert_eq!(example_versions.0, 1);
    assert_eq!(shared_versions.0, 1);
    assert!(table_exists.0);
    Ok(())
}

#[tokio::test]
async fn module_permissions_reject_sibling_reads_and_mutations() -> TestResult {
    let fixture = Fixture::new().await?;
    fixture.migrate().await?;
    let operations = PgPool::connect(&fixture.operations_url).await?;
    sqlx::query("CREATE SCHEMA bc_test_sibling")
        .execute(&operations)
        .await?;
    sqlx::query("CREATE TABLE bc_test_sibling.secrets (id integer PRIMARY KEY)")
        .execute(&operations)
        .await?;
    let runtime = PgPool::connect(&fixture.example_url).await?;
    let own_read = sqlx::query("SELECT * FROM bc_example.example_records")
        .fetch_all(&runtime)
        .await;
    let sibling_read = sqlx::query("SELECT * FROM bc_test_sibling.secrets")
        .fetch_all(&runtime)
        .await;
    let sibling_write = sqlx::query("INSERT INTO bc_test_sibling.secrets (id) VALUES (1)")
        .execute(&runtime)
        .await;
    let shared_read = sqlx::query("SELECT * FROM bc_shared._sqlx_migrations")
        .fetch_all(&runtime)
        .await;
    sqlx::query(&format!(
        "GRANT USAGE ON SCHEMA bc_test_sibling TO \"{}\"",
        fixture.example_role
    ))
    .execute(&operations)
    .await?;
    let overgranted_role_rejected = fixture.migrate().await.is_err();
    sqlx::query(&format!(
        "REVOKE USAGE ON SCHEMA bc_test_sibling FROM \"{}\"",
        fixture.example_role
    ))
    .execute(&operations)
    .await?;
    runtime.close().await;
    operations.close().await;
    fixture.cleanup().await?;
    assert!(own_read.is_ok());
    assert!(overgranted_role_rejected);
    for denied in [sibling_read.err(), sibling_write.err(), shared_read.err()] {
        let error = denied.expect("cross-schema SQL must fail");
        assert_eq!(
            error
                .as_database_error()
                .and_then(|db| db.code())
                .as_deref(),
            Some("42501")
        );
    }
    Ok(())
}

#[tokio::test]
async fn module_permissions_reject_role_reuse_across_installations() -> TestResult {
    let fixture = Fixture::new().await?;
    fixture.migrate().await?;
    let other_database = format!("{}_other", fixture.database);
    sqlx::query(&format!("CREATE DATABASE \"{other_database}\""))
        .execute(&fixture.admin)
        .await?;
    let mut operations = Url::parse(&fixture.operations_url)?;
    operations.set_path(&other_database);
    let mut example = Url::parse(&fixture.example_url)?;
    example.set_path(&other_database);
    example
        .set_password(Some("different_test_password"))
        .map_err(|_| "invalid test password")?;
    let mut readiness = Url::parse(&fixture.readiness_url)?;
    readiness.set_path(&other_database);
    readiness
        .set_password(Some("different_test_password"))
        .map_err(|_| "invalid test password")?;
    let second_installation_rejected = migrate_installation(
        operations.as_str(),
        true,
        Some(example.as_str()),
        readiness.as_str(),
    )
    .await
    .is_err();
    let original_credentials_valid = PgPool::connect(&fixture.example_url).await;
    if let Ok(pool) = &original_credentials_valid {
        pool.close().await;
    }
    sqlx::query(&format!("DROP DATABASE \"{other_database}\" WITH (FORCE)"))
        .execute(&fixture.admin)
        .await?;
    fixture.cleanup().await?;
    assert!(second_installation_rejected);
    assert!(original_credentials_valid.is_ok());
    Ok(())
}

#[tokio::test]
async fn module_permissions_reject_role_owning_another_database() -> TestResult {
    let fixture = Fixture::new().await?;
    fixture.migrate().await?;
    let other_database = format!("{}_owned", fixture.database);
    sqlx::query(&format!(
        "CREATE DATABASE \"{other_database}\" OWNER \"{}\"",
        fixture.example_role
    ))
    .execute(&fixture.admin)
    .await?;
    let owner_rejected = fixture.migrate().await.is_err();
    sqlx::query(&format!("DROP DATABASE \"{other_database}\" WITH (FORCE)"))
        .execute(&fixture.admin)
        .await?;
    fixture.cleanup().await?;
    assert!(owner_rejected);
    Ok(())
}

#[tokio::test]
async fn readiness_checks_both_histories_and_connectivity_while_healthz_stays_live() -> TestResult {
    let fixture = Fixture::new().await?;
    let statuses: TestResult<[StatusCode; 7]> = async {
        let database = ReadinessDatabase::connect(&fixture.readiness_url).await?;
        let composition = composition(&fixture.example_url).await?;
        let before = status(&composition, &database, "/readyz").await?;
        let health_before = status(&composition, &database, "/healthz").await?;
        fixture.migrate().await?;
        let migrated = status(&composition, &database, "/readyz").await?;
        let operations = PgPool::connect(&fixture.operations_url).await?;
        sqlx::query("UPDATE bc_example._sqlx_migrations SET success = false")
            .execute(&operations)
            .await?;
        let bad_example = status(&composition, &database, "/readyz").await?;
        sqlx::query("UPDATE bc_example._sqlx_migrations SET success = true")
            .execute(&operations)
            .await?;
        sqlx::query("UPDATE bc_shared._sqlx_migrations SET success = false")
            .execute(&operations)
            .await?;
        let bad_shared = status(&composition, &database, "/readyz").await?;
        sqlx::query("UPDATE bc_shared._sqlx_migrations SET success = true")
            .execute(&operations)
            .await?;
        operations.close().await;
        database.close().await;
        composition.close_databases().await;
        let disconnected = status(&composition, &database, "/readyz").await?;
        let health_after = status(&composition, &database, "/healthz").await?;
        Ok([
            before,
            health_before,
            migrated,
            bad_example,
            bad_shared,
            disconnected,
            health_after,
        ])
    }
    .await;
    fixture.cleanup().await?;
    assert_eq!(
        statuses?,
        [
            StatusCode::SERVICE_UNAVAILABLE,
            StatusCode::OK,
            StatusCode::OK,
            StatusCode::SERVICE_UNAVAILABLE,
            StatusCode::SERVICE_UNAVAILABLE,
            StatusCode::SERVICE_UNAVAILABLE,
            StatusCode::OK,
        ]
    );
    Ok(())
}
