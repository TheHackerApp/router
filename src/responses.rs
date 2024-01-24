use apollo_router::{graphql, services::supergraph};
use http::StatusCode;
use tower::BoxError;
#[macro_export]
macro_rules! error_response {
    ($message:expr, req: $req:expr, service: $service:ident) => {
        error_response!($message, "INTERNAL_ERROR", req: $req, service: $service)
    };
    ($message:expr, $code:expr, req: $req:expr, service: $service:ident) => {
        error_response!($message, $code, http::StatusCode::INTERNAL_SERVER_ERROR, req: $req, service: $service)
    };
    ($message:expr, $code:expr, $status:expr, req: $req:expr, service: $service:ident) => {{
        apollo_router::services::$service::Response::error_builder()
            .error(
                apollo_router::graphql::Error::builder()
                    .message($message)
                    .extension_code($code)
                    .build(),
            )
            .status_code($status)
            .context($req.context)
            .build()?
    }};
}

/// Create a response for an invalid request
pub fn invalid<S: Into<String>>(
    req: supergraph::Request,
    message: S,
) -> Result<supergraph::Response, BoxError> {
    error(req, message, StatusCode::BAD_REQUEST)
}

/// Create a new error for the request
pub fn error<S: Into<String>>(
    req: supergraph::Request,
    message: S,
    code: StatusCode,
) -> Result<supergraph::Response, BoxError> {
    let error = graphql::Error::builder()
        .message(message)
        .extension_code(status_code_to_extension_code(code))
        .build();

    supergraph::Response::builder()
        .error(error)
        .status_code(code)
        .context(req.context)
        .build()
}

fn status_code_to_extension_code(status: StatusCode) -> &'static str {
    match status {
        StatusCode::BAD_REQUEST => "BAD_REQUEST",
        StatusCode::INTERNAL_SERVER_ERROR => "INTERNAL_ERROR",
        _ => "UNKNOWN",
    }
}
