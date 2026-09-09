use std::{
    convert::Infallible,
    error::Error,
    sync::Arc,
    time::{Duration, Instant},
};

use bytes::Bytes;
use futures_util::TryStreamExt;
use http::{
    HeaderMap, HeaderValue, Request, Response, StatusCode,
    header::{CONTENT_LENGTH, CONTENT_TYPE},
};
use http_body_util::{
    BodyExt, Full, StreamBody,
    combinators::UnsyncBoxBody,
};
use hyper::body::{Frame, Incoming};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use reqwest::{Client, Url};
use serde_json::Value;
use url::form_urlencoded;

use crate::config::Config;

const IGNORE_HEADERS: &[&str] = &[
    "host",
    "origin",
    "referer",
    "cdn-loop",
    "cf-",
    "x-",
    "range",
    "upgrade",
    "connection",
];

type BoxError = Box<dyn Error + Send + Sync>;
type ProxyBody = UnsyncBoxBody<Bytes, BoxError>;

pub struct AppState {
    pub config: Config,
    pub client: Client,
}

impl AppState {
    pub fn new(config: Config) -> Result<Self, reqwest::Error> {
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .pool_max_idle_per_host(10)
            .tls_danger_accept_invalid_certs(true)
            .build()?;

        Ok(Self { config, client })
    }
}

struct ProxyError {
    status: StatusCode,
    message: String,
}

impl ProxyError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

struct TransferLog {
    started: Instant,
    target: String,
    upstream: String,
    status: StatusCode,
    written: u64,
}

impl TransferLog {
    fn new(started: Instant, target: String, upstream: String, status: StatusCode) -> Self {
        Self {
            started,
            target,
            upstream,
            status,
            written: 0,
        }
    }
}

impl Drop for TransferLog {
    fn drop(&mut self) {
        eprintln!(
            "Proxy request -> {} , upstream={} , mode=stream , status={} , size={} bytes , cost={:?}",
            self.target,
            self.upstream,
            self.status.as_u16(),
            self.written,
            self.started.elapsed(),
        );
    }
}

pub async fn handle(request: Request<Incoming>, state: Arc<AppState>) -> Response<ProxyBody> {
    if request.uri().path() != state.config.proxy_path {
        return error_response(StatusCode::NOT_FOUND, "404 page not found");
    }

    match handle_proxy(request, state).await {
        Ok(response) => response,
        Err(error) => error_response(error.status, &error.message),
    }
}

