use std::future::Future;

/// The capability this consumer needs from the example provider.
pub trait ExampleLabelPort {
    fn read_label(&self) -> String;
}

/// Consumer-owned application logic for reading the configured example label.
pub fn read_example_label(provider: &impl ExampleLabelPort) -> String {
    provider.read_label()
}

/// The smallest command capability this consumer needs from the example module.
pub trait ExampleCommandPort {
    type Error;

    fn create_example_record(
        &self,
        label: String,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

/// Consumer-owned application use case for creating one example record.
pub async fn create_example_record<P: ExampleCommandPort>(
    provider: &P,
    label: String,
) -> Result<(), P::Error> {
    provider.create_example_record(label).await
}
