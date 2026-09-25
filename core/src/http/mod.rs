mod routes;

#[cfg(test)]
use crate::{
    composition::{Composition, compose_modules},
    manifest::{parse_and_validate, supported_release_metadata},
};

pub use routes::{router, router_with_readiness};

#[cfg(test)]
fn test_composition() -> Composition {
    let source = "release: 0.1.0\ndeployment_mode: self_hosted\nmodules: {}\n";
    let manifest = parse_and_validate(source, &supported_release_metadata()).unwrap();
    compose_modules(manifest).unwrap()
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    #[tokio::test]
    async fn healthz_returns_success_without_external_dependencies() {
        let response = super::router(super::test_composition())
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), http::StatusCode::OK);
        assert_eq!(
            response.headers()[http::header::CONTENT_TYPE],
            "application/json"
        );
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
            serde_json::json!({ "status": "ok" })
        );
    }

    #[tokio::test]
    async fn unsupported_path_uses_json_error_envelope() {
        let response = super::router(super::test_composition())
            .oneshot(
                Request::builder()
                    .uri("/unsupported")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), http::StatusCode::NOT_FOUND);
        assert_eq!(
            response.headers()[http::header::CONTENT_TYPE],
            "application/json"
        );
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
            serde_json::json!({
                "error": {
                    "code": "not_found",
                    "message": "The requested resource was not found."
                }
            })
        );
    }
}
