use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ExampleConfiguration {
    pub label: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExampleModule {
    pub configuration: ExampleConfiguration,
}

impl ExampleModule {
    pub fn new(configuration: ExampleConfiguration) -> Self {
        Self { configuration }
    }

    /// Published application API used by a consumer-owned local adapter.
    pub fn label(&self) -> &str {
        &self.configuration.label
    }
}
