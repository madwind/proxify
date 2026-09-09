use std::{
    error::Error,
    fmt,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use bytes::Bytes;
use http::{
    HeaderMap, HeaderValue, Method, Request, Response, StatusCode,
    header::{
        AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, COOKIE, LOCATION, PROXY_AUTHORIZATION, REFERER,
        TRANSFER_ENCODING,
    },
};
use http_body_util::Empty;
use hyper::body::{Body, Frame, Incoming};
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::{
    client::legacy::{Client, connect::HttpConnector},
    rt::TokioExecutor,
};
use rustls::{
    DigitallySignedStruct, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::CryptoProvider,
    pki_types::{CertificateDer, ServerName, UnixTime},
};
use tokio::time::{Instant, Sleep, timeout_at};
use url::Url;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_REDIRECTS: usize = 10;

type HyperClient = Client<HttpsConnector<HttpConnector>, Empty<Bytes>>;

pub struct OutboundClient {
    inner: HyperClient,
}

impl OutboundClient {
    pub fn new() -> Result<Self, rustls::Error> {
        let provider = rustls::crypto::aws_lc_rs::default_provider();
        let tls = rustls::ClientConfig::builder_with_provider(Arc::new(provider.clone()))
            .with_safe_default_protocol_versions()?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(DangerIgnoreVerifier(provider)))
            .with_no_client_auth();

        let https = HttpsConnectorBuilder::new()
            .with_tls_config(tls)
            .https_or_http()
            .enable_http1()
            .enable_http2()
            .build();

        let client = Client::builder(TokioExecutor::new())
            .pool_idle_timeout(None)
            .pool_max_idle_per_host(10)
            .build(https);

        Ok(Self { inner: client })
    }

    pub async fn request(
        &self,
        method: Method,
        mut url: Url,
        mut headers: HeaderMap,
    ) -> Result<Response<DeadlineBody>, ClientError> {
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        let mut method = method;

        for redirects in 0..=MAX_REDIRECTS {
            let request = build_request(&method, &url, &headers)?;
            let response = timeout_at(deadline, self.inner.request(request))
                .await
                .map_err(|_| ClientError::new("request timed out"))?
                .map_err(|error| ClientError::new(error.to_string()))?;

            let status = response.status();
            if !is_redirect(status) {
                return Ok(response.map(|body| DeadlineBody::new(body, deadline)));
            }

            let Some(location) = response.headers().get(LOCATION) else {
                return Ok(response.map(|body| DeadlineBody::new(body, deadline)));
            };

            if redirects == MAX_REDIRECTS {
                return Err(ClientError::new("too many redirects"));
            }

            let location = location
                .to_str()
                .map_err(|_| ClientError::new("invalid redirect location"))?;
            let next_url = url
                .join(location)
                .map_err(|error| ClientError::new(format!("invalid redirect: {error}")))?;

            if !matches!(next_url.scheme(), "http" | "https") {
                return Err(ClientError::new("redirect to unsupported scheme"));
            }

            if !same_origin(&url, &next_url) {
                headers.remove(AUTHORIZATION);
                headers.remove(COOKIE);
                headers.remove(PROXY_AUTHORIZATION);
            }

            update_referer(&mut headers, &url, &next_url);

            let next_method = redirect_method(&method, status);
            if next_method != method {
                headers.remove(CONTENT_LENGTH);
                headers.remove(CONTENT_TYPE);
                headers.remove(TRANSFER_ENCODING);
            }

            method = next_method;
            url = next_url;
        }

        unreachable!("redirect loop exits through a response or error")
    }
}

fn build_request(
    method: &Method,
    url: &Url,
    headers: &HeaderMap,
) -> Result<Request<Empty<Bytes>>, ClientError> {
    let mut request_url = url.clone();
    request_url.set_fragment(None);

    let mut request = Request::builder()
        .method(method.clone())
        .uri(request_url.as_str())
        .body(Empty::new())
        .map_err(|error| ClientError::new(error.to_string()))?;
    *request.headers_mut() = headers.clone();
    Ok(request)
}

fn is_redirect(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::MOVED_PERMANENTLY
            | StatusCode::FOUND
            | StatusCode::SEE_OTHER
            | StatusCode::TEMPORARY_REDIRECT
            | StatusCode::PERMANENT_REDIRECT
    )
}

fn redirect_method(method: &Method, status: StatusCode) -> Method {
    match status {
        StatusCode::SEE_OTHER if method != Method::HEAD => Method::GET,
        StatusCode::MOVED_PERMANENTLY | StatusCode::FOUND
            if method != Method::GET && method != Method::HEAD =>
        {
            Method::GET
        }
        _ => method.clone(),
    }
}

fn same_origin(left: &Url, right: &Url) -> bool {
    left.scheme() == right.scheme()
        && left.host_str() == right.host_str()
        && left.port_or_known_default() == right.port_or_known_default()
}

fn update_referer(headers: &mut HeaderMap, previous: &Url, next: &Url) {
    if previous.scheme() == "https" && next.scheme() == "http" {
        headers.remove(REFERER);
        return;
    }

    let mut referer = previous.clone();
    let _ = referer.set_username("");
    let _ = referer.set_password(None);
    referer.set_fragment(None);

    match HeaderValue::from_str(referer.as_str()) {
        Ok(value) => {
            headers.insert(REFERER, value);
        }
        Err(_) => {
            headers.remove(REFERER);
        }
    }
}

#[derive(Debug)]
pub struct ClientError {
    message: String,
}

impl ClientError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for ClientError {}

#[derive(Debug)]
pub enum ClientBodyError {
    Hyper(hyper::Error),
    Timeout,
}

impl fmt::Display for ClientBodyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hyper(error) => write!(formatter, "{error}"),
            Self::Timeout => formatter.write_str("request timed out"),
        }
    }
}

impl Error for ClientBodyError {}

pub struct DeadlineBody {
    inner: Pin<Box<Incoming>>,
    timeout: Pin<Box<Sleep>>,
}

impl DeadlineBody {
    fn new(inner: Incoming, deadline: Instant) -> Self {
        Self {
            inner: Box::pin(inner),
            timeout: Box::pin(tokio::time::sleep_until(deadline)),
        }
    }
}

impl Body for DeadlineBody {
    type Data = Bytes;
    type Error = ClientBodyError;

    fn poll_frame(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();

        match this.inner.as_mut().poll_frame(context) {
            Poll::Ready(Some(Ok(frame))) => Poll::Ready(Some(Ok(frame))),
            Poll::Ready(Some(Err(error))) => {
                Poll::Ready(Some(Err(ClientBodyError::Hyper(error))))
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => match this.timeout.as_mut().poll(context) {
                Poll::Ready(()) => Poll::Ready(Some(Err(ClientBodyError::Timeout))),
                Poll::Pending => Poll::Pending,
            },
        }
    }
}

#[derive(Debug)]
struct DangerIgnoreVerifier(CryptoProvider);

impl ServerCertVerifier for DangerIgnoreVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signed: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            certificate,
            signed,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signed: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            certificate,
            signed,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}
