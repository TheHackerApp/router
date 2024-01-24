use apollo_router::{graphql, services::supergraph};
use http::StatusCode;
use tower::BoxError;

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
