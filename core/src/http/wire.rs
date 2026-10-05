//! Shared HTTP encodings. These types stay at the transport boundary.
use axum::{
    Json,
    body::to_bytes,
    extract::{FromRequest, Request},
    response::{IntoResponse, Response},
};
use http::StatusCode;
use serde::{
    Deserialize, Serialize,
    de::{self, DeserializeOwned, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Map, Value};
use std::fmt;

pub const MAX_BODY_BYTES: usize = 1024 * 1024;

#[derive(Clone)]
pub struct RequestId(pub String);

pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: &'static str,
    details: Value,
    request_id: String,
}

impl ApiError {
    pub fn new(
        status: StatusCode,
        code: &'static str,
        message: &'static str,
        request_id: &RequestId,
    ) -> Self {
        Self {
            status,
            code,
            message,
            details: serde_json::json!({}),
            request_id: request_id.0.clone(),
        }
    }

    pub fn invalid(request_id: &RequestId) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "INVALID_REQUEST",
            "The request is invalid.",
            request_id,
        )
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = details;
        self
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(serde_json::json!({"error": {
            "code":self.code, "message":self.message, "details":self.details, "request_id":self.request_id
        }}))).into_response()
    }
}

pub struct StrictJson<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned> FromRequest<S> for StrictJson<T> {
    type Rejection = ApiError;

    async fn from_request(request: Request, _state: &S) -> Result<Self, Self::Rejection> {
        let id = request
            .extensions()
            .get::<RequestId>()
            .cloned()
            .unwrap_or(RequestId("unavailable".into()));
        let mut content_types = request.headers().get_all(http::header::CONTENT_TYPE).iter();
        let supported = content_types
            .next()
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| {
                v.split(';')
                    .next()
                    .is_some_and(|media| media.trim().eq_ignore_ascii_case("application/json"))
            })
            && content_types.next().is_none();
        if !supported {
            return Err(ApiError::new(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "UNSUPPORTED_MEDIA_TYPE",
                "Use application/json.",
                &id,
            ));
        }
        let bytes = to_bytes(request.into_body(), MAX_BODY_BYTES)
            .await
            .map_err(|error| {
                use std::error::Error;
                if error
                    .source()
                    .is_some_and(|source| source.is::<http_body_util::LengthLimitError>())
                {
                    ApiError::new(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "REQUEST_TOO_LARGE",
                        "The request body exceeds 1 MiB.",
                        &id,
                    )
                } else {
                    ApiError::invalid(&id)
                }
            })?;
        let mut decoder = serde_json::Deserializer::from_slice(&bytes);
        let value = UniqueValue::deserialize(&mut decoder)
            .map_err(|_| ApiError::invalid(&id))?
            .0;
        decoder.end().map_err(|_| ApiError::invalid(&id))?;
        let input = serde_json::from_value(value).map_err(|_| ApiError::invalid(&id))?;
        Ok(Self(input))
    }
}

// A recursive visitor detects duplicates before serde_json::Value can erase them.
struct UniqueValue(Value);
impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D: de::Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        struct UniqueVisitor;
        impl<'de> Visitor<'de> for UniqueVisitor {
            type Value = UniqueValue;
            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("JSON without duplicated members")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut values = Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if values.contains_key(&key) {
                        return Err(de::Error::custom("duplicate member"));
                    }
                    values.insert(key, map.next_value::<UniqueValue>()?.0);
                }
                Ok(UniqueValue(Value::Object(values)))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element::<UniqueValue>()? {
                    values.push(value.0);
                }
                Ok(UniqueValue(Value::Array(values)))
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
                serde_json::Number::from_f64(v)
                    .map(|n| UniqueValue(Value::Number(n)))
                    .ok_or_else(|| de::Error::custom("invalid number"))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(UniqueValue(Value::Null))
            }
        }
        decoder.deserialize_any(UniqueVisitor)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct OpaqueId(pub String);
impl<'de> Deserialize<'de> for OpaqueId {
    fn deserialize<D: de::Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        let value = String::deserialize(decoder)?;
        if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
            return Err(de::Error::custom("invalid ID"));
        }
        Ok(Self(value))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct DecimalString(String);
impl DecimalString {
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// The owning endpoint maps range errors to its specific 422 business code.
    pub fn checked_i64(&self) -> Option<i64> {
        self.0.parse().ok()
    }
}
impl<'de> Deserialize<'de> for DecimalString {
    fn deserialize<D: de::Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        let value = String::deserialize(decoder)?;
        if value.is_empty()
            || !value.bytes().all(|b| b.is_ascii_digit())
            || (value.len() > 1 && value.starts_with('0'))
        {
            return Err(de::Error::custom("invalid decimal string"));
        }
        Ok(Self(value))
    }
}

#[derive(Serialize)]
pub struct ListResponse<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(transparent)]
pub struct Quantity(serde_json::Number);
impl Quantity {
    pub fn positive_i32(&self, request_id: &RequestId) -> Result<i32, ApiError> {
        self.0
            .as_i64()
            .and_then(|v| i32::try_from(v).ok())
            .filter(|v| *v > 0)
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "INVALID_QUANTITY",
                    "Use a positive whole quantity no greater than 2147483647.",
                    request_id,
                )
            })
    }
}

pub struct ListQuery {
    pub limit: u8,
    pub cursor: Option<String>,
}
impl<S: Send + Sync> axum::extract::FromRequestParts<S> for ListQuery {
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
        let mut limit = None;
        let mut cursor = None;
        let query = parts.uri.query().unwrap_or("");
        if query.len() > 2048 {
            return Err(ApiError::invalid(&id));
        }
        for (index, byte) in query.bytes().enumerate() {
            if byte == b'%'
                && !query
                    .as_bytes()
                    .get(index + 1..index + 3)
                    .is_some_and(|digits| digits.iter().all(u8::is_ascii_hexdigit))
            {
                return Err(ApiError::invalid(&id));
            }
        }
        percent_encoding::percent_decode_str(query)
            .decode_utf8()
            .map_err(|_| ApiError::invalid(&id))?;
        for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
            match key.as_ref() {
                "limit" if limit.is_none() => {
                    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                        return Err(ApiError::invalid(&id));
                    }
                    let parsed = value
                        .parse::<u8>()
                        .ok()
                        .filter(|v| (1..=100).contains(v))
                        .ok_or_else(|| ApiError::invalid(&id))?;
                    limit = Some(parsed);
                }
                "cursor"
                    if cursor.is_none()
                        && !value.is_empty()
                        && value.len() <= 1024
                        && !value.chars().any(char::is_control) =>
                {
                    cursor = Some(value.into_owned())
                }
                _ => return Err(ApiError::invalid(&id)),
            }
        }
        // Cursor interpretation/filter binding remains the owning capability's responsibility.
        Ok(Self {
            limit: limit.unwrap_or(25),
            cursor,
        })
    }
}
