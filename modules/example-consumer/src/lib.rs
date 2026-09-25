/// The capability this consumer needs from the example provider.
pub trait ExampleLabelPort {
    fn read_label(&self) -> String;
}

/// Consumer-owned application logic for reading the configured example label.
pub fn read_example_label(provider: &impl ExampleLabelPort) -> String {
    provider.read_label()
}
