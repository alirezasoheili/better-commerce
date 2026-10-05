use std::{
    net::{IpAddr, SocketAddr},
    path::Path,
    sync::Arc,
};

use axum::{
    Router,
    extract::{ConnectInfo, Request},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use http::{HeaderValue, StatusCode, header};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use super::{ApiError, RequestId};
use crate::manifest::{HttpConfiguration, SecretReference};

/// Resolved transport configuration retains only a fixed-length admin verifier.
#[derive(Clone)]
pub struct HttpRuntime {
    origin: String,
    authority: String,
    loopback_http: bool,
    trusted_proxy_ip: Option<IpAddr>,
    admin_verifier: [u8; 32],
    admin_token_length: usize,
}

#[derive(Debug)]
pub struct HttpConfigurationError;
impl std::fmt::Display for HttpConfigurationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HTTP configuration requires a canonical HTTPS origin (HTTP only on loopback), a valid secret reference, and a random base64url admin token of at least 32 bytes")
    }
}
impl std::error::Error for HttpConfigurationError {}

impl HttpRuntime {
    pub fn resolve(
        configuration: &HttpConfiguration,
        manifest_dir: &Path,
    ) -> Result<Self, HttpConfigurationError> {
        Self::resolve_with_environment(configuration, manifest_dir, &|name| std::env::var(name))
    }

    pub(crate) fn resolve_with_environment<F>(
        configuration: &HttpConfiguration,
        manifest_dir: &Path,
        get_env: &F,
    ) -> Result<Self, HttpConfigurationError>
    where
        F: Fn(&str) -> Result<String, std::env::VarError>,
    {
        let value = crate::secrets::resolve(&configuration.admin_token, manifest_dir, get_env)
            .map_err(|_| HttpConfigurationError)?;
        Self::from_resolved_token(configuration, &value)
    }

    pub fn from_resolved_token(
        configuration: &HttpConfiguration,
        token: &str,
    ) -> Result<Self, HttpConfigurationError> {
        let valid_reference = match &configuration.admin_token {
            SecretReference::Environment { env } => {
                !env.is_empty() && env.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            }
            SecretReference::File { file } => !file.as_os_str().is_empty(),
        };
        let url =
            url::Url::parse(&configuration.public_origin).map_err(|_| HttpConfigurationError)?;
        let loopback = url.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .trim_matches(['[', ']'])
                    .parse::<IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
        let origin = url.origin().ascii_serialization();
        if !valid_reference
            || (url.scheme() == "https" && configuration.trusted_proxy_ip.is_none())
            || origin != configuration.public_origin
            || !url.username().is_empty()
            || url.password().is_some()
            || !(url.scheme() == "https" || (url.scheme() == "http" && loopback))
        {
            return Err(HttpConfigurationError);
        }
        let decoded = URL_SAFE_NO_PAD
            .decode(token)
            .map_err(|_| HttpConfigurationError)?;
        if !(32..=128).contains(&decoded.len()) {
            return Err(HttpConfigurationError);
        }
        Ok(Self {
            authority: url[url::Position::BeforeHost..url::Position::AfterPort].to_owned(),
            origin,
            loopback_http: url.scheme() == "http" && loopback,
            trusted_proxy_ip: configuration.trusted_proxy_ip,
            admin_verifier: Sha256::digest(token.as_bytes()).into(),
            admin_token_length: token.len(),
        })
    }

    fn credential_transport_allowed(&self, request: &Request) -> bool {
        self.transport_allowed(request.headers(), request.extensions())
    }

    fn transport_allowed(&self, headers: &http::HeaderMap, extensions: &http::Extensions) -> bool {
        let peer = extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|info| info.0.ip());
        let host = single_header_value(headers, header::HOST.as_str());
        if host != Some(self.authority.as_str()) {
            return false;
        }
        if self.loopback_http {
            return peer.is_some_and(|ip| ip.is_loopback() || Some(ip) == self.trusted_proxy_ip);
        }
        peer.is_some_and(|ip| Some(ip) == self.trusted_proxy_ip)
            && single_header_value(headers, "x-forwarded-proto") == Some("https")
    }

    fn admin_authorized(&self, request: &Request) -> bool {
        let token = single_header(request, header::AUTHORIZATION.as_str())
            .and_then(|v| v.strip_prefix("Bearer "));
        let Some(token) = token.filter(|v| {
            !v.is_empty()
                && v.len() <= 172
                && v.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        }) else {
            return false;
        };
        let supplied: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        bool::from(self.admin_verifier.ct_eq(&supplied))
    }

    fn contains_admin_token(&self, value: &str) -> bool {
        // A correlation ID must not be usable to reflect the configured credential.
        value
            .as_bytes()
            .windows(self.admin_token_length)
            .any(|candidate| {
                let supplied: [u8; 32] = Sha256::digest(candidate).into();
                bool::from(self.admin_verifier.ct_eq(&supplied))
            })
    }
}

