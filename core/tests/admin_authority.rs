use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use better_commerce_core::{
    composition::compose_modules,
    http::{HttpRuntime, router_with_http},
    manifest::{
        HttpConfiguration, SecretReference, parse_and_validate, supported_release_metadata,
    },
};
use std::net::SocketAddr;
use tower::ServiceExt;

fn token() -> String {
    URL_SAFE_NO_PAD.encode((0u8..32).collect::<Vec<_>>())
}
fn configuration(origin: &str, proxy: Option<&str>) -> HttpConfiguration {
    HttpConfiguration {
        public_origin: origin.into(),
        admin_token: SecretReference::Environment {
            env: "TEST_ONLY_TOKEN".into(),
        },
        trusted_proxy_ip: proxy.map(|v| v.parse().unwrap()),
    }
}
fn runtime(origin: &str, proxy: Option<&str>) -> HttpRuntime {
    HttpRuntime::from_resolved_token(&configuration(origin, proxy), &token()).unwrap()
}
async fn request(
    runtime: Option<HttpRuntime>,
    path: &str,
    method: &str,
    bearer: Option<&str>,
    origin: Option<&str>,
    transport: (&str, &str, Option<&str>),
) -> (StatusCode, http::HeaderMap, serde_json::Value) {
    let (host, peer, forwarded) = transport;
    let manifest = parse_and_validate(
        "release: 0.1.0\ndeployment_mode: self_hosted\nmodules: {}\n",
        &supported_release_metadata(),
    )
    .unwrap();
    let app = router_with_http(compose_modules(manifest).unwrap(), None, runtime, None);
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("host", host);
    if let Some(value) = bearer {
        builder = builder.header("authorization", format!("Bearer {value}"));
    }
    if let Some(value) = origin {
        builder = builder.header("origin", value);
    }
    if let Some(value) = forwarded {
        builder = builder.header("x-forwarded-proto", value);
    }
    let mut request = builder.body(Body::empty()).unwrap();
    request
        .extensions_mut()
        .insert(ConnectInfo(peer.parse::<SocketAddr>().unwrap()));
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), 8192)
        .await
        .unwrap();
    (status, headers, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn admin_verifier_authorizes_only_the_configured_token_and_never_echoes_it() {
    for bearer in [None, Some("not-the-token"), Some(token().as_str())] {
        let (status, headers, body) = request(
            Some(runtime("http://127.0.0.1:3000", None)),
            "/api/v1/admin/status",
            "GET",
            bearer,
            None,
            ("127.0.0.1:3000", "127.0.0.1:4010", None),
        )
        .await;
        assert_eq!(
            status,
            if bearer == Some(token().as_str()) {
                StatusCode::OK
            } else {
                StatusCode::UNAUTHORIZED
            }
        );
        assert_eq!(headers["cache-control"], "no-store");
        assert!(!body.to_string().contains(&token()));
        if status == StatusCode::OK {
            assert_eq!(
                body,
                serde_json::json!({"status":"available","capabilities":[]})
            );
        } else {
            assert_eq!(body["error"]["code"], "ADMIN_AUTHORITY_INVALID");
        }
    }
}

#[tokio::test]
async fn browser_mutations_require_the_exact_approved_origin_nonbrowser_use_remains_available() {
    for origin in [
        Some("https://evil.test"),
        Some("null"),
        Some("http://127.0.0.1:3000.evil.test"),
        Some("http://127.0.0.1:3000"),
        None,
    ] {
        let (status, headers, body) = request(
            Some(runtime("http://127.0.0.1:3000", None)),
            "/api/v1/admin/future",
            "POST",
            Some(&token()),
            origin,
            ("127.0.0.1:3000", "127.0.0.1:4010", None),
        )
        .await;
        let approved = origin.is_none() || origin == Some("http://127.0.0.1:3000");
        assert_eq!(
            status,
            if approved {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::BAD_REQUEST
            }
        );
        assert_eq!(headers["cache-control"], "no-store");
        assert_eq!(
            body["error"]["request_id"],
            headers["x-request-id"].to_str().unwrap()
        );
        assert!(!headers.contains_key("access-control-allow-origin"));
    }
}

#[tokio::test]
async fn credential_transport_rejects_untrusted_peers_and_spoofed_https() {
    for (origin, proxy, host, peer, forwarded, expected) in [
        (
            "http://127.0.0.1:3000",
            Some("192.0.2.2"),
            "127.0.0.1:3000",
            "192.0.2.2:4000",
            None,
            StatusCode::OK,
        ),
        (
            "http://127.0.0.1:3000",
            None,
            "127.0.0.1:3000",
            "192.0.2.1:4000",
            None,
            StatusCode::BAD_REQUEST,
        ),
        (
            "http://127.0.0.1:3000",
            None,
            "evil.test",
            "127.0.0.1:4000",
            None,
            StatusCode::BAD_REQUEST,
        ),
        (
            "https://shop.test",
            Some("192.0.2.2"),
            "shop.test",
            "192.0.2.1:4000",
            Some("https"),
            StatusCode::BAD_REQUEST,
        ),
        (
            "https://shop.test",
            Some("192.0.2.2"),
            "shop.test",
            "192.0.2.2:4000",
            Some("http"),
            StatusCode::BAD_REQUEST,
        ),
        (
            "https://shop.test",
            Some("192.0.2.2"),
            "shop.test",
            "192.0.2.2:4000",
            Some("https"),
            StatusCode::OK,
        ),
    ] {
        assert_eq!(
            request(
                Some(runtime(origin, proxy)),
                "/api/v1/admin/status",
                "GET",
                Some(&token()),
                None,
                (host, peer, forwarded)
            )
            .await
            .0,
            expected
        );
    }
}

#[test]
fn invalid_token_and_origin_configuration_errors_are_redacted() {
    assert!(
        HttpRuntime::from_resolved_token(&configuration("https://shop.test", None), &token())
            .is_err()
    );
    let mut invalid_reference = configuration("http://127.0.0.1:3000", None);
    invalid_reference.admin_token = SecretReference::Environment { env: "".into() };
    assert!(HttpRuntime::from_resolved_token(&invalid_reference, &token()).is_err());
    for origin in [
        "http://shop.test",
        "https://shop.test/path",
        "https://shop.test/",
        "https://user:secret@shop.test",
    ] {
        assert!(HttpRuntime::from_resolved_token(&configuration(origin, None), &token()).is_err());
    }
    for invalid in [
        "secret",
        "",
        "not base64url",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
    ] {
        let error = HttpRuntime::from_resolved_token(
            &configuration("http://localhost:3000", None),
            invalid,
        )
        .err()
        .unwrap();
        assert!(!format!("{error:?}").contains(invalid) || invalid.is_empty());
    }
    let config = HttpConfiguration {
        public_origin: "http://localhost:3000".into(),
        admin_token: SecretReference::File {
            file: "missing-admin-secret".into(),
        },
        trusted_proxy_ip: None,
    };
    assert!(HttpRuntime::resolve(&config, std::path::Path::new(".")).is_err());
}

#[tokio::test]
async fn missing_http_configuration_never_reports_admin_available_or_m1_readiness() {
    assert_eq!(
        request(
            None,
            "/api/v1/admin/status",
            "GET",
            None,
            None,
            ("127.0.0.1:3000", "127.0.0.1:4010", None)
        )
        .await
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    let (_, _, readiness) = request(
        None,
        "/readyz",
        "GET",
        None,
        None,
        ("127.0.0.1:3000", "127.0.0.1:4010", None),
    )
    .await;
    assert_eq!(readiness["error"]["code"], "not_ready");
}

#[tokio::test]
async fn secret_responses_require_secure_transport_and_cannot_be_cached_before_a_bearer_exists() {
    use better_commerce_core::http::{CredentialTransport, SecretJson, boundary_layer};
    let app = boundary_layer(
        axum::Router::new().route(
            "/api/v1/secret-probe",
            axum::routing::post(|transport: CredentialTransport| async {
                SecretJson::new(
                    serde_json::json!({"test_secret":"test-only-secret"}),
                    transport,
                )
            }),
        ),
        Some(runtime("http://127.0.0.1:3000", None)),
    );
    for (peer, expected) in [
        ("127.0.0.1:4010", StatusCode::OK),
        ("192.0.2.1:4010", StatusCode::BAD_REQUEST),
    ] {
        let mut request = Request::builder()
            .method("POST")
            .uri("/api/v1/secret-probe")
            .header("host", "127.0.0.1:3000")
            .body(Body::empty())
            .unwrap();
        request
            .extensions_mut()
            .insert(ConnectInfo(peer.parse::<SocketAddr>().unwrap()));
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), expected);
        if expected == StatusCode::OK {
            assert_eq!(response.headers()["cache-control"], "no-store");
        } else {
            let body = axum::body::to_bytes(response.into_body(), 8192)
                .await
                .unwrap();
            assert!(!String::from_utf8_lossy(&body).contains("test-only-secret"));
        }
    }
}

