use serde::{Deserialize, Serialize};

pub const MAX_UPLOAD_BYTES: u64 = 12 * 1024 * 1024;

#[derive(Debug, Deserialize)]
pub struct UploadRequest {
    pub creator_id: String,
    pub asset_id: String,
    pub content_type: String,
}

#[derive(Debug, Serialize)]
pub struct UploadTicket {
    pub method: &'static str,
    pub upload_url: String,
    pub object_key: String,
    pub max_bytes: u64,
}

#[derive(Debug, Deserialize)]
pub struct PublishRequest {
    pub creator_id: String,
    pub asset_id: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriberUpdate {
    VariantReady,
}

#[derive(Debug, Serialize)]
pub struct PublishedVariant {
    pub asset_id: String,
    pub delivery_url: String,
    pub format: &'static str,
    pub subscriber_update: SubscriberUpdate,
}

pub fn object_key(creator_id: &str, asset_id: &str) -> String {
    format!("creators/{creator_id}/originals/{asset_id}")
}

pub fn publish_variant(asset_id: String, delivery_url: String) -> PublishedVariant {
    PublishedVariant {
        asset_id,
        delivery_url,
        format: "webp",
        subscriber_update: SubscriberUpdate::VariantReady,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_processing_becomes_a_subscriber_ready_webp() {
        let result = publish_variant(
            "cover-42".to_owned(),
            "https://cdn.example/cover-42.webp".to_owned(),
        );

        assert_eq!(result.asset_id, "cover-42");
        assert_eq!(result.format, "webp");
        assert_eq!(result.subscriber_update, SubscriberUpdate::VariantReady);
    }
}
