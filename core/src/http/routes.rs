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

use crate::{composition::Composition, context::RequestContext, database::ReadinessDatabase};

pub fn router(composition: Composition) -> Router {
    router_with_readiness(composition, None)
}

pub fn router_with_readiness(
    composition: Composition,
    readiness: Option<ReadinessDatabase>,
) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .fallback(not_found)
        .layer(Extension(Arc::new(readiness)))
        .layer(Extension(Arc::new(composition)))
        .layer(middleware::from_fn(provide_anonymous_context))
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

async fn not_found() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorEnvelope {
            error: ApiError {
                code: "not_found",
                message: "The requested resource was not found.",
            },
        }),
    )
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
