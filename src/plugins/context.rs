use super::error::PluginDisabledError;
use crate::responses;
use apollo_router::{
    layers::ServiceBuilderExt,
    plugin::{Plugin, PluginInit},
    register_plugin,
    services::{subgraph, supergraph},
};
use context::{scope, user};
use schemars::JsonSchema;
use serde::Deserialize;
use std::ops::ControlFlow;
use tower::{BoxError, ServiceBuilder, ServiceExt};

register_plugin!("thehackerapp", "context", Context);

const USER_CONTEXT_KEY: &str = "thehackerapp/context/user";
const SCOPE_CONTEXT_KEY: &str = "thehackerapp/context/scope";

#[derive(Clone, Copy, Debug)]
struct Context;

#[derive(Debug, Default, Deserialize, JsonSchema)]
struct Config {
    /// Whether the plugin is enabled
    enabled: bool,
}

#[async_trait::async_trait]
impl Plugin for Context {
    type Config = Config;

    async fn new(init: PluginInit<Self::Config>) -> Result<Self, BoxError> {
        if !init.config.enabled {
            return Err(Box::new(PluginDisabledError));
        }

        Ok(Context)
    }

    fn supergraph_service(&self, service: supergraph::BoxService) -> supergraph::BoxService {
        ServiceBuilder::new()
            .checkpoint(|req: supergraph::Request| {
                let headers = req.supergraph_request.headers();
                let user = match user::Context::try_from(headers) {
                    Ok(user) => user,
                    Err(e) => {
                        return Ok(ControlFlow::Break(responses::invalid(req, e.to_string())?));
                    }
                };
                let scope = match scope::Context::try_from(headers) {
                    Ok(scope) => scope,
                    Err(e) => {
                        return Ok(ControlFlow::Break(responses::invalid(req, e.to_string())?));
                    }
                };

                req.context.insert(USER_CONTEXT_KEY, user)?;
                req.context.insert(SCOPE_CONTEXT_KEY, scope)?;

                Ok(ControlFlow::Continue(req))
            })
            .service(service)
            .boxed()
    }

    fn subgraph_service(&self, _name: &str, service: subgraph::BoxService) -> subgraph::BoxService {
        ServiceBuilder::new()
            .checkpoint(|mut req: subgraph::Request| {
                let user = req
                    .context
                    .get::<_, user::Context>(USER_CONTEXT_KEY)?
                    .expect("user context must be present");
                let scope = req
                    .context
                    .get::<_, scope::Context>(SCOPE_CONTEXT_KEY)?
                    .expect("scope context must be present");

                let headers = req.subgraph_request.headers_mut();
                user.write_headers(headers);
                scope.write_headers(headers);

                Ok(ControlFlow::Continue(req))
            })
            .service(service)
            .boxed()
    }
}
