#[macro_export]
macro_rules! error_response {
    ($message:expr, req: $req:expr, service: $service:ident) => {
        error_response!($message, "INTERNAL_ERROR", req: $req, service: $service);
    };
    ($message:expr, $code:expr, req: $req:expr, service: $service:ident) => {
        error_response!($message, $code, reqwest::StatusCode::INTERNAL_SERVER_ERROR, req: $req, service: $service);
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
