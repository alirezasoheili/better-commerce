use better_commerce_core::{
    composition::compose_modules,
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
    let composition = compose_modules(manifest)?;
    let example_enabled = composition.module("example").is_some();
    let example_url = if example_enabled {
        Some(std::env::var("EXAMPLE_RUNTIME_DATABASE_URL")?)
    } else {
        None
    };
    let dispatcher_url = if example_enabled {
        Some(std::env::var("DISPATCHER_DATABASE_URL")?)
    } else {
        None
    };
    migrate_installation(
        &std::env::var("OPERATIONS_DATABASE_URL")?,
        example_enabled,
        example_url.as_deref(),
        dispatcher_url.as_deref(),
        &std::env::var("READINESS_DATABASE_URL")?,
    )
    .await
}
