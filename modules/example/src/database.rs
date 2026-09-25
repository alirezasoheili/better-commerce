use sqlx::{PgPool, postgres::PgPoolOptions};

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
            .max_connections(5)
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
