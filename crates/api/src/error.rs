use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{FromRequest, Request};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use serde::de::DeserializeOwned;
use utoipa::ToSchema;

/// Every error the API returns:
/// `{ "error": { "code": "snake_case_code", "message": "Human sentence." } }` with a fitting status.
/// Validation errors add `"fields": { name: message }`.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
    fields: BTreeMap<String, String>,
}

/// The body of every error response.
#[derive(Serialize, ToSchema)]
pub(crate) struct ErrorEnvelope<'a> {
    error: ErrorBody<'a>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct ErrorBody<'a> {
    /// What went wrong, for code to branch on: `snake_case`, stable.
    code: &'a str,
    /// What went wrong, for a person to read.
    message: &'a str,
    /// Validation errors only: each field that's wrong and why.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    #[schema(required = false)]
    fields: &'a BTreeMap<String, String>,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        ApiError {
            status,
            code,
            message: message.into(),
            fields: BTreeMap::new(),
        }
    }

    /// A 422 naming each field that's wrong and why.
    pub fn validation(message: impl Into<String>, fields: BTreeMap<String, String>) -> Self {
        ApiError {
            fields,
            ..ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation_failed",
                message,
            )
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let envelope = ErrorEnvelope {
            error: ErrorBody {
                code: self.code,
                message: &self.message,
                fields: &self.fields,
            },
        };
        (self.status, Json(envelope)).into_response()
    }
}

/// Something on the server failed: logged in full, answered with a 500 that says no more.
impl ApiError {
    pub fn internal(error: &dyn std::fmt::Display) -> Self {
        tracing::error!(%error, "request failed");
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "Something went wrong on the server.",
        )
    }
}

impl From<db::Error> for ApiError {
    fn from(error: db::Error) -> Self {
        ApiError::internal(&error)
    }
}

pub async fn not_found() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "not_found",
        "There's nothing at this address.",
    )
}

pub async fn method_not_allowed() -> ApiError {
    ApiError::new(
        StatusCode::METHOD_NOT_ALLOWED,
        "method_not_allowed",
        "This address doesn't accept that method.",
    )
}

/// A JSON request body; one that doesn't parse gets the error envelope, not axum's plain text.
pub struct Body<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned> FromRequest<S> for Body<T> {
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(request, state).await {
            Ok(Json(value)) => Ok(Body(value)),
            Err(_) => Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_body",
                "The request body isn't the JSON this address expects.",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::body::to_bytes;
    use serde_json::{Value, json};

    use super::*;

    async fn render(error: ApiError) -> (StatusCode, Value) {
        let response = error.into_response();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn internal_errors_say_no_more_than_that() {
        let error = db::Error::Unreadable {
            what: "a currency",
            value: "XXX".to_owned(),
        };
        let (status, body) = render(error.into()).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            body,
            json!({ "error": {
                "code": "internal_error",
                "message": "Something went wrong on the server.",
            } })
        );
    }

    #[tokio::test]
    async fn validation_errors_name_their_fields() {
        let fields = BTreeMap::from([("name".to_owned(), "Give it a name.".to_owned())]);
        let (status, body) = render(ApiError::validation("Check the form.", fields)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            body,
            json!({ "error": {
                "code": "validation_failed",
                "message": "Check the form.",
                "fields": { "name": "Give it a name." },
            } })
        );
    }
}
