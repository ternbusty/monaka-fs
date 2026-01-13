//! Chunked WASI HTTP Client for AWS SDK
//!
//! Custom HTTP client implementation that chunks request bodies to work around
//! the WASI HTTP 4096 byte limit for blocking_write_and_flush.

use aws_smithy_runtime_api::client::connector_metadata::ConnectorMetadata;
use aws_smithy_runtime_api::{
    client::{
        http::{
            HttpClient, HttpConnector, HttpConnectorFuture, HttpConnectorSettings,
            SharedHttpClient, SharedHttpConnector,
        },
        orchestrator::HttpRequest,
        result::ConnectorError,
        runtime_components::RuntimeComponents,
    },
    http::Response,
    shared::IntoShared,
};
use aws_smithy_types::body::SdkBody;
use bytes::{Bytes, BytesMut};

use crate::wasi::clocks::monotonic_clock::subscribe_duration;
use crate::wasi::http::{
    outgoing_handler,
    types::{self as wasi_http, OutgoingBody, RequestOptions},
};
use crate::wasi::io::poll::poll;

/// Builder for [`ChunkedWasiHttpClient`].
#[derive(Default, Debug)]
pub struct ChunkedWasiHttpClientBuilder {}

impl ChunkedWasiHttpClientBuilder {
    /// Creates a new builder.
    pub fn new() -> Self {
        Default::default()
    }

    /// Builds the [`ChunkedWasiHttpClient`].
    pub fn build(self) -> SharedHttpClient {
        let client = ChunkedWasiHttpClient {};
        client.into_shared()
    }
}

/// An HTTP client that chunks request bodies to work around WASI HTTP limits.
#[derive(Debug, Clone)]
pub struct ChunkedWasiHttpClient {}

impl ChunkedWasiHttpClient {
    /// Create a new chunked WASI HTTP client.
    pub fn new() -> SharedHttpClient {
        ChunkedWasiHttpClientBuilder::new().build()
    }
}

impl HttpClient for ChunkedWasiHttpClient {
    fn http_connector(
        &self,
        settings: &HttpConnectorSettings,
        _components: &RuntimeComponents,
    ) -> SharedHttpConnector {
        let options = WasiRequestOptions::from(settings);
        let connector = ChunkedWasiHttpConnector { options };

        connector.into_shared()
    }

    fn connector_metadata(&self) -> Option<ConnectorMetadata> {
        Some(ConnectorMetadata::new("chunked-wasi-http-client", None))
    }
}

/// HTTP connector with chunked body writes
#[derive(Debug, Clone)]
struct ChunkedWasiHttpConnector {
    options: WasiRequestOptions,
}

impl HttpConnector for ChunkedWasiHttpConnector {
    fn call(&self, request: HttpRequest) -> HttpConnectorFuture {
        let options = self.options.clone();

        HttpConnectorFuture::new(async move {
            let client = WasiClient::new(options);
            let http_req = request.try_into_http1x().expect("Http request invalid");
            let converted_req = http_req.map(|body| match body.bytes() {
                Some(value) => Bytes::copy_from_slice(value),
                None => Bytes::new(),
            });

            let fut = client.handle_async(converted_req).await?;
            let response = fut.map(|body| {
                if body.is_empty() {
                    SdkBody::empty()
                } else {
                    SdkBody::from(body)
                }
            });

            let sdk_res = Response::try_from(response)
                .map_err(|err| ConnectorError::other(err.into(), None))?;

            Ok(sdk_res)
        })
    }
}

/// Poll timeout for async HTTP (1ms in nanoseconds)
const HTTP_POLL_TIMEOUT_NS: u64 = 1_000_000;

/// WASI HTTP client with streaming body writes
struct WasiClient {
    options: WasiRequestOptions,
}

impl WasiClient {
    fn new(options: WasiRequestOptions) -> Self {
        Self { options }
    }

