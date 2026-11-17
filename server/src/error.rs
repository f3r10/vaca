use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

#[derive(Debug)]
pub enum ApiError {
    BadRequest(String),
    NotFound,
    Database,
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    error: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    route: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    owner: Option<&'a str>,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, body) = match &self {
            ApiError::BadRequest(message) => (
                StatusCode::BAD_REQUEST,
                ErrorBody {
                    error: "bad_request",
                    message: Some(message.clone()),
                    route: None,
                    owner: None,
                },
            ),
            ApiError::NotFound => (
                StatusCode::NOT_FOUND,
                ErrorBody {
                    error: "not_found",
                    message: Some("No pot with that id.".to_string()),
                    route: None,
                    owner: None,
                },
            ),
            ApiError::Database => (
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorBody {
                    error: "database",
                    message: Some("The pot store failed.".to_string()),
                    route: None,
                    owner: None,
                },
            ),
        };
        (status, Json(body)).into_response()
    }
}
