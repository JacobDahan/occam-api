use axum::{
    body::Body,
    extract::Request,
    http::HeaderValue,
    middleware::Next,
    response::Response,
};
use uuid::Uuid;

/// HTTP header name for request ID
pub const REQUEST_ID_HEADER: &str = "x-request-id";

/// Extension type for storing request ID in request extensions
#[derive(Clone, Debug)]
pub struct RequestId(pub Uuid);

impl RequestId {
    /// Creates a new random request ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Returns the UUID as a string
    pub fn as_str(&self) -> String {
        self.0.to_string()
    }
}

impl Default for RequestId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for RequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Middleware that generates or extracts a request ID and adds it to the request extensions.
/// Also adds the request ID to the response headers.
///
/// If the incoming request has an `x-request-id` header, it will be used.
/// Otherwise, a new UUID v4 will be generated.
pub async fn request_id_middleware(mut request: Request, next: Next) -> Response {
    // Try to extract request ID from header, otherwise generate new one
    let request_id = request
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| Uuid::parse_str(s).ok())
        .map(RequestId)
        .unwrap_or_else(RequestId::new);

    // Store in request extensions for handlers to access
    request.extensions_mut().insert(request_id.clone());

    // Continue processing the request
    let mut response = next.run(request).await;

    // Add request ID to response headers
    if let Ok(header_value) = HeaderValue::from_str(&request_id.as_str()) {
        response
            .headers_mut()
            .insert(REQUEST_ID_HEADER, header_value);
    }

    response
}

/// Helper function to create a tracing span with request ID
pub fn make_span_with_request_id(request: &Request<Body>) -> tracing::Span {
    let request_id = request
        .extensions()
        .get::<RequestId>()
        .map(|id| id.as_str())
        .unwrap_or_else(|| "unknown".to_string());

    tracing::info_span!(
        "http_request",
        method = %request.method(),
        uri = %request.uri(),
        request_id = %request_id,
    )
}
