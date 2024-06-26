use crate::{
    http::{Client, RequestBuilderExt, Response},
    responses::Responder,
};
use apollo_router::{
    layers::ServiceBuilderExt,
    plugin::{Plugin, PluginInit},
    register_plugin,
    services::{router, subgraph},
};
use context::{
    headers::{EventDomain, EventSlug},
    Scope, User,
};
use headers::{
    authorization::{Authorization, Bearer},
    HeaderMapExt,
};
use http::Method;
use hyper::body::Buf;
use schemars::JsonSchema;
use serde::Deserialize;
use std::{ops::ControlFlow, sync::Arc};
use tower::{BoxError, Service, ServiceBuilder, ServiceExt};
use url::Url;

pub(crate) const AUTHENTICATION_SCOPE_CONTEXT_KEY: &str = "thehackerapp::authentication::scope";
pub(crate) const AUTHENTICATION_USER_CONTEXT_KEY: &str = "thehackerapp::authentication::user";

register_plugin!("thehackerapp", "authentication", Authentication);

#[derive(Clone)]
struct Authentication {
    client: Client,
    upstream: Arc<Url>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct Config {
    /// The upstream server for validating authentication tokens
    upstream: Url,
}

#[async_trait::async_trait]
impl Plugin for Authentication {
    type Config = Config;

    async fn new(init: PluginInit<Self::Config>) -> Result<Self, BoxError> {
        Ok(Authentication {
            client: Client::new()?,
            upstream: Arc::new(init.config.upstream),
        })
    }

    fn router_service(&self, service: router::BoxService) -> router::BoxService {
        let client = self.client.clone();
        let upstream = self.upstream.clone();

        let handler = move |req: router::Request| {
            let mut client = client.clone();
            let upstream = upstream.clone();

            async move {
                match fetch_context(req, &upstream, &mut client).await? {
                    Ok((req, scope, user)) => {
                        req.context
                            .insert(AUTHENTICATION_SCOPE_CONTEXT_KEY, scope)?;
                        req.context.insert(AUTHENTICATION_USER_CONTEXT_KEY, user)?;
                        Ok(ControlFlow::Continue(req))
                    }
                    Err(e) => Ok(ControlFlow::Break(e)),
                }
            }
        };

        ServiceBuilder::new()
            .oneshot_checkpoint_async(handler)
            .service(service)
            .boxed()
    }

    fn subgraph_service(
        &self,
        _subgraph_name: &str,
        service: subgraph::BoxService,
    ) -> subgraph::BoxService {
        ServiceBuilder::new()
            .map_request(|mut req: subgraph::Request| {
                let user = req
                    .context
                    .get::<_, User>(AUTHENTICATION_USER_CONTEXT_KEY)
                    .expect("user context must be deserializable")
                    .expect("user context must be present");
                let scope = req
                    .context
                    .get::<_, Scope>(AUTHENTICATION_SCOPE_CONTEXT_KEY)
                    .expect("scope context must be deserializable")
                    .expect("scope context must be present");

                let headers = req.subgraph_request.headers_mut();
                user.write_headers(headers);
                scope.write_headers(headers);

                req
            })
            .service(service)
            .boxed()
    }
}

/// Retrieve the request context from the identity service
pub(crate) async fn fetch_context(
    mut req: router::Request,
    upstream: &Url,
    client: &mut Client,
) -> Result<Result<(router::Request, Scope, User), router::Response>, BoxError> {
    let mut upstream = Url::clone(upstream);

    {
        let headers = req.router_request.headers();
        let mut pairs = upstream.query_pairs_mut();

        if let Some(auth) = headers.typed_get::<Authorization<Bearer>>() {
            pairs.append_pair("token", auth.token());
        }

        if let Some(slug) = headers.typed_get::<EventSlug>() {
            pairs.append_pair("slug", &slug);
        } else if let Some(domain) = headers.typed_get::<EventDomain>() {
            pairs.append_pair("domain", &domain);
        } else {
            return Ok(Err(req.respond_invalid(
                "could not determine event, pass Event-Slug or Event-Domain headers",
            )?));
        }
    }

    let Response { response, context } = client
        .call(
            http::Request::builder()
                .uri(upstream.as_str())
                .method(Method::GET)
                .context(req.context)?,
        )
        .await?;
    req.context = context;

    let (parts, body) = response.into_parts();
    if !parts.status.is_success() {
        let bytes = hyper::body::aggregate(body).await?;
        let response = serde_json::from_reader::<_, ApiError>(bytes.reader())?;

        return Ok(Err(req.respond(response.message, parts.status)?));
    }

    let scope = match Scope::try_from(&parts.headers) {
        Ok(s) => s,
        Err(e) => return Ok(Err(req.respond_invalid(e.to_string())?)),
    };
    let user = match User::try_from(&parts.headers) {
        Ok(u) => u,
        Err(e) => return Ok(Err(req.respond_invalid(e.to_string())?)),
    };

    Ok(Ok((req, scope, user)))
}

#[derive(Debug, Deserialize)]
struct ApiError {
    message: String,
}
