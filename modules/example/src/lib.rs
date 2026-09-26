use serde::Deserialize;

pub mod database;
use database::ExampleDatabase;

pub mod events {
    /// Domain event produced when the example module creates a record.
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct ExampleRecordCreated {
        pub record_id: i64,
        pub label: String,
    }
}

#[derive(Debug)]
pub enum ExampleError {
    DatabaseNotAttached,
    Database(sqlx::Error),
}

impl From<sqlx::Error> for ExampleError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ExampleConfiguration {
    pub label: String,
}

#[derive(Clone, Debug)]
pub struct ExampleModule {
    pub configuration: ExampleConfiguration,
    database: Option<ExampleDatabase>,
}

impl ExampleModule {
    pub fn new(configuration: ExampleConfiguration) -> Self {
        Self {
            configuration,
            database: None,
        }
    }

    /// Published application API used by a consumer-owned local adapter.
    pub fn label(&self) -> &str {
        &self.configuration.label
    }

    /// Published application API used by a consumer-owned local adapter.
    pub async fn create_record(
        &self,
        label: String,
    ) -> Result<events::ExampleRecordCreated, ExampleError> {
        let database = self
            .database
            .as_ref()
            .ok_or(ExampleError::DatabaseNotAttached)?;
        database.create_record(label).await.map_err(Into::into)
    }

    pub fn attach_database(&mut self, database: ExampleDatabase) {
        self.database = Some(database);
    }

    pub async fn database_is_ready(&self) -> bool {
        let Some(database) = &self.database else {
            return false;
        };
        matches!(database.migration_state_is_current().await, Ok(true))
            && database.ping().await.is_ok()
    }

    pub async fn close_database(&self) {
        if let Some(database) = &self.database {
            database.close().await;
        }
    }
}
