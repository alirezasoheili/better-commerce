use std::sync::Arc;

use axum::{
    Extension, Json, Router,
    extract::Request,
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Serialize;

use super::{ApiError as M1Error, FrontendAssets, HttpRuntime, RequestId, boundary_layer};
use crate::{composition::Composition, context::RequestContext, database::ReadinessDatabase};

pub fn router(composition: Composition) -> Router {
    router_with_readiness(composition, None)
}

pub fn router_with_readiness(
    composition: Composition,
    readiness: Option<ReadinessDatabase>,
) -> Router {
    router_with_http(composition, readiness, None, None)
}

pub fn router_with_http(
    composition: Composition,
    readiness: Option<ReadinessDatabase>,
    runtime: Option<HttpRuntime>,
    assets: Option<FrontendAssets>,
) -> Router {
    let mut router = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/api/v1/admin/status", get(admin_status))
        .fallback(not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .layer(Extension(Arc::new(readiness)))
        .layer(Extension(Arc::new(composition)))
        .layer(middleware::from_fn(provide_anonymous_context));
    if let Some(assets) = assets {
        router = assets.attach(router);
        let csp = assets.csp;
        router = router.layer(middleware::from_fn(move |request: Request, next: Next| {
            let csp = csp.clone();
            async move {
                let mut response = next.run(request).await;
                response
                    .headers_mut()
                    .insert("content-security-policy", csp);
                response
            }
        }));
    }
    boundary_layer(router, runtime)
}

async fn admin_status() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status":"available", "capabilities":[]}))
}

async fn method_not_allowed(Extension(id): Extension<RequestId>, request: Request) -> Response {
    if !request.uri().path().starts_with("/api/v1/") {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    M1Error::new(
        StatusCode::METHOD_NOT_ALLOWED,
        "INVALID_REQUEST",
        "The method is not supported.",
        &id,
    )
    .into_response()
}

async fn provide_anonymous_context(mut request: Request, next: Next) -> Response {
    if request.extensions().get::<RequestContext>().is_none() {
        request.extensions_mut().insert(RequestContext::default());
    }

    next.run(request).await
}

async fn healthz(
    Extension(_context): Extension<RequestContext>,
    Extension(_composition): Extension<Arc<Composition>>,
) -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn readyz(
    Extension(_context): Extension<RequestContext>,
    Extension(composition): Extension<Arc<Composition>>,
    Extension(readiness): Extension<Arc<Option<ReadinessDatabase>>>,
) -> Response {
    if let Some(database) = readiness.as_ref() {
        if database.is_ready().await && composition.modules_are_ready().await {
            return (StatusCode::OK, Json(HealthResponse { status: "ok" })).into_response();
        }
    }
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ErrorEnvelope {
            error: ApiError {
                code: "not_ready",
                message: "Database prerequisites are not ready.",
            },
        }),
    )
        .into_response()
}

async fn not_found(Extension(id): Extension<RequestId>, request: Request) -> Response {
    if request.uri().path() == "/api/v1" || request.uri().path().starts_with("/api/v1/") {
        return M1Error::new(
            StatusCode::NOT_FOUND,
            "RESOURCE_NOT_FOUND",
            "The requested resource was not found.",
            &id,
        )
        .into_response();
    }
    (
        StatusCode::NOT_FOUND,
        Json(ErrorEnvelope {
            error: ApiError {
                code: "not_found",
                message: "The requested resource was not found.",
            },
        }),
    )
        .into_response()
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[derive(Serialize)]
struct ErrorEnvelope {
    error: ApiError,
}

#[derive(Serialize)]
struct ApiError {
    code: &'static str,
    message: &'static str,
}
