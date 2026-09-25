mod routes;

pub use routes::router;

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    #[tokio::test]
    async fn healthz_returns_success_without_external_dependencies() {
        let response = super::router()
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
        let response = super::router()
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