    async fn handle_async(
        &self,
        req: http::Request<Bytes>,
    ) -> Result<http::Response<Bytes>, ConnectorError> {
        let (parts, body) = req.into_parts();

        let request = create_outgoing_request(&parts)
            .map_err(|err| ConnectorError::other(err.into(), None))?;

        let request_body = request.body().expect("Body accessed more than once");
        let request_stream = request_body
            .write()
            .expect("Output stream accessed more than once");

        let future_response = outgoing_handler::handle(request, self.options.clone().0)
            .map_err(|err| ConnectorError::other(err.into(), None))?;

        write_body_streaming_async(&request_stream, &body)
            .await
            .map_err(|err| ConnectorError::other(err.into(), None))?;

        drop(request_stream);
        OutgoingBody::finish(request_body, None)
            .map_err(|err| ConnectorError::other(err.into(), None))?;

        let subscription = future_response.subscribe();
        loop {
            let timeout = subscribe_duration(HTTP_POLL_TIMEOUT_NS);
            let ready = poll(&[&subscription, &timeout]);

            if ready.iter().any(|&i| i == 0) {
                break;
            }

            tokio::task::yield_now().await;
        }

        let incoming_res = future_response
            .get()
            .expect("Http response not ready")
            .expect("Http response accessed more than once")
            .map_err(|err| ConnectorError::other(err.into(), None))?;

        let response = http::Response::try_from(WasiResponse(incoming_res))
            .map_err(|err| ConnectorError::other(err.into(), None))?;

        Ok(response)
    }
}

fn create_outgoing_request(
    parts: &http::request::Parts,
) -> Result<wasi_http::OutgoingRequest, ParseError> {
    let method = convert_method(parts.method.clone())?;
    let path_with_query = parts.uri.path_and_query().map(|path| path.as_str());
    let headers = convert_headers(parts.headers.clone())?;
    let scheme = match parts.uri.scheme_str().unwrap_or("") {
        "http" => Some(&wasi_http::Scheme::Http),
        "https" => Some(&wasi_http::Scheme::Https),
        _ => None,
    };
    let authority = parts.uri.authority().map(|auth| auth.as_str());

    let request = wasi_http::OutgoingRequest::new(headers);
    request
        .set_scheme(scheme)
        .map_err(|_| ParseError::new("Failed to set HTTP scheme"))?;
    request
        .set_method(&method)
        .map_err(|_| ParseError::new("Failed to set HTTP method"))?;
    request
        .set_path_with_query(path_with_query)
        .map_err(|_| ParseError::new("Failed to set HTTP path"))?;
    request
        .set_authority(authority)
        .map_err(|_| ParseError::new("Failed to set HTTP authority"))?;

    Ok(request)
}

async fn write_body_streaming_async(
    stream: &wasi_http::OutputStream,
    body: &Bytes,
) -> Result<(), ParseError> {
    if body.is_empty() {
        return Ok(());
    }

    let mut offset = 0;
    while offset < body.len() {
        let permitted = stream
            .check_write()
            .map_err(|_| ParseError::new("Failed to check write capacity"))?;

        if permitted == 0 {
            let pollable = stream.subscribe();
            loop {
                let timeout = subscribe_duration(HTTP_POLL_TIMEOUT_NS);
                let ready = poll(&[&pollable, &timeout]);
                if ready.iter().any(|&i| i == 0) {
                    break;
                }
                tokio::task::yield_now().await;
            }
            continue;
        }

        let end = std::cmp::min(offset + permitted as usize, body.len());
        let chunk = &body[offset..end];

        stream
            .write(chunk)
            .map_err(|_| ParseError::new("Failed to write HTTP body chunk"))?;

        offset = end;
    }

    stream
        .flush()
        .map_err(|_| ParseError::new("Failed to start flush"))?;

    let pollable = stream.subscribe();
    loop {
        let timeout = subscribe_duration(HTTP_POLL_TIMEOUT_NS);
        let ready = poll(&[&pollable, &timeout]);
        if ready.iter().any(|&i| i == 0) {
            break;
        }
        tokio::task::yield_now().await;
    }

    Ok(())
}

