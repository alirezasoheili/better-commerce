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

#[derive(Debug)]
pub enum ExampleConfigurationError {
    Invalid(serde_yaml::Error),
    EmptyLabel,
}

impl std::fmt::Display for ExampleConfigurationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(error) => write!(formatter, "{error}"),
            Self::EmptyLabel => formatter.write_str("configuration.label must not be empty"),
        }
    }
}

impl std::error::Error for ExampleConfigurationError {}

/// Parse and validate the example module's opaque manifest configuration.
pub fn parse_configuration(
    value: serde_yaml::Value,
) -> Result<ExampleConfiguration, ExampleConfigurationError> {
    let configuration: ExampleConfiguration =
        serde_yaml::from_value(value).map_err(ExampleConfigurationError::Invalid)?;
    if configuration.label.trim().is_empty() {
        return Err(ExampleConfigurationError::EmptyLabel);
    }
    Ok(configuration)
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

#[cfg(test)]
mod configuration_tests {
    use super::{ExampleConfiguration, ExampleConfigurationError, parse_configuration};

    fn configuration(yaml: &str) -> Result<ExampleConfiguration, ExampleConfigurationError> {
        parse_configuration(serde_yaml::from_str(yaml).unwrap())
    }

    #[test]
    fn parses_valid_configuration_into_the_typed_value() {
        assert_eq!(
            configuration("label: Demo").unwrap(),
            ExampleConfiguration {
                label: "Demo".into()
            }
        );
    }

    #[test]
    fn rejects_blank_label() {
        assert!(matches!(
            configuration("label: '  '").unwrap_err(),
            ExampleConfigurationError::EmptyLabel
        ));
    }

    #[test]
    fn rejects_unknown_fields() {
        assert!(matches!(
            configuration("label: Demo\nsurprise: true").unwrap_err(),
            ExampleConfigurationError::Invalid(_)
        ));
    }

    #[test]
    fn rejects_malformed_field_types() {
        assert!(matches!(
            configuration("label: 42").unwrap_err(),
            ExampleConfigurationError::Invalid(_)
        ));
    }
}
