use crate::http::Client;
use apollo_router::{
    plugin::{Plugin, PluginInit},
    register_plugin,
    services::router,
    Endpoint, ListenAddr,
};
use futures::future::BoxFuture;
use http::uri::{Authority, Scheme, Uri};
use multimap::MultiMap;
use schemars::JsonSchema;
use serde::Deserialize;
use std::{
    sync::Arc,
    task::{Context, Poll},
};
use tower::{BoxError, Service, ServiceExt};
use url::Url;

register_plugin!("thehackerapp", "proxy", Proxy);

struct Proxy {
    address: ListenAddr,
    client: Client,
    routes: Vec<Route>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct Config {
    /// The address where the proxy should listen. You'll likely want this to be the same as the
    /// supergraph listen address
    listen: ListenAddr,

    /// The routes to transparently proxy through the router
    routes: Vec<Route>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct Route {
    /// The path spec to proxy
    path: String,

    /// The URI to proxy the request to as-is
    upstream: Url,
}

#[async_trait::async_trait]
impl Plugin for Proxy {
    type Config = Config;

    async fn new(init: PluginInit<Self::Config>) -> Result<Self, BoxError> {
        Ok(Self {
            address: init.config.listen,
            client: Client::new()?,
            routes: init.config.routes,
        })
    }

    fn web_endpoints(&self) -> MultiMap<ListenAddr, Endpoint> {
        let endpoints = self.routes.iter().map(|route| {
            Endpoint::from_router_service(
                route.path.to_owned(),
                ProxyService {
                    client: self.client.clone(),
                    upstream: Arc::new(route.upstream.clone()),
                }
                .boxed(),
            )
        });

        let mut map = MultiMap::with_capacity(1);
        map.insert_many(self.address.clone(), endpoints);
        map
    }
}

struct ProxyService {
    client: Client,
    upstream: Arc<Url>,
}

impl Service<router::Request> for ProxyService {
    type Response = router::Response;
    type Error = BoxError;
    type Future = BoxFuture<'static, Result<router::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.client.poll_ready(cx)
    }

    fn call(&mut self, mut req: router::Request) -> Self::Future {
        let upstream = self.upstream.clone();

        let client = self.client.clone();
        let mut client = std::mem::replace(&mut self.client, client);

        Box::pin(async move {
            req.router_request = {
                let (mut parts, body) = req.router_request.into_parts();

                parts.uri = {
                    let mut parts = parts.uri.into_parts();

                    let authority = Authority::try_from(upstream.authority())?;
                    parts.authority = Some(authority);

                    let scheme = Scheme::try_from(upstream.scheme())?;
                    parts.scheme = Some(scheme);

                    Uri::from_parts(parts)?
                };

                http::Request::from_parts(parts, body)
            };

            let response = client.call(req.into()).await?;
            Ok(response.into())
        })
    }
}
