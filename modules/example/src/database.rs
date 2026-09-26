use sqlx::{PgPool, postgres::PgPoolOptions};

use crate::events::ExampleRecordCreated;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");
pub const SCHEMA: &str = "bc_example";

/// The only database capability supplied to example infrastructure.
#[derive(Clone, Debug)]
pub struct ExampleDatabase {
    pool: PgPool,
}

impl ExampleDatabase {
    pub async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .after_connect(|connection, _| {
                Box::pin(async move {
                    sqlx::query("SET search_path TO bc_example, pg_catalog")
                        .execute(connection)
                        .await?;
                    Ok(())
                })
            })
            .connect_lazy(database_url)?;
        Ok(Self { pool })
    }

    pub async fn create_record(&self, label: String) -> Result<ExampleRecordCreated, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let event = write_business_state(&mut tx, label).await?;
        write_outbox_event(&mut tx, &event).await?;
        tx.commit().await?;
        Ok(event)
    }

    pub async fn migration_state_is_current(&self) -> Result<bool, sqlx::Error> {
        let rows: Vec<(i64, bool, Vec<u8>)> = sqlx::query_as(
            "SELECT version, success, checksum FROM _sqlx_migrations ORDER BY version",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.len() == MIGRATOR.migrations.len()
            && rows.iter().zip(MIGRATOR.migrations.iter()).all(
                |((version, success, checksum), expected)| {
                    *version == expected.version
                        && *success
                        && checksum.as_slice() == expected.checksum.as_ref()
                },
            ))
    }

    pub async fn ping(&self) -> Result<(), sqlx::Error> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }
}

async fn write_business_state(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    label: String,
) -> Result<ExampleRecordCreated, sqlx::Error> {
    let (record_id, label): (i64, String) =
        sqlx::query_as("INSERT INTO example_records (label) VALUES ($1) RETURNING id, label")
            .bind(label)
            .fetch_one(&mut **tx)
            .await?;
    Ok(ExampleRecordCreated { record_id, label })
}

async fn write_outbox_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    event: &ExampleRecordCreated,
) -> Result<(), sqlx::Error> {
    const EVENT_TYPE: &str = "example.record_created";
    const EVENT_VERSION: i16 = 1;
    const AGGREGATE_TYPE: &str = "example_record";
    const PAYLOAD_SCHEMA_VERSION: i16 = 1;
    let payload = serde_json::json!({
        "schema_version": PAYLOAD_SCHEMA_VERSION,
        "record": {
            "id": event.record_id,
            "label": event.label,
        },
    });
    sqlx::query(
        "INSERT INTO outbox_events \
         (event_type, event_version, aggregate_type, aggregate_id, aggregate_sequence, payload_schema_version, payload) \
         VALUES ($1, $2, $3, $4, 1, $5, $6::jsonb)",
    )
    .bind(EVENT_TYPE)
    .bind(EVENT_VERSION)
    .bind(AGGREGATE_TYPE)
    .bind(event.record_id)
    .bind(PAYLOAD_SCHEMA_VERSION)
    .bind(payload.to_string())
    .execute(&mut **tx)
    .await?;
    Ok(())
}
