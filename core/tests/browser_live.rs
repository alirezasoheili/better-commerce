//! Separate frontend gate: production assets and a real scoped PostgreSQL installation.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use better_commerce_core::{
    composition::compose_modules,
    database::{ReadinessDatabase, migrate_installation, scoped_role_names},
    http::{FrontendAssets, HttpRuntime, router_with_http},
    manifest::{
        HttpConfiguration, SecretReference, parse_and_validate, supported_release_metadata,
    },
};
use sqlx::postgres::PgPoolOptions;
use std::{path::PathBuf, process::Command};

#[tokio::test(flavor = "multi_thread")]
#[ignore = "Requires pinned frontend installs, production builds and Playwright Chromium; run the documented frontend gate"]
async fn production_surfaces_against_live_postgres() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut random = [0u8; 32];
    getrandom::getrandom(&mut random)?;
    let token = URL_SAFE_NO_PAD.encode(random);
    let mut database_random = [0u8; 12];
    getrandom::getrandom(&mut database_random)?;
    let database = format!(
        "bc_browser_{}",
        URL_SAFE_NO_PAD.encode(database_random).replace('-', "x")
    );
    let admin_url = std::env::var("BC_TEST_ADMIN_DATABASE_URL")?;
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&admin_url)
        .await?;
    sqlx::query(&format!("CREATE DATABASE \"{database}\""))
        .execute(&admin)
        .await?;
    let result = async {
        let (_, _, ready_role) = scoped_role_names(&database);
        let mut operations_url = url::Url::parse(&admin_url)?;
        operations_url.set_path(&database);
        let mut ready_url = operations_url.clone();
        ready_url
            .set_username(&ready_role)
            .map_err(|_| "invalid test identity")?;
        ready_url
            .set_password(Some(&token))
            .map_err(|_| "invalid test password")?;
        migrate_installation(
            operations_url.as_str(),
            false,
            None,
            None,
            ready_url.as_str(),
        )
        .await?;
        let readiness = ReadinessDatabase::connect(ready_url.as_str()).await?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let origin = format!("http://{}", listener.local_addr()?);
        let runtime = HttpRuntime::from_resolved_token(
            &HttpConfiguration {
                public_origin: origin.clone(),
                admin_token: SecretReference::Environment {
                    env: "TEST_ONLY_TOKEN".into(),
                },
                trusted_proxy_ip: None,
            },
            &token,
        )?;
        let assets = FrontendAssets::load(
            &root.join("frontend/storefront/dist"),
            &root.join("frontend/admin/dist"),
        )?;
        let composition = compose_modules(parse_and_validate(
            "release: 0.1.0\ndeployment_mode: self_hosted\nmodules: {}\n",
            &supported_release_metadata(),
        )?)?;
        let app = router_with_http(composition, Some(readiness), Some(runtime), Some(assets));
        let redact_token = token.clone();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await
        });
        let node_result = tokio::task::spawn_blocking(move || {
            Command::new("node")
                .arg(root.join("frontend/e2e/smoke.mjs"))
                .env("BC_BROWSER_ORIGIN", origin)
                .env("BC_BROWSER_TOKEN", token)
                .output()
        })
        .await?;
        server.abort();
        let output = node_result?;
        if !output.status.success() {
            let diagnostics = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
            .replace(&redact_token, "[REDACTED]");
            return Err(format!("live browser smoke failed: {diagnostics}").into());
        }
        println!("{}", String::from_utf8_lossy(&output.stdout));
        Ok::<(), Box<dyn std::error::Error>>(())
    }
    .await;
    sqlx::query(&format!("DROP DATABASE \"{database}\" WITH (FORCE)"))
        .execute(&admin)
        .await?;
    let (_, _, ready_role) = scoped_role_names(&database);
    sqlx::query(&format!("DROP ROLE IF EXISTS \"{ready_role}\""))
        .execute(&admin)
        .await?;
    admin.close().await;
    result
}
