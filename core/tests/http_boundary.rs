use axum::{
    Json, Router,
    body::Body,
    http::{Request, StatusCode},
    routing::post,
};
use better_commerce_core::http::{
    ApiError, DecimalString, ListQuery, ListResponse, OpaqueId, Quantity, RequestId, StrictJson,
    boundary_layer,
};
use serde::{Deserialize, Serialize};
use tower::ServiceExt;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Input {
    id: OpaqueId,
    amount: DecimalString,
    nested: Nested,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Nested {
    label: String,
}

fn app() -> Router {
    boundary_layer(
        Router::new()
            .route(
                "/api/v1/list",
                axum::routing::get(|query: ListQuery| async move {
                    Json(ListResponse {
                        items: vec![
                            serde_json::json!({"id":"Product/Mixed:opaque", "limit":query.limit}),
                        ],
                        next_cursor: query.cursor,
                    })
                }),
            )
            .route(
                "/api/v1/quantity",
                post(
                    |axum::Extension(id): axum::Extension<RequestId>,
                     StrictJson(input): StrictJson<QuantityInput>| async move {
                        let quantity = input.quantity.positive_i32(&id)?;
                        let amount = input.amount.checked_i64().ok_or_else(|| {
                            ApiError::new(
                                StatusCode::UNPROCESSABLE_ENTITY,
                                "INVALID_MONEY",
                                "The amount exceeds the supported range.",
                                &id,
                            )
                        })?;
                        Ok::<_, ApiError>(Json(
                            serde_json::json!({"quantity":quantity,"amount":amount.to_string()}),
                        ))
                    },
                ),
            )
            .route(
                "/api/v1/probe",
                post(|StrictJson(input): StrictJson<Input>| async { Json(input) }),
            ),
        None,
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QuantityInput {
    quantity: Quantity,
    amount: DecimalString,
}

#[tokio::test]
async fn list_queries_are_bounded_and_keep_opaque_cursors() {
    for (query, status) in [
        ("", StatusCode::OK),
        ("?limit=100&cursor=Cursor%2FMixed", StatusCode::OK),
        ("?limit=0", StatusCode::BAD_REQUEST),
        ("?limit=101", StatusCode::BAD_REQUEST),
        ("?limit=1&limit=2", StatusCode::BAD_REQUEST),
        ("?cursor=a&cursor=b", StatusCode::BAD_REQUEST),
        ("?cursor=%ZZ", StatusCode::BAD_REQUEST),
        ("?cursor=%FF", StatusCode::BAD_REQUEST),
        ("?unknown=x", StatusCode::BAD_REQUEST),
    ] {
        let response = app()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/list{query}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status, "{query}");
        if status == StatusCode::OK {
            let bytes = axum::body::to_bytes(response.into_body(), 8192)
                .await
                .unwrap();
            let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(
                value,
                if query.is_empty() {
                    serde_json::json!({"items":[{"id":"Product/Mixed:opaque","limit":25}],"next_cursor":null})
                } else {
                    serde_json::json!({"items":[{"id":"Product/Mixed:opaque","limit":100}],"next_cursor":"Cursor/Mixed"})
                }
            );
        }
    }
}

#[tokio::test]
async fn numeric_quantity_and_decimal_range_errors_are_business_invalid_after_decoding() {
    for (body, status, code) in [
        (
            r#"{"quantity":0,"amount":"0"}"#,
            StatusCode::UNPROCESSABLE_ENTITY,
            "INVALID_QUANTITY",
        ),
        (
            r#"{"quantity":-1,"amount":"0"}"#,
            StatusCode::UNPROCESSABLE_ENTITY,
            "INVALID_QUANTITY",
        ),
        (
            r#"{"quantity":1.5,"amount":"0"}"#,
            StatusCode::UNPROCESSABLE_ENTITY,
            "INVALID_QUANTITY",
        ),
        (
            r#"{"quantity":2147483648,"amount":"0"}"#,
            StatusCode::UNPROCESSABLE_ENTITY,
            "INVALID_QUANTITY",
        ),
        (
            r#"{"quantity":"1","amount":"0"}"#,
            StatusCode::BAD_REQUEST,
            "INVALID_REQUEST",
        ),
        (
            r#"{"quantity":1,"amount":"9223372036854775808"}"#,
            StatusCode::UNPROCESSABLE_ENTITY,
            "INVALID_MONEY",
        ),
    ] {
        let response = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/quantity")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        let bytes = axum::body::to_bytes(response.into_body(), 8192)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["error"]["code"],
            code
        );
    }
}

async fn send(body: &str, media: &str) -> (StatusCode, http::HeaderMap, serde_json::Value) {
    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/probe")
                .header("content-type", media)
                .header("x-request-id", "test-request")
                .body(Body::from(body.to_owned()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    (status, headers, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn opaque_ids_and_large_decimal_strings_round_trip_without_normalization() {
    let input = r#"{"id":"Variant/Mixed_Case:opaque","amount":"9223372036854775807","nested":{"label":"safe"}}"#;
    let (status, headers, body) = send(input, "application/json; charset=utf-8").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["x-request-id"], "test-request");
    assert_eq!(
        body,
        serde_json::from_str::<serde_json::Value>(input).unwrap()
    );
}

#[tokio::test]
async fn malformed_unknown_and_duplicate_members_return_correlated_sanitized_errors() {
    for input in [
        r#"{"id":"opaque","amount":"1","nested":{"label":"safe"},"secret":"do-not-echo"}"#,
        r#"{"id":"opaque","amount":"1","nested":{"label":"first","label":"do-not-echo"}}"#,
        r#"{"id":"opaque","id":"do-not-echo","amount":"1","nested":{"label":"safe"}}"#,
        r#"{"id":42,"amount":"1","nested":{"label":"safe"}}"#,
        r#"{"id":"opaque","amount":1,"nested":{"label":"safe"}}"#,
        "do-not-echo",
    ] {
        let (status, headers, body) = send(input, "application/json").await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{input}");
        assert_eq!(headers["x-request-id"], "test-request");
        assert_eq!(
            body,
            serde_json::json!({"error":{"code":"INVALID_REQUEST","message":"The request is invalid.","details":{},"request_id":"test-request"}})
        );
    }
}

#[tokio::test]
async fn decimal_encoding_is_canonical() {
    for amount in ["01", "-1", "+1", "1.0", "1e2", " 1", "", "1,000"] {
        let input = serde_json::json!({"id":"opaque", "amount":amount,"nested":{"label":"safe"}})
            .to_string();
        assert_eq!(
            send(&input, "application/json").await.0,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        send(
            r#"{"id":"opaque","amount":"0","nested":{"label":"safe"}}"#,
            "application/json"
        )
        .await
        .0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn media_type_and_actual_stream_size_are_bounded() {
    assert_eq!(
        send("{}", "text/plain").await.0,
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    let oversized = format!(
        r#"{{"id":"opaque","amount":"1","nested":{{"label":"{}"}}}}"#,
        "x".repeat(1024 * 1024)
    );
    let (status, _, body) = send(&oversized, "application/json").await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body["error"]["code"], "REQUEST_TOO_LARGE");
}