#[tokio::test]
async fn credentials_cannot_be_reflected_through_a_request_id() {
    let manifest = parse_and_validate(
        "release: 0.1.0\ndeployment_mode: self_hosted\nmodules: {}\n",
        &supported_release_metadata(),
    )
    .unwrap();
    let app = router_with_http(
        compose_modules(manifest).unwrap(),
        None,
        Some(runtime("http://127.0.0.1:3000", None)),
        None,
    );
    for credential in [token(), "Rotated_or_wrong_installation_credential".into()] {
        let mut request = Request::builder()
            .uri("/api/v1/missing")
            .header("host", "127.0.0.1:3000")
            .header("authorization", format!("Bearer {credential}"))
            .header("x-request-id", format!("prefix-{credential}-suffix"))
            .body(Body::empty())
            .unwrap();
        request
            .extensions_mut()
            .insert(ConnectInfo("127.0.0.1:4010".parse::<SocketAddr>().unwrap()));
        let response = app.clone().oneshot(request).await.unwrap();
        assert!(
            !response.headers()["x-request-id"]
                .to_str()
                .unwrap()
                .contains(&credential)
        );
        let body = axum::body::to_bytes(response.into_body(), 8192)
            .await
            .unwrap();
        assert!(!String::from_utf8_lossy(&body).contains(&credential));
    }
}
