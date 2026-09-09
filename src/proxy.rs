use std::{
    convert::Infallible,
    error::Error,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use bytes::Bytes;
use futures_util::TryStreamExt;
use http::{
    HeaderMap, HeaderValue, Request, Response, StatusCode,
    header::{ACCEPT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, TRANSFER_ENCODING},
};
use http_body_util::{
    BodyExt, Full, StreamBody,
    combinators::UnsyncBoxBody,
};
use hyper::body::{Frame, Incoming};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
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
    "sec-",
    "accept-encoding",
    "range",
    "upgrade",
    "connection",
];

type BoxError = Box<dyn Error + Send + Sync>;
type ProxyBody = UnsyncBoxBody<Bytes, BoxError>;

struct JwtVerifier {
    key: DecodingKey,
    validation: Validation,
}

pub struct AppState {
    pub config: Config,
    pub client: Client,
    jwt: Option<JwtVerifier>,
}

impl AppState {
    pub fn new(config: Config) -> Result<Self, reqwest::Error> {
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .no_proxy()
            .pool_idle_timeout(None)
            .pool_max_idle_per_host(10)
            .tls_danger_accept_invalid_certs(true)
            .build()?;

        let jwt = if config.jwt_key.is_empty() {
            None
        } else {
            let mut validation = Validation::new(Algorithm::HS256);
            validation.algorithms = vec![Algorithm::HS256, Algorithm::HS384, Algorithm::HS512];
            validation.required_spec_claims.clear();
            validation.leeway = 0;
            validation.validate_nbf = true;
            validation.validate_aud = false;

            Some(JwtVerifier {
                key: DecodingKey::from_secret(&config.jwt_key),
                validation,
            })
        };

        Ok(Self {
            config,
            client,
            jwt,
        })
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

#[derive(Default)]
struct ProxyQuery {
    url: Option<String>,
    upstream: Option<String>,
    token: Option<String>,
}

struct TransferLog {
    started: Instant,
    target: String,
    upstream: String,
    status: StatusCode,
    written: AtomicU64,
}

impl TransferLog {
    fn new(started: Instant, target: String, upstream: String, status: StatusCode) -> Self {
        Self {
            started,
            target,
            upstream,
            status,
            written: AtomicU64::new(0),
        }
    }

    fn add_written(&self, bytes: u64) {
        self.written.fetch_add(bytes, Ordering::Relaxed);
    }
}

impl Drop for TransferLog {
    fn drop(&mut self) {
        eprintln!(
            "Proxy request -> {} , upstream={} , mode=stream , status={} , size={} bytes , cost={:?}",
            self.target,
            self.upstream,
            self.status.as_u16(),
            self.written.load(Ordering::Relaxed),
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
    let query = parse_query(&request);
    let target_url = query
        .url
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ProxyError::new(StatusCode::BAD_REQUEST, "Missing url"))?;
    let upstream = query.upstream.unwrap_or_default();
    let token = query.token.unwrap_or_default();

    if let Some(jwt) = &state.jwt
        && !validate_token(&token, jwt)
    {
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
    let mut response_headers = upstream_response.headers().clone();
    let upstream_label = if upstream_used {
        upstream.clone()
    } else {
        "direct".to_owned()
    };

    if response_headers.contains_key(CONTENT_LENGTH) {
        let target_for_error = target_url.clone();
        let transfer_log = Arc::new(TransferLog::new(
            started,
            target_url,
            upstream_label,
            upstream_status,
        ));
        let stream_log = Arc::clone(&transfer_log);

        let stream = upstream_response
            .bytes_stream()
            .map_ok(move |chunk| {
                stream_log.add_written(chunk.len() as u64);
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
        Ok(build_response(upstream_status, response_headers, body))
    } else {
        let body = upstream_response
            .bytes()
            .await
            .map_err(|_| ProxyError::new(StatusCode::BAD_GATEWAY, "Failed to read upstream body"))?;

        response_headers.remove(TRANSFER_ENCODING);
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
            upstream_status,
            response_headers,
            full_body(body),
        ))
    }
}

fn parse_query(request: &Request<Incoming>) -> ProxyQuery {
    let Some(query) = request.uri().query() else {
        return ProxyQuery::default();
    };

    let mut result = ProxyQuery::default();
    for (key, value) in form_urlencoded::parse(query.as_bytes()) {
        match key.as_ref() {
            "url" if result.url.is_none() => result.url = Some(value.into_owned()),
            "upstream" if result.upstream.is_none() => {
                result.upstream = Some(value.into_owned());
            }
            "token" if result.token.is_none() => result.token = Some(value.into_owned()),
            _ => {}
        }
    }

    result
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

    filtered.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));
    filtered
}

fn validate_token(token: &str, jwt: &JwtVerifier) -> bool {
    !token.is_empty() && decode::<Value>(token, &jwt.key, &jwt.validation).is_ok()
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
