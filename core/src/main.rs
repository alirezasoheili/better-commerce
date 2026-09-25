use tokio::net::TcpListener;

use better_commerce_core::{
    composition::compose_modules,
    manifest::{parse_and_validate, supported_release_metadata},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_path = std::env::var("MANIFEST_PATH").unwrap_or_else(|_| "manifest.yaml".into());
    let manifest_source = std::fs::read_to_string(&manifest_path)?;
    let manifest = parse_and_validate(&manifest_source, &supported_release_metadata())?;
    let composition = compose_modules(manifest)?;

    let port = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(3000);
    let listener = TcpListener::bind(("0.0.0.0", port)).await?;

    axum::serve(listener, better_commerce_core::http::router(composition)).await?;
    Ok(())
}