async fn handle_proxy(
    request: Request<Incoming>,
    state: Arc<AppState>,
) -> Result<Response<ProxyBody>, ProxyError> {
    let started = Instant::now();
    let target_url = query_param(&request, "url")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ProxyError::new(StatusCode::BAD_REQUEST, "Missing url"))?;
    let upstream = query_param(&request, "upstream").unwrap_or_default();
    let token = query_param(&request, "token").unwrap_or_default();

    if !state.config.jwt_key.is_empty() && !validate_token(&token, &state.config.jwt_key) {
        return Err(ProxyError::new(
            StatusCode::UNAUTHORIZED,
            "Unauthorized: Invalid token",
        ));
    }

    let upstream_used = !upstream.is_empty();
    let request_url = if upstream_used {
        let mut url = Url::parse(&format!("https://{}{}", upstream, state.config.proxy_path))
            .map_err(|_| ProxyError::new(StatusCode::BAD_REQUEST, "Invalid upstream"))?;
        url.query_pairs_mut()
            .append_pair("url", &target_url)
            .append_pair("token", &token);
        url
    } else {
        Url::parse(&target_url)
            .map_err(|_| ProxyError::new(StatusCode::BAD_REQUEST, "Invalid url"))?
    };

    let request_headers = build_request_headers(request.headers(), upstream_used);
    let upstream_response = state
        .client
        .request(request.method().clone(), request_url)
        .headers(request_headers)
        .send()
        .await
        .map_err(|error| ProxyError::new(StatusCode::BAD_GATEWAY, error.to_string()))?;

    let upstream_status = upstream_response.status();
    let response_status = if upstream_status == StatusCode::PARTIAL_CONTENT {
        StatusCode::OK
    } else {
        upstream_status
    };
    let mut response_headers = upstream_response.headers().clone();
    let upstream_label = if upstream_used {
        upstream.clone()
    } else {
        "direct".to_owned()
    };

    if response_headers.contains_key(CONTENT_LENGTH) {
        let target_for_error = target_url.clone();
        let mut transfer_log = TransferLog::new(
            started,
            target_url,
            upstream_label,
            upstream_status,
        );

        let stream = upstream_response
            .bytes_stream()
            .map_ok(move |chunk| {
                transfer_log.written += chunk.len() as u64;
                Frame::data(chunk)
            })
            .map_err(move |error| -> BoxError {
                eprintln!(
                    "Error copying response body for {}: {}",
                    target_for_error, error
                );
                Box::new(error)
            });

        let body = StreamBody::new(stream).boxed_unsync();
        Ok(build_response(response_status, response_headers, body))
    } else {
        let body = upstream_response
            .bytes()
            .await
            .map_err(|_| ProxyError::new(StatusCode::BAD_GATEWAY, "Failed to read upstream body"))?;

        response_headers.insert(
            CONTENT_LENGTH,
            HeaderValue::from_str(&body.len().to_string())
                .expect("body length is always a valid Content-Length"),
        );

        eprintln!(
            "Proxy request -> {} , upstream={} , mode=read-all , status={} , size={} bytes , cost={:?}",
            target_url,
            upstream_label,
            upstream_status.as_u16(),
            body.len(),
            started.elapsed(),
        );

        Ok(build_response(
            response_status,
            response_headers,
            full_body(body),
        ))
    }
}

fn query_param(request: &Request<Incoming>, name: &str) -> Option<String> {
    let query = request.uri().query()?;
    form_urlencoded::parse(query.as_bytes())
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

fn build_request_headers(headers: &HeaderMap, preserve_range: bool) -> HeaderMap {
    let mut filtered = HeaderMap::new();

    for (name, value) in headers {
        let lower = name.as_str();
        let skip = IGNORE_HEADERS.iter().any(|ignored| {
            lower.starts_with(ignored) && !(preserve_range && *ignored == "range")
        });

        if !skip {
            filtered.append(name.clone(), value.clone());
        }
    }

    filtered
}

fn validate_token(token: &str, key: &[u8]) -> bool {
    if token.is_empty() {
        return false;
    }

    let Ok(header) = decode_header(token) else {
        return false;
    };

    if !matches!(
        header.alg,
        Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512
    ) {
        return false;
    }

    let mut validation = Validation::new(header.alg);
    validation.required_spec_claims.clear();
    validation.leeway = 0;
    validation.validate_nbf = true;
    validation.validate_aud = false;

    decode::<Value>(
        token,
        &DecodingKey::from_secret(key),
        &validation,
    )
    .is_ok()
}

fn build_response(
    status: StatusCode,
    headers: HeaderMap,
    body: ProxyBody,
) -> Response<ProxyBody> {
    let mut response = Response::new(body);
    *response.status_mut() = status;
    *response.headers_mut() = headers;
    response
}

fn error_response(status: StatusCode, message: &str) -> Response<ProxyBody> {
    let body = format!("{message}\n");
    let body_len = body.len();
    let mut response = Response::new(full_body(Bytes::from(body)));
    *response.status_mut() = status;
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response.headers_mut().insert(
        CONTENT_LENGTH,
        HeaderValue::from_str(&body_len.to_string())
            .expect("error body length is always a valid Content-Length"),
    );
    response
}

fn full_body(data: Bytes) -> ProxyBody {
    Full::new(data)
        .map_err(|never: Infallible| match never {})
        .boxed_unsync()
}
