//! This module is largely copied from the Apollo Router source to utilize a [`hyper::Client`] from
//! within a plugin.
//!
//! Source: https://github.com/apollographql/router/blob/da64c28/apollo-router/src/services/http/service.rs

use apollo_router::{services::router, Context};
use futures::{
    future::{BoxFuture, TryFutureExt},
    Stream,
};
use http::header::{HeaderMap, HeaderValue, ACCEPT_ENCODING, CONTENT_ENCODING};
use hyper::{body::Bytes, client::HttpConnector};
use hyper_rustls::{ConfigBuilderExt, HttpsConnector};
use opentelemetry_api::global::get_text_map_propagator;
use pin_project_lite::pin_project;
use std::{io, task::Poll, time::Duration};
use tower::{BoxError, Service, ServiceBuilder};
use tower_http::decompression::{Decompression, DecompressionBody, DecompressionLayer};
use tracing::Instrument;
use tracing_opentelemetry::OpenTelemetrySpanExt;

mod resolver;

pub use hyper::Body;

static ACCEPTED_ENCODINGS: HeaderValue = HeaderValue::from_static("gzip, br, deflate");

type HttpClient =
    Decompression<hyper::Client<HttpsConnector<HttpConnector<resolver::AsyncResolver>>, Body>>;

pub struct Request {
    pub context: Context,
    pub request: http::Request<Body>,
}

impl From<router::Request> for Request {
    fn from(req: router::Request) -> Self {
        Self {
            request: req.router_request,
            context: req.context,
        }
    }
}

pub trait RequestBuilderExt {
    /// Create a new request with a body
    fn body_with_context(self, body: Body, context: Context) -> Result<Request, http::Error>;

    /// Create a new request with an empty body
    fn context(self, context: Context) -> Result<Request, http::Error>
    where
        Self: Sized,
    {
        self.body_with_context(Body::empty(), context)
    }
}

impl RequestBuilderExt for http::request::Builder {
    fn body_with_context(self, body: Body, context: Context) -> Result<Request, http::Error> {
        Ok(Request {
            context,
            request: self.body(body)?,
        })
    }
}

pub struct Response {
    pub context: Context,
    pub response: http::Response<Body>,
}

impl From<Response> for router::Response {
    fn from(resp: Response) -> Self {
        let mut router_response = router::Response::from(resp.response);
        router_response.context = resp.context;

        router_response
    }
}

#[derive(Clone)]
pub struct Client {
    client: HttpClient,
}

impl Client {
    pub fn new() -> Result<Self, io::Error> {
        let resolver = resolver::AsyncResolver::new()?;
        let mut http_connector = HttpConnector::new_with_resolver(resolver);
        http_connector.set_nodelay(true);
        http_connector.set_keepalive(Some(Duration::from_secs(60)));
        http_connector.enforce_http(false);

        let tls_config = rustls::ClientConfig::builder()
            .with_safe_defaults()
            .with_native_roots()
            .with_no_client_auth();

        let https_connector = hyper_rustls::HttpsConnectorBuilder::new()
            .with_tls_config(tls_config)
            .https_or_http()
            .enable_http1()
            .enable_http2()
            .wrap_connector(http_connector);

        let client = hyper::Client::builder()
            .pool_idle_timeout(Some(Duration::from_secs(5)))
            .build(https_connector);

        let client = ServiceBuilder::new()
            .layer(DecompressionLayer::new())
            .service(client);

        Ok(Self { client })
    }
}

impl Service<Request> for Client {
    type Response = Response;
    type Error = BoxError;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.client
            .poll_ready(cx)
            .map(|res| res.map_err(|e| Box::new(e) as BoxError))
    }

    fn call(&mut self, req: Request) -> Self::Future {
        let Request {
            context,
            mut request,
        } = req;

        let uri = request.uri();
        let path = uri.path();
        let host = uri.host().unwrap_or_default();
        let port = uri.port_u16().unwrap_or_else(|| {
            let scheme = uri.scheme_str();
            match scheme {
                Some("https") => 443,
                Some("http") => 80,
                _ => 0,
            }
        });

        let request_span = tracing::info_span!(
            "http_request",
            otel.kind = "CLIENT",
            net.peer.name = %host,
            net.peer.port = %port,
            http.route = %path,
            http.url = %uri,
            net.transport = "ip_tcp",
        );
        get_text_map_propagator(|propagator| {
            let mut injector = opentelemetry_http::HeaderInjector(request.headers_mut());
            propagator.inject_context(&request_span.context(), &mut injector)
        });

        let client = self.client.clone();
        Box::pin(async move {
            let (parts, body) = request.into_parts();
            let body = hyper::body::to_bytes(body).await.map_err(|err| {
                tracing::error!(compress_error = debug(&err));
                err
            })?;
            let body = compress(body, &parts.headers)
                .instrument(tracing::debug_span!("body_compression"))
                .await
                .map_err(|err| {
                    tracing::error!(compress_error = debug(&err));
                    err
                })?;
            let mut request = http::Request::from_parts(parts, Body::from(body));

            request
                .headers_mut()
                .insert(ACCEPT_ENCODING, ACCEPTED_ENCODINGS.clone());

            let display_headers =
                context.contains_key("apollo_telemetry::logging::display_headers");
            if display_headers {
                tracing::info!(http.request.headers = ?request.headers());
            }
            if context.contains_key("apollo_telemetry::logging::display_body") {
                tracing::info!(http.request.body = ?request.body());
            }

            let response = fetch(client, &context, request)
                .instrument(request_span)
                .await?;

            if display_headers {
                tracing::info!(response.headers = ?response.headers());
            }

            Ok(Response { response, context })
        })
    }
}

async fn compress(body: Bytes, headers: &HeaderMap) -> Result<Bytes, BoxError> {
    let content_encoding = headers
        .get(&CONTENT_ENCODING)
        .map(|header| header.to_str())
        .transpose()?;
    match content_encoding {
        Some("br") => todo!(),
        Some("gzip") => todo!(),
        Some("deflate") => todo!(),
        Some("identity") | None => Ok(body),
        Some(encoding) => {
            tracing::error!(%encoding, "unknown content-encoding value");
            Err(BoxError::from(format!(
                "unknown content-encoding {encoding:?}"
            )))
        }
    }
}

async fn fetch(
    mut client: HttpClient,
    context: &Context,
    request: http::Request<Body>,
) -> Result<http::Response<Body>, BoxError> {
    let _active_request_guard = context.enter_active_request();

    let (parts, body) = client
        .call(request)
        .map_err(|err| {
            tracing::error!(fetch_error = ?err);
            err
        })
        .await?
        .into_parts();

    Ok(http::Response::from_parts(
        parts,
        Body::wrap_stream(BodyStream { inner: body }),
    ))
}

pin_project! {
    pub(crate) struct BodyStream<B: hyper::body::HttpBody> {
        #[pin]
        inner: DecompressionBody<B>
    }
}

impl<B> Stream for BodyStream<B>
where
    B: hyper::body::HttpBody,
    B::Error: Into<tower_http::BoxError>,
{
    type Item = Result<Bytes, BoxError>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        use hyper::body::HttpBody;

        self.project().inner.poll_data(cx)
    }
}