fn single_header<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    single_header_value(request.headers(), name)
}

fn single_header_value<'a>(headers: &'a http::HeaderMap, name: &str) -> Option<&'a str> {
    let mut values = headers.get_all(name).iter();
    let first = values.next()?.to_str().ok()?;
    values.next().is_none().then_some(first)
}

/// Secret-returning endpoints must check transport even before they have a bearer.
pub struct CredentialTransport(());
impl<S: Send + Sync> axum::extract::FromRequestParts<S> for CredentialTransport {
    type Rejection = ApiError;
    async fn from_request_parts(
        parts: &mut http::request::Parts,
        _: &S,
    ) -> Result<Self, Self::Rejection> {
        let id = parts
            .extensions
            .get::<RequestId>()
            .cloned()
            .unwrap_or(RequestId("unavailable".into()));
        if parts
            .extensions
            .get::<Arc<Option<HttpRuntime>>>()
            .is_some_and(|runtime| {
                runtime
                    .as_ref()
                    .as_ref()
                    .is_some_and(|r| r.transport_allowed(&parts.headers, &parts.extensions))
            })
        {
            Ok(Self(()))
        } else {
            Err(ApiError::invalid(&id))
        }
    }
}

pub struct SecretJson<T> {
    value: T,
}
impl<T> SecretJson<T> {
    pub fn new(value: T, _transport: CredentialTransport) -> Self {
        Self { value }
    }
}
impl<T: serde::Serialize> IntoResponse for SecretJson<T> {
    fn into_response(self) -> Response {
        let mut response = axum::Json(self.value).into_response();
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        response
    }
}

pub fn boundary_layer(router: Router, runtime: Option<HttpRuntime>) -> Router {
    router.layer(middleware::from_fn(move |request, next| {
        boundary(request, next, Arc::new(runtime.clone()))
    }))
}

async fn boundary(mut request: Request, next: Next, runtime: Arc<Option<HttpRuntime>>) -> Response {
    let supplied = single_header(&request, "x-request-id").filter(|v| {
        !v.is_empty()
            && v.len() <= 128
            && v.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            && !runtime
                .as_ref()
                .as_ref()
                .is_some_and(|r| r.contains_admin_token(v))
            && !request
                .headers()
                .get_all(header::AUTHORIZATION)
                .iter()
                .filter_map(|header| header.to_str().ok())
                .filter_map(|header| header.split_once(' ').map(|(_, token)| token))
                .any(|token| !token.is_empty() && v.contains(token))
    });
    let id = RequestId(supplied.map(str::to_owned).unwrap_or_else(|| {
        let mut bytes = [0u8; 16];
        if getrandom::getrandom(&mut bytes).is_ok() {
            URL_SAFE_NO_PAD.encode(bytes)
        } else {
            static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
            format!(
                "request-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            )
        }
    }));
    request.extensions_mut().insert(id.clone());
    request.extensions_mut().insert(runtime.clone());
    let path = request.uri().path();
    let api = path == "/api/v1" || path.starts_with("/api/v1/");
    let admin = path == "/api/v1/admin" || path.starts_with("/api/v1/admin/");
    let no_store = request.headers().contains_key(header::AUTHORIZATION) || admin;
    let mutation = !matches!(
        *request.method(),
        http::Method::GET | http::Method::HEAD | http::Method::OPTIONS
    );
    let invalid_origin = api
        && mutation
        && request.headers().contains_key(header::ORIGIN)
        && single_header(&request, header::ORIGIN.as_str())
            != runtime.as_ref().as_ref().map(|r| r.origin.as_str());
    let insecure_credential = api
        && request.headers().contains_key(header::AUTHORIZATION)
        && !runtime
            .as_ref()
            .as_ref()
            .is_some_and(|r| r.credential_transport_allowed(&request));
    let rejection = if invalid_origin || insecure_credential {
        Some(ApiError::invalid(&id))
    } else if admin {
        match runtime.as_ref().as_ref() {
            None => Some(ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "NOT_READY",
                "The admin boundary is not configured.",
                &id,
            )),
            Some(r) if !r.admin_authorized(&request) => Some(ApiError::new(
                StatusCode::UNAUTHORIZED,
                "ADMIN_AUTHORITY_INVALID",
                "The installation token is invalid.",
                &id,
            )),
            _ => None,
        }
    } else {
        None
    };
    let mut response = if let Some(error) = rejection {
        error.into_response()
    } else {
        next.run(request).await
    };
    response.headers_mut().insert(
        "x-request-id",
        HeaderValue::from_str(&id.0).expect("validated request ID"),
    );
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    response
        .headers_mut()
        .insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    if no_store {
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    if !response.headers().contains_key("content-security-policy") {
        response.headers_mut().insert("content-security-policy", HeaderValue::from_static("default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'self'; frame-ancestors 'none'; form-action 'self'"));
    }
    response
}
