use crate::error_response;
use apollo_router::{
    layers::ServiceBuilderExt,
    plugin::{Plugin, PluginInit},
    register_plugin,
    services::{subgraph, supergraph},
};
use context::{
    event::{Context, Params},
    EventSlug,
};
use headers::{HeaderMapExt, Host};
use reqwest::{Client, StatusCode};
use schemars::JsonSchema;
use serde::Deserialize;
use std::{borrow::Cow, ops::ControlFlow, sync::Arc, time::Duration};
use tower::{BoxError, ServiceBuilder, ServiceExt};
use url::Url;

pub(crate) const EVENT_CONTEXT_KEY: &str = "thehackerapp/event";

register_plugin!("thehackerapp", "events", Events);

#[derive(Debug)]
struct Events {
    client: Client,
    url: Arc<Url>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct Config {
    /// The URL to fetch event context from
    url: Url,
}

#[async_trait::async_trait]
impl Plugin for Events {
    type Config = Config;

    async fn new(init: PluginInit<Self::Config>) -> Result<Self, BoxError> {
        let client = Client::builder()
            .user_agent("apollo-")
            .timeout(Duration::from_secs(1))
            .connect_timeout(Duration::from_secs(1))
            .build()?;

        Ok(Events {
            client,
            url: Arc::new(init.config.url),
        })
    }

    // Retrieves the details about the current event
    fn supergraph_service(&self, service: supergraph::BoxService) -> supergraph::BoxService {
        let client = self.client.clone();
        let url = self.url.clone();

        ServiceBuilder::new()
            .oneshot_checkpoint_async(move |req: supergraph::Request| {
                let client = client.clone();
                let url = url.clone();

                async move {
                    let params = if let Some(slug) =
                        req.supergraph_request.headers().typed_get::<EventSlug>()
                    {
                        Params::Slug(Cow::Owned(slug.into_inner()))
                    } else if let Some(host) = req.supergraph_request.headers().typed_get::<Host>()
                    {
                        Params::Domain(Cow::Owned(host.hostname().to_string()))
                    } else {
                        return Ok(ControlFlow::Break(error_response!(
                            "no Host header",
                            "UNKNOWN_EVENT",
                            StatusCode::NOT_FOUND,
                            req: req,
                            service: supergraph
                        )));
                    };

                    let context = client
                        .get(url.as_str())
                        .query(&params)
                        .send()
                        .await?
                        .error_for_status()?
                        .json::<Option<Context>>()
                        .await?;

                    match context {
                        Some(context) => {
                            req.context.insert(EVENT_CONTEXT_KEY, context)?;
                            Ok(ControlFlow::Continue(req))
                        }
                        None => Ok(ControlFlow::Break(error_response!(
                            "unknown event", "UNKNOWN_EVENT",
                            StatusCode::NOT_FOUND,
                            req: req, service: supergraph
                        ))),
                    }
                }
            })
            .service(service)
            .boxed()
    }

    // Injects the event's details into the subgraph request
    fn subgraph_service(&self, _name: &str, service: subgraph::BoxService) -> subgraph::BoxService {
        ServiceBuilder::new()
            .service(service)
            .map_request(|mut req: subgraph::Request| {
                let context = req
                    .context
                    .get::<_, Context>(EVENT_CONTEXT_KEY)
                    .expect("event context must deserialize")
                    .expect("event context must exist");

                let headers = req.subgraph_request.headers_mut();
                context.write_headers(headers);

                req
            })
            .boxed()
    }
}
