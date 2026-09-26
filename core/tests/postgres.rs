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
    dispatcher::{
        DispatchOutcome, EventDelivery, InProcessEventDelivery, PostgresOutboxDispatcher,
    },
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
    dispatcher_url: String,
    readiness_url: String,
    database: String,
    example_role: String,
    dispatcher_role: String,
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
        let (example_role, dispatcher_role, readiness_role) = scoped_role_names(&database);
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
        url.set_username(&dispatcher_role)
            .map_err(|_| "invalid test username")?;
        url.set_password(Some(&password))
            .map_err(|_| "invalid test password")?;
        let dispatcher_url = url.to_string();
        url.set_username(&readiness_role)
            .map_err(|_| "invalid test username")?;
        let readiness_url = url.to_string();
        Ok(Self {
            admin,
            operations_url,
            example_url,
            dispatcher_url,
            readiness_url,
            database,
            example_role,
            dispatcher_role,
            readiness_role,
        })
    }

    async fn migrate(&self) -> TestResult {
        migrate_installation(
            &self.operations_url,
            true,
            Some(&self.example_url),
            Some(&self.dispatcher_url),
            &self.readiness_url,
        )
        .await
    }

    async fn cleanup(self) -> TestResult {
        sqlx::query(&format!("DROP DATABASE \"{}\" WITH (FORCE)", self.database))
            .execute(&self.admin)
            .await?;
        for role in [
            &self.example_role,
            &self.dispatcher_role,
            &self.readiness_role,
        ] {
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
    assert_eq!(example_versions.0, 2);
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
        Some(fixture.dispatcher_url.as_str()),
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
async fn outbox_atomicity_commits_business_state_and_event_together() -> TestResult {
    let fixture = Fixture::new().await?;
    fixture.migrate().await?;
    let composition = composition(&fixture.example_url).await?;
    assert!(
        composition
            .create_example_record("atomic commit".into())
            .await
            .expect("example module is composed")
            .is_ok()
    );
    let operations = PgPool::connect(&fixture.operations_url).await?;
    let state_count: (i64,) = sqlx::query_as("SELECT count(*) FROM bc_example.example_records")
        .fetch_one(&operations)
        .await?;
    let event_count: (i64,) = sqlx::query_as("SELECT count(*) FROM bc_example.outbox_events")
        .fetch_one(&operations)
        .await?;
    operations.close().await;
    composition.close_databases().await;
    fixture.cleanup().await?;
    assert_eq!(state_count.0, 1);
    assert_eq!(event_count.0, 1);
    Ok(())
}

#[tokio::test]
async fn outbox_atomicity_rolls_back_both_writes_when_event_insert_fails() -> TestResult {
    let fixture = Fixture::new().await?;
    fixture.migrate().await?;
    let composition = composition(&fixture.example_url).await?;
    let operations = PgPool::connect(&fixture.operations_url).await?;
    sqlx::query(
        "CREATE FUNCTION bc_example.reject_outbox_insert() RETURNS trigger \
         LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'deterministic outbox failure'; END $$",
    )
    .execute(&operations)
    .await?;
    sqlx::query(
        "CREATE TRIGGER reject_outbox_insert BEFORE INSERT ON bc_example.outbox_events \
         FOR EACH ROW EXECUTE FUNCTION bc_example.reject_outbox_insert()",
    )
    .execute(&operations)
    .await?;
    let command_result = composition
        .create_example_record("must roll back".into())
        .await
        .expect("example module is composed");
    let state_count: (i64,) = sqlx::query_as("SELECT count(*) FROM bc_example.example_records")
        .fetch_one(&operations)
        .await?;
    let event_count: (i64,) = sqlx::query_as("SELECT count(*) FROM bc_example.outbox_events")
        .fetch_one(&operations)
        .await?;
    operations.close().await;
    composition.close_databases().await;
    fixture.cleanup().await?;
    assert!(
        command_result.is_err(),
        "outbox trigger must fail the command"
    );
    assert_eq!(state_count.0, 0, "business state must roll back");
    assert_eq!(event_count.0, 0, "outbox event must roll back");
    Ok(())
}

#[tokio::test]
async fn outbox_envelope_persists_versioned_event_and_ordering_metadata() -> TestResult {
    let fixture = Fixture::new().await?;
    fixture.migrate().await?;
    let composition = composition(&fixture.example_url).await?;
    composition
        .create_example_record("versioned payload".into())
        .await
        .expect("example module is composed")
        .expect("command succeeds");
    let operations = PgPool::connect(&fixture.operations_url).await?;
    let row: (bool, String, i16, String, i64, i64, bool, i16, serde_json::Value, i32) =
        sqlx::query_as(
            "SELECT event_id IS NOT NULL, event_type, event_version, aggregate_type, aggregate_id, \
             aggregate_sequence, occurred_at IS NOT NULL, payload_schema_version, payload, attempt_count \
             FROM bc_example.outbox_events",
        )
        .fetch_one(&operations)
        .await?;
    operations.close().await;
    composition.close_databases().await;
    fixture.cleanup().await?;
    assert!(row.0);
    assert_eq!(row.1, "example.record_created");
    assert_eq!(row.2, 1);
    assert_eq!(row.3, "example_record");
    assert_eq!(row.4, 1);
    assert_eq!(row.5, 1);
    assert!(row.6);
    assert_eq!(row.7, 1);
    assert_eq!(row.8["schema_version"], 1);
    assert_eq!(row.8["record"]["id"], row.4);
    assert_eq!(row.8["record"]["label"], "versioned payload");
    assert_eq!(row.9, 0);
    Ok(())
}

#[tokio::test]
async fn dispatcher_permissions_are_limited_to_the_example_outbox() -> TestResult {
    let fixture = Fixture::new().await?;
    fixture.migrate().await?;
    let composition = composition(&fixture.example_url).await?;
    composition
        .create_example_record("dispatcher permission".into())
        .await
        .expect("example module is composed")
        .expect("command succeeds");
    let dispatcher = PgPool::connect(&fixture.dispatcher_url).await?;
    let (event_count,): (i64,) = sqlx::query_as("SELECT count(*) FROM bc_example.outbox_events")
        .fetch_one(&dispatcher)
        .await?;
    sqlx::query(
        "UPDATE bc_example.outbox_events SET claimed_by = 'dispatcher-test', \
         claim_until = clock_timestamp() + interval '1 minute'",
    )
    .execute(&dispatcher)
    .await?;
    let business_read = sqlx::query("SELECT * FROM bc_example.example_records")
        .fetch_all(&dispatcher)
        .await;
    let business_write = sqlx::query("UPDATE bc_example.example_records SET label = 'forbidden'")
        .execute(&dispatcher)
        .await;
    dispatcher.close().await;
    composition.close_databases().await;
    fixture.cleanup().await?;
    assert_eq!(event_count, 1);
    for denied in [business_read.err(), business_write.err()] {
        let error = denied.expect("dispatcher must not access business state");
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
async fn outbox_delivery_acceptance_marks_event_delivered() -> TestResult {
    let fixture = Fixture::new().await?;
    fixture.migrate().await?;
    let composition = composition(&fixture.example_url).await?;
    composition
        .create_example_record("delivery accepted".into())
        .await
        .expect("example module is composed")
        .expect("example command succeeds");
    let dispatcher = PostgresOutboxDispatcher::connect(&fixture.dispatcher_url, 60).await?;
    let delivery = InProcessEventDelivery::default();
    let outcome = dispatcher.dispatch_one("dispatcher-one", &delivery).await?;
    let operations = PgPool::connect(&fixture.operations_url).await?;
    let row: (i16, i16, serde_json::Value, Option<String>, i32) = sqlx::query_as(
        "SELECT event_version, payload_schema_version, payload, delivered_at::text, attempt_count \
         FROM bc_example.outbox_events",
    )
    .fetch_one(&operations)
    .await?;
    let accepted = delivery.accepted_event_ids();
    dispatcher.close().await;
    operations.close().await;
    composition.close_databases().await;
    fixture.cleanup().await?;
    assert!(matches!(outcome, DispatchOutcome::Delivered { .. }));
    assert_eq!(row.0, 1);
    assert_eq!(row.1, 1);
    assert_eq!(row.2["schema_version"], 1);
    assert!(row.3.is_some());
    assert_eq!(row.4, 1);
    assert_eq!(accepted.len(), 1);
    Ok(())
}

#[tokio::test]
async fn outbox_retry_rejected_delivery_remains_unresolved_then_succeeds() -> TestResult {
    let fixture = Fixture::new().await?;
    fixture.migrate().await?;
    let composition = composition(&fixture.example_url).await?;
    composition
        .create_example_record("delivery retry".into())
        .await
        .expect("example module is composed")
        .expect("example command succeeds");
    let dispatcher = PostgresOutboxDispatcher::connect(&fixture.dispatcher_url, 60).await?;
    let delivery = InProcessEventDelivery::default();
    delivery.reject_next(1);
    let rejected = dispatcher.dispatch_one("dispatcher-one", &delivery).await?;
    let operations = PgPool::connect(&fixture.operations_url).await?;
    let after_rejection: (Option<String>, i32) =
        sqlx::query_as("SELECT delivered_at::text, attempt_count FROM bc_example.outbox_events")
            .fetch_one(&operations)
            .await?;
    let succeeded = dispatcher.dispatch_one("dispatcher-one", &delivery).await?;
    let delivered_at: (Option<String>, i32) =
        sqlx::query_as("SELECT delivered_at::text, attempt_count FROM bc_example.outbox_events")
            .fetch_one(&operations)
            .await?;
    dispatcher.close().await;
    operations.close().await;
    composition.close_databases().await;
    fixture.cleanup().await?;
    assert!(matches!(rejected, DispatchOutcome::Rejected { .. }));
    assert!(after_rejection.0.is_none());
    assert_eq!(after_rejection.1, 1);
    assert!(matches!(succeeded, DispatchOutcome::Delivered { .. }));
    assert!(delivered_at.0.is_some());
    assert_eq!(delivered_at.1, 2);
    Ok(())
}

#[tokio::test]
async fn outbox_retry_abandoned_lease_can_be_reclaimed() -> TestResult {
    let fixture = Fixture::new().await?;
    fixture.migrate().await?;
    let composition = composition(&fixture.example_url).await?;
    composition
        .create_example_record("abandoned lease".into())
        .await
        .expect("example module is composed")
        .expect("example command succeeds");
    let dispatcher = PostgresOutboxDispatcher::connect(&fixture.dispatcher_url, 60).await?;
    let first_claim = dispatcher.claim_next("dispatcher-one").await?.unwrap();
    assert!(dispatcher.claim_next("dispatcher-two").await?.is_none());
    let operations = PgPool::connect(&fixture.operations_url).await?;
    sqlx::query(
        "UPDATE bc_example.outbox_events SET claim_until = clock_timestamp() - interval '1 second' \
         WHERE event_id = $1::uuid",
    )
    .bind(&first_claim.request.event_id)
    .execute(&operations)
    .await?;
    let reclaimed = dispatcher.claim_next("dispatcher-two").await?.unwrap();
    let delivery = InProcessEventDelivery::default();
    delivery.deliver(reclaimed.request.clone()).await?;
    assert!(dispatcher.acknowledge(&reclaimed).await?);
    let attempts: (i32, Option<String>) = sqlx::query_as(
        "SELECT attempt_count, delivered_at::text FROM bc_example.outbox_events WHERE event_id = $1::uuid",
    )
    .bind(&first_claim.request.event_id)
    .fetch_one(&operations)
    .await?;
    dispatcher.close().await;
    operations.close().await;
    composition.close_databases().await;
    fixture.cleanup().await?;
    assert_eq!(attempts.0, 2);
    assert!(attempts.1.is_some());
    Ok(())
}

#[tokio::test]
async fn outbox_delivery_lost_ack_causes_tolerated_duplicate() -> TestResult {
    let fixture = Fixture::new().await?;
    fixture.migrate().await?;
    let composition = composition(&fixture.example_url).await?;
    composition
        .create_example_record("lost acknowledgement".into())
        .await
        .expect("example module is composed")
        .expect("example command succeeds");
    let dispatcher = PostgresOutboxDispatcher::connect(&fixture.dispatcher_url, 60).await?;
    let delivery = InProcessEventDelivery::default();
    let accepted_before_lost_ack = dispatcher.claim_next("dispatcher-one").await?.unwrap();
    delivery
        .deliver(accepted_before_lost_ack.request.clone())
        .await?;
    // Simulate process loss after acceptance: deliberately do not call acknowledge.
    let operations = PgPool::connect(&fixture.operations_url).await?;
    sqlx::query(
        "UPDATE bc_example.outbox_events SET claim_until = clock_timestamp() - interval '1 second' \
         WHERE event_id = $1::uuid",
    )
    .bind(&accepted_before_lost_ack.request.event_id)
    .execute(&operations)
    .await?;
    let retry = dispatcher.claim_next("dispatcher-two").await?.unwrap();
    delivery.deliver(retry.request.clone()).await?;
    assert!(dispatcher.acknowledge(&retry).await?);
    let attempts: (i32, Option<String>) = sqlx::query_as(
        "SELECT attempt_count, delivered_at::text FROM bc_example.outbox_events WHERE event_id = $1::uuid",
    )
    .bind(&retry.request.event_id)
    .fetch_one(&operations)
    .await?;
    let accepted_ids = delivery.accepted_event_ids();
    let delivery_attempts = delivery.attempts_for(&retry.request.event_id);
    dispatcher.close().await;
    operations.close().await;
    composition.close_databases().await;
    fixture.cleanup().await?;
    assert_eq!(delivery_attempts, 2);
    assert_eq!(accepted_ids.len(), 1);
    assert_eq!(attempts.0, 2);
    assert!(attempts.1.is_some());
    Ok(())
}

#[tokio::test]
async fn outbox_delivery_blocks_later_same_aggregate_event_while_head_unresolved() -> TestResult {
    let fixture = Fixture::new().await?;
    fixture.migrate().await?;
    let operations = PgPool::connect(&fixture.operations_url).await?;
    sqlx::query(
        "INSERT INTO bc_example.outbox_events \
         (event_type, event_version, aggregate_type, aggregate_id, aggregate_sequence, \
          payload_schema_version, payload) \
         VALUES ('example.test', 1, 'ordered_test', 991, 1, 1, '{}'::jsonb), \
                ('example.test', 1, 'ordered_test', 991, 2, 1, '{}'::jsonb)",
    )
    .execute(&operations)
    .await?;
    let dispatcher = PostgresOutboxDispatcher::connect(&fixture.dispatcher_url, 60).await?;
    let head = dispatcher.claim_next("dispatcher-one").await?.unwrap();
    assert_eq!(head.request.aggregate_sequence, 1);
    assert!(dispatcher.claim_next("dispatcher-two").await?.is_none());
    sqlx::query(
        "UPDATE bc_example.outbox_events SET claim_until = clock_timestamp() - interval '1 second' \
         WHERE event_id = $1::uuid",
    )
    .bind(&head.request.event_id)
    .execute(&operations)
    .await?;
    // An expired but unresolved head remains a barrier; it is reclaimed first.
    let reclaimed_head = dispatcher.claim_next("dispatcher-two").await?.unwrap();
    assert_eq!(reclaimed_head.request.aggregate_sequence, 1);
    assert!(dispatcher.claim_next("dispatcher-three").await?.is_none());
    let delivery = InProcessEventDelivery::default();
    delivery.deliver(reclaimed_head.request.clone()).await?;
    assert!(dispatcher.acknowledge(&reclaimed_head).await?);
    let second = dispatcher.claim_next("dispatcher-three").await?.unwrap();
    assert_eq!(second.request.aggregate_sequence, 2);
    delivery.deliver(second.request.clone()).await?;
    assert!(dispatcher.acknowledge(&second).await?);
    dispatcher.close().await;
    operations.close().await;
    fixture.cleanup().await?;
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