#[derive(Debug)]
struct WasiRequestOptions(Option<outgoing_handler::RequestOptions>);

impl From<&HttpConnectorSettings> for WasiRequestOptions {
    fn from(value: &HttpConnectorSettings) -> Self {
        let connect_timeout = value
            .connect_timeout()
            .map(|dur| u64::try_from(dur.as_nanos()).unwrap_or(u64::MAX));
        let read_timeout = value
            .read_timeout()
            .map(|dur| u64::try_from(dur.as_nanos()).unwrap_or(u64::MAX));

        let wasi_http_opts = wasi_http::RequestOptions::new();
        wasi_http_opts
            .set_connect_timeout(connect_timeout)
            .expect("Connect timeout not supported");
        wasi_http_opts
            .set_first_byte_timeout(read_timeout)
            .expect("Read timeout not supported");

        WasiRequestOptions(Some(wasi_http_opts))
    }
}

impl Clone for WasiRequestOptions {
    fn clone(&self) -> Self {
        let new_opts = if let Some(opts) = &self.0 {
            let new_opts = RequestOptions::new();
            new_opts
                .set_between_bytes_timeout(opts.between_bytes_timeout())
                .expect("Between bytes timeout");
            new_opts
                .set_connect_timeout(opts.connect_timeout())
                .expect("Connect timeout");
            new_opts
                .set_first_byte_timeout(opts.first_byte_timeout())
                .expect("First byte timeout");

            Some(new_opts)
        } else {
            None
        };

        Self(new_opts)
    }
}

#[derive(Debug)]
struct ParseError(String);

impl ParseError {
    fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ParseError {}

fn convert_method(method: http::Method) -> Result<wasi_http::Method, ParseError> {
    Ok(match method {
        http::Method::GET => wasi_http::Method::Get,
        http::Method::POST => wasi_http::Method::Post,
        http::Method::PUT => wasi_http::Method::Put,
        http::Method::DELETE => wasi_http::Method::Delete,
        http::Method::PATCH => wasi_http::Method::Patch,
        http::Method::CONNECT => wasi_http::Method::Connect,
        http::Method::TRACE => wasi_http::Method::Trace,
        http::Method::HEAD => wasi_http::Method::Head,
        http::Method::OPTIONS => wasi_http::Method::Options,
        _ => return Err(ParseError::new("Unsupported HTTP method")),
    })
}

fn convert_headers(headers: http::HeaderMap) -> Result<wasi_http::Fields, ParseError> {
    let entries = headers
        .iter()
        .map(|(name, value)| {
            (
                name.to_string(),
                value.to_str().unwrap().as_bytes().to_vec(),
            )
        })
        .collect::<Vec<_>>();

    wasi_http::Fields::from_list(&entries).map_err(|err| ParseError::new(err.to_string()))
}

struct WasiResponse(wasi_http::IncomingResponse);

impl TryFrom<WasiResponse> for http::Response<Bytes> {
    type Error = ParseError;

    fn try_from(value: WasiResponse) -> Result<Self, Self::Error> {
        let response = value.0;
        let status = response.status();
        let headers = response.headers().entries();

        let res_build = headers
            .into_iter()
            .fold(http::Response::builder().status(status), |rb, header| {
                rb.header(header.0, header.1)
            });

        let body_incoming = response.consume().expect("Consume called more than once");
        let body_stream = body_incoming
            .stream()
            .expect("Stream accessed more than once");

        let mut body = BytesMut::new();

        while let Ok(stream_bytes) = body_stream.blocking_read(u64::MAX) {
            body.extend_from_slice(stream_bytes.as_slice())
        }

        drop(body_stream);

        let res = res_build
            .body(body.freeze())
            .map_err(|err| ParseError::new(err.to_string()))?;

        Ok(res)
    }
}
