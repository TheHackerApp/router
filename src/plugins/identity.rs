use apollo_router::{
    layers::ServiceBuilderExt,
    plugin::{Plugin, PluginInit},
    register_plugin,
    services::{subgraph, supergraph},
};
use context::user::{Context, Params};
use headers::{
    authorization::{Authorization, Bearer},
    HeaderMapExt,
};
use reqwest::Client;
use schemars::JsonSchema;
use serde::Deserialize;
use std::borrow::Cow;
use std::{ops::ControlFlow, sync::Arc, time::Duration};
use tower::{BoxError, ServiceBuilder, ServiceExt};
use url::Url;

pub(crate) const IDENTITY_CONTEXT_KEY: &str = "thehackerapp/identity";

register_plugin!("thehackerapp", "identity", Identity);

#[derive(Debug)]
struct Identity {
    client: Client,
    url: Arc<Url>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct Config {
    /// The URL to fetch event context from
    url: Url,
}

#[async_trait::async_trait]
impl Plugin for Identity {
    type Config = Config;

    async fn new(init: PluginInit<Self::Config>) -> Result<Self, BoxError> {
        let client = Client::builder()
            .user_agent("apollo-")
            .timeout(Duration::from_secs(1))
            .connect_timeout(Duration::from_secs(1))
            .build()?;

        Ok(Identity {
            client,
            url: Arc::new(init.config.url),
        })
    }

    // Checks that the user's session is valid
    fn supergraph_service(&self, service: supergraph::BoxService) -> supergraph::BoxService {
        let client = self.client.clone();
        let url = self.url.clone();

        ServiceBuilder::new()
            .oneshot_checkpoint_async(move |req: supergraph::Request| {
                let client = client.clone();
                let url = url.clone();

                async move {
                    let context = if let Some(authorization) = req
                        .supergraph_request
                        .headers()
                        .typed_get::<Authorization<Bearer>>()
                    {
                        client
                            .get(url.as_str())
                            .query(&Params {
                                token: Cow::Borrowed(authorization.token()),
                            })
                            .send()
                            .await?
                            .error_for_status()?
                            .json::<Context>()
                            .await?
                    } else {
                        Context::Unauthenticated
                    };

                    req.context.insert(IDENTITY_CONTEXT_KEY, context)?;
                    Ok(ControlFlow::Continue(req))
                }
            })
            .service(service)
            .boxed()
    }

    // Adds authentication headers to the request
    fn subgraph_service(&self, _name: &str, service: subgraph::BoxService) -> subgraph::BoxService {
        ServiceBuilder::new()
            .service(service)
            .map_request(|mut req: subgraph::Request| {
                let context = req
                    .context
                    .get::<_, Context>(IDENTITY_CONTEXT_KEY)
                    .expect("identity context must deserialize")
                    .expect("identity context must exist");

                let headers = req.subgraph_request.headers_mut();
                context.write_headers(headers);

                req
            })
            .boxed()
    }
}
