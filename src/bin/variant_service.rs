use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use creator_image_variant_service::{
    creator_pipeline::{
        object_key, publish_variant, PublishRequest, UploadRequest, UploadTicket, MAX_UPLOAD_BYTES,
    },
    infrai::{InfraiClient, InfraiError},
};
use serde::Serialize;
use std::{
    env,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use thiserror::Error;

static REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
struct AppState {
    infrai: InfraiClient,
    bucket: Arc<str>,
}

#[derive(Debug, Error)]
enum ServiceError {
    #[error("unsupported content type")]
    UnsupportedContentType,
    #[error("invalid variant dimensions")]
    InvalidDimensions,
    #[error(transparent)]
    Infrai(#[from] InfraiError),
}

#[derive(Serialize)]
struct ErrorReply {
    error: String,
}

impl IntoResponse for ServiceError {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::UnsupportedContentType | Self::InvalidDimensions => StatusCode::BAD_REQUEST,
            Self::Infrai(InfraiError::Api { status, .. }) if *status >= 400 && *status < 500 => {
                StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_REQUEST)
            }
            Self::Infrai(_) => StatusCode::BAD_GATEWAY,
        };
        (
            status,
            Json(ErrorReply {
                error: self.to_string(),
            }),
        )
            .into_response()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let infrai = InfraiClient::from_env()?;
    let bucket = env::var("ASSET_BUCKET").unwrap_or_else(|_| "creator-assets".to_owned());
    infrai.ensure_bucket(&bucket).await?;
    let state = AppState {
        infrai,
        bucket: bucket.into(),
    };

    let app = Router::new()
        .route("/uploads", post(create_upload))
        .route("/variants", post(create_variant))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    println!("variant service listening on http://127.0.0.1:3000");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn create_upload(
    State(state): State<AppState>,
    Json(input): Json<UploadRequest>,
) -> Result<Json<UploadTicket>, ServiceError> {
    if !matches!(
        input.content_type.as_str(),
        "image/jpeg" | "image/png" | "image/webp"
    ) {
        return Err(ServiceError::UnsupportedContentType);
    }
    let key = object_key(&input.creator_id, &input.asset_id);
    let request_id = request_id();
    let signed = state
        .infrai
        .presign_upload(
            &state.bucket,
            &key,
            &input.content_type,
            MAX_UPLOAD_BYTES,
            &request_id,
        )
        .await?;
    Ok(Json(UploadTicket {
        method: "PUT",
        upload_url: signed.url,
        object_key: key,
        max_bytes: MAX_UPLOAD_BYTES,
    }))
}

async fn create_variant(
    State(state): State<AppState>,
    Json(input): Json<PublishRequest>,
) -> Result<Json<creator_image_variant_service::creator_pipeline::PublishedVariant>, ServiceError> {
    if input.width == 0 || input.height == 0 || input.width > 4096 || input.height > 4096 {
        return Err(ServiceError::InvalidDimensions);
    }
    let key = object_key(&input.creator_id, &input.asset_id);
    let request_id = request_id();
    let resized = state
        .infrai
        .resize_from_object(&state.bucket, &key, input.width, input.height, &request_id)
        .await?;
    Ok(Json(publish_variant(input.asset_id, resized.url)))
}

fn request_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock must follow the Unix epoch")
        .as_nanos();
    let sequence = REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("variant-service-{nanos}-{sequence}")
}
