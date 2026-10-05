use std::{net::SocketAddr, path::Path};
use tokio::net::TcpListener;

use better_commerce_core::{
    composition::compose_modules,
    database::ReadinessDatabase,
    manifest::{parse_and_validate, supported_release_metadata},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_path = std::env::var("MANIFEST_PATH").unwrap_or_else(|_| "manifest.yaml".into());
    let manifest_source = std::fs::read_to_string(&manifest_path)?;
    let manifest = parse_and_validate(&manifest_source, &supported_release_metadata())?;
    let http = manifest
        .http
        .as_ref()
        .map(|configuration| {
            let mut configuration = configuration.clone();
            if configuration.public_origin.starts_with("http://") {
                if let Ok(ingress) = std::env::var("BC_HTTP_LOOPBACK_PROXY_IP") {
                    if !ingress.is_empty() {
                        configuration.trusted_proxy_ip = Some(
                            ingress
                                .parse()
                                .map_err(|_| better_commerce_core::http::HttpConfigurationError)?,
                        );
                    }
                }
            }
            if let Ok(token) = std::env::var("BC_RESOLVED_ADMIN_TOKEN") {
                if !token.is_empty() {
                    return better_commerce_core::http::HttpRuntime::from_resolved_token(
                        &configuration,
                        &token,
                    );
                }
            }
            better_commerce_core::http::HttpRuntime::resolve(
                &configuration,
                Path::new(&manifest_path).parent().unwrap_or(Path::new(".")),
            )
        })
        .transpose()?;
    let assets = if http.is_some() {
        Some(better_commerce_core::http::FrontendAssets::load(
            Path::new(
                &std::env::var("STOREFRONT_DIST")
                    .unwrap_or_else(|_| "frontend/storefront/dist".into()),
            ),
            Path::new(
                &std::env::var("ADMIN_DIST").unwrap_or_else(|_| "frontend/admin/dist".into()),
            ),
        )?)
    } else {
        None
    };
    let mut composition = compose_modules(manifest)?;
    if composition.module("example").is_some() {
        composition
            .attach_example_database(&std::env::var("EXAMPLE_RUNTIME_DATABASE_URL")?)
            .await?;
    }
    let readiness = ReadinessDatabase::connect(&std::env::var("READINESS_DATABASE_URL")?).await?;

    let port = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(3000);
    let listener = TcpListener::bind(("0.0.0.0", port)).await?;

    axum::serve(
        listener,
        better_commerce_core::http::router_with_http(composition, Some(readiness), http, assets)
            .into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}
