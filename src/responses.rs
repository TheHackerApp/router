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

            fn respond<S>(self, message: S, code: ::http::StatusCode) -> Result<Self::Response, BoxError>
            where
                S: Into<String>,
            {
                let builder = ::apollo_router::services::$module::Response::builder()
                    .error(build_error(message, code))
                    .status_code(code);
                impl_responder!(@internal builder, self.context; $($rest)*)
            }
        }
    };
    (@internal $builder:expr, $context:expr; headers $($rest:tt)*) => {{
        let with_header = $builder.header(::http::header::CONTENT_TYPE, "application/json");
        impl_responder!(@internal with_header, $context; $($rest)*)
    }};
    (@internal $builder:expr, $context:expr; infallible) => {{
        let value = $builder.context($context).build();
        Ok(value)
    }};
    (@internal $builder:expr, $context:expr;) => {
        $builder.context($context).build()
    };
}

impl_responder!(router headers);
impl_responder!(supergraph headers);
impl_responder!(subgraph infallible);

fn build_error<S>(message: S, code: StatusCode) -> graphql::Error
where
    S: Into<String>,
{
    graphql::Error::builder()
        .message(message)
        .extension_code(match code {
            StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY => "BAD_REQUEST",
            StatusCode::INTERNAL_SERVER_ERROR => "INTERNAL_ERROR",
            _ => "UNKNOWN",
        })
        .build()
}
