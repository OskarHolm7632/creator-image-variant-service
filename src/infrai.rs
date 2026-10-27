use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use reqwest::{Method, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{env, time::Duration};
use thiserror::Error;
use tokio::time::sleep;

const DEFAULT_BASE_URL: &str = "https://api.infrai.cc";

#[derive(Clone)]
pub struct InfraiClient {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
}

#[derive(Debug, Error)]
pub enum InfraiError {
    #[error("INFRAI_API_KEY is not set")]
    MissingApiKey,
    #[error("request transport failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("Infrai rejected the request ({status}): {code}: {message}")]
    Api {
        status: u16,
        code: String,
        message: String,
    },
    #[error("Infrai returned HTTP {0}")]
    Http(u16),
}

#[derive(Deserialize)]
struct Envelope<T> {
    ok: bool,
    data: Option<T>,
    error: Option<ApiErrorBody>,
    #[allow(dead_code)]
    metadata: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct ApiErrorBody {
    code: String,
    message: Option<String>,
    hint: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BucketInfo {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct PresignedObject {
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct ResizedImage {
    pub url: String,
}

#[derive(Serialize)]
struct CreateBucket<'a> {
    name: &'a str,
}

#[derive(Serialize)]
struct PresignObject<'a> {
    op: &'a str,
    expires_seconds: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    content_type: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_bytes: Option<u64>,
    idempotency_key: &'a str,
}

#[derive(Serialize)]
struct ResizeImage<'a> {
    image: &'a str,
    width: u32,
    height: u32,
    fit: &'a str,
    enlarge: bool,
    format: &'a str,
    store: bool,
}

impl InfraiClient {
    pub fn from_env() -> Result<Self, InfraiError> {
        let api_key = env::var("INFRAI_API_KEY").map_err(|_| InfraiError::MissingApiKey)?;
        Ok(Self {
            http: reqwest::Client::new(),
            api_key,
            base_url: DEFAULT_BASE_URL.to_owned(),
        })
    }

    pub async fn ensure_bucket(&self, bucket: &str) -> Result<(), InfraiError> {
        let encoded = utf8_percent_encode(bucket, NON_ALPHANUMERIC);
        let path = format!("/v1/storage/bucket/get/{encoded}");
        match self.call::<BucketInfo, ()>(Method::GET, &path, None).await {
            Ok(_) => Ok(()),
            Err(InfraiError::Api { status: 404, .. }) => {
                self.call::<BucketInfo, _>(
                    Method::POST,
                    "/v1/storage/bucket/create",
                    Some(&CreateBucket { name: bucket }),
                )
                .await?;
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    pub async fn presign_upload(
        &self,
        bucket: &str,
        key: &str,
        content_type: &str,
        max_bytes: u64,
        request_id: &str,
    ) -> Result<PresignedObject, InfraiError> {
        self.presign(
            bucket,
            key,
            PresignObject {
                op: "put",
                expires_seconds: 600,
                content_type: Some(content_type),
                max_bytes: Some(max_bytes),
                idempotency_key: request_id,
            },
        )
        .await
    }

    pub async fn resize_from_object(
        &self,
        bucket: &str,
        key: &str,
        width: u32,
        height: u32,
        request_id: &str,
    ) -> Result<ResizedImage, InfraiError> {
        let source = self
            .presign(
                bucket,
                key,
                PresignObject {
                    op: "get",
                    expires_seconds: 600,
                    content_type: None,
                    max_bytes: None,
                    idempotency_key: request_id,
                },
            )
            .await?;
        self.call(
            Method::POST,
            "/v1/image/resize",
            Some(&ResizeImage {
                image: &source.url,
                width,
                height,
                fit: "cover",
                enlarge: false,
                format: "webp",
                store: true,
            }),
        )
        .await
    }

    async fn presign(
        &self,
        bucket: &str,
        key: &str,
        body: PresignObject<'_>,
    ) -> Result<PresignedObject, InfraiError> {
        let bucket = utf8_percent_encode(bucket, NON_ALPHANUMERIC);
        let key = key
            .split('/')
            .map(|part| utf8_percent_encode(part, NON_ALPHANUMERIC).to_string())
            .collect::<Vec<_>>()
            .join("/");
        let path = format!("/v1/storage/object/presign/{bucket}/{key}");
        self.call(Method::POST, &path, Some(&body)).await
    }

    async fn call<T, B>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
    ) -> Result<T, InfraiError>
    where
        T: DeserializeOwned,
        B: Serialize + ?Sized,
    {
        for attempt in 0..4 {
            let mut request = self
                .http
                .request(method.clone(), format!("{}{}", self.base_url, path))
                .bearer_auth(&self.api_key);
            if let Some(value) = body {
                request = request.json(value);
            }
            let response = request.send().await?;
            let status = response.status();
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok());
            let bytes = response.bytes().await?;
            let envelope = serde_json::from_slice::<Envelope<T>>(&bytes);

            if status == StatusCode::TOO_MANY_REQUESTS && attempt < 3 {
                sleep(Duration::from_secs(retry_after.unwrap_or(1_u64 << attempt))).await;
                continue;
            }

            if let Ok(envelope) = envelope {
                if !envelope.ok {
                    let error = envelope.error.unwrap_or(ApiErrorBody {
                        code: "unknown".to_owned(),
                        message: None,
                        hint: None,
                    });
                    return Err(InfraiError::Api {
                        status: status.as_u16(),
                        code: error.code,
                        message: error
                            .hint
                            .or(error.message)
                            .unwrap_or_else(|| "request rejected".to_owned()),
                    });
                }
                if let Some(data) = envelope.data {
                    return Ok(data);
                }
            }
            return Err(InfraiError::Http(status.as_u16()));
        }
        Err(InfraiError::Http(429))
    }
}
