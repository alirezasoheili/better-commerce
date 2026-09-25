use serde::Deserialize;

pub mod database;
use database::ExampleDatabase;

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
