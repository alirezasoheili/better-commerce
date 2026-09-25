use axum::{
    Extension, Json, Router,
    extract::Request,
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Serialize;

use crate::context::RequestContext;

pub fn router() -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .fallback(not_found)
        .layer(middleware::from_fn(provide_anonymous_context))
}

async fn provide_anonymous_context(mut request: Request, next: Next) -> Response {
    if request.extensions().get::<RequestContext>().is_none() {
        request.extensions_mut().insert(RequestContext::default());
    }

    next.run(request).await
}

async fn healthz(Extension(_context): Extension<RequestContext>) -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
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
