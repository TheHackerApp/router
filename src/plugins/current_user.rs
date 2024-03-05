use super::authentication::fetch_context;
use crate::http::Client;
use apollo_router::{
    plugin::{Plugin, PluginInit},
    register_plugin,
    services::router,
    Endpoint, ListenAddr,
};
use context::{Scope, User};
use futures::future::BoxFuture;
use http::header::CONTENT_TYPE;
use multimap::MultiMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    sync::Arc,
    task::{Context, Poll},
};
use tower::{BoxError, Service, ServiceExt};
use url::Url;

register_plugin!("thehackerapp", "current_user", CurrentUser);

#[derive(Debug, Deserialize, JsonSchema)]
struct CurrentUser {
    /// The address where the proxy should listen. You'll likely want this to be the same as the
    /// supergraph listen address
    listen: ListenAddr,

    /// The path where the route should be served
    path: String,

    /// The upstream server for getting authentication info
    upstream: Url,
}

#[async_trait::async_trait]
impl Plugin for CurrentUser {
    type Config = CurrentUser;

    async fn new(init: PluginInit<Self::Config>) -> Result<Self, BoxError> {
        Ok(init.config)
    }

    fn web_endpoints(&self) -> MultiMap<ListenAddr, Endpoint> {
        let endpoint = Endpoint::from_router_service(
            self.path.clone(),
            CurrentUserService {
                client: Client::new().expect("client must build"),
                upstream: Arc::new(self.upstream.clone()),
            }
            .boxed(),
        );

        let mut map = MultiMap::with_capacity(1);
        map.insert(self.listen.clone(), endpoint);
        map
    }
}

struct CurrentUserService {
    client: Client,
    upstream: Arc<Url>,
}

impl Service<router::Request> for CurrentUserService {
    type Response = router::Response;
    type Error = BoxError;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.client.poll_ready(cx)
    }

    fn call(&mut self, req: router::Request) -> Self::Future {
        let upstream = self.upstream.clone();
        let client = self.client.clone();
        let mut client = std::mem::replace(&mut self.client, client);

        Box::pin(async move {
            let (req, scope, user) = match fetch_context(req, &upstream, &mut client).await? {
                Ok(values) => values,
                Err(response) => return Ok(response),
            };

            let body = hyper::Body::from(serde_json::to_vec(&ResponseBody { scope, user })?);
            let response = router::Response::builder()
                .header(CONTENT_TYPE, "application/json")
                .context(req.context)
                .build()?;

            Ok(response.map(|_body| body))
        })
    }
}

#[derive(Serialize)]
struct ResponseBody {
    scope: Scope,
    user: User,
}
