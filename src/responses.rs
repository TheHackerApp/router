use apollo_router::graphql;
use http::StatusCode;
use tower::BoxError;

pub trait Responder {
    type Response;

    /// Respond with an error
    fn respond<S>(self, message: S, code: StatusCode) -> Result<Self::Response, BoxError>
    where
        S: Into<String>;

    /// Create a response for an invalid request
    fn respond_invalid<S>(self, message: S) -> Result<Self::Response, BoxError>
    where
        S: Into<String>,
        Self: Sized,
    {
        self.respond(message, StatusCode::BAD_REQUEST)
    }
}

macro_rules! impl_responder {
    ($module:ident $($rest:tt)*) => {
        impl Responder for ::apollo_router::services::$module::Request {
            type Response = ::apollo_router::services::$module::Response;

            impl_responder!(@internal $module $($rest)*);
        }
    };
    (@internal $module:ident) => {
        fn respond<S>(self, message: S, code: StatusCode) -> Result<Self::Response, BoxError>
        where
            S: Into<String>,
        {
            ::apollo_router::services::$module::Response::builder()
                .error(build_error(message, code))
                .status_code(code)
                .context(self.context)
                .build()
        }
    };
    (@internal $module:ident infallible) => {
        fn respond<S>(self, message: S, code: StatusCode) -> Result<Self::Response, BoxError>
        where
            S: Into<String>,
        {
            Ok(::apollo_router::services::$module::Response::builder()
                .error(build_error(message, code))
                .status_code(code)
                .context(self.context)
                .build())
        }
    };
}

impl_responder!(router);
impl_responder!(supergraph);
impl_responder!(subgraph infallible);

fn build_error<S>(message: S, code: StatusCode) -> graphql::Error
where
    S: Into<String>,
{
    graphql::Error::builder()
        .message(message)
        .extension_code(match code {
            StatusCode::BAD_REQUEST => "BAD_REQUEST",
            StatusCode::INTERNAL_SERVER_ERROR => "INTERNAL_ERROR",
            _ => "UNKNOWN",
        })
        .build()
}
