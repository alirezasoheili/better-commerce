//! Protobuf DTOs and handwritten translation for the example transport boundary.

/// Generated transport messages; do not re-export these from domain or application crates.
pub mod generated {
    #![allow(
        clippy::all,
        dead_code,
        missing_docs,
        non_camel_case_types,
        non_snake_case
    )]

    include!("generated/better/commerce/example/v1/better.commerce.example.v1.rs");
}

use better_commerce_example_consumer::ExampleLabelPort;

/// Adapts a protobuf response into the consumer-owned label port.
#[derive(Clone, Debug)]
pub struct ProtobufExampleLabelAdapter {
    response: generated::ReadLabelResponse,
}

impl ProtobufExampleLabelAdapter {
    pub fn from_response(response: generated::ReadLabelResponse) -> Self {
        Self { response }
    }
}

impl ExampleLabelPort for ProtobufExampleLabelAdapter {
    fn read_label(&self) -> String {
        self.response.label.clone()
    }
}

#[cfg(test)]
mod tests {
    use better_commerce_example_consumer::read_example_label;

    use super::{ProtobufExampleLabelAdapter, generated};

    #[test]
    fn generated_response_is_translated_through_the_consumer_port() {
        let response = generated::ReadLabelResponse {
            label: "Example over transport".to_owned(),
        };
        let adapter = ProtobufExampleLabelAdapter::from_response(response);

        assert_eq!(read_example_label(&adapter), "Example over transport");
    }
}
