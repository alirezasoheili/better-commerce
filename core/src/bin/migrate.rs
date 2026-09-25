use better_commerce_core::{
    database::migrate_installation,
    manifest::{parse_and_validate, supported_release_metadata},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_path = std::env::var("MANIFEST_PATH").unwrap_or_else(|_| "manifest.yaml".into());
    let manifest = parse_and_validate(
        &std::fs::read_to_string(manifest_path)?,
        &supported_release_metadata(),
    )?;
    let example_url = if manifest.modules.contains_key("example") {
        Some(std::env::var("EXAMPLE_RUNTIME_DATABASE_URL")?)
    } else {
        None
    };
    migrate_installation(
        &std::env::var("OPERATIONS_DATABASE_URL")?,
        manifest.modules.contains_key("example"),
        example_url.as_deref(),
        &std::env::var("READINESS_DATABASE_URL")?,
    )
    .await
}
