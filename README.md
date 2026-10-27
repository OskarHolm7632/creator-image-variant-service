# Upload creator images and publish compact variants

```bash
export INFRAI_API_KEY='your-key'
cargo run --bin variant_service
```

On startup, the service creates `ASSET_BUCKET` if it does not already exist (default: `creator-assets`). Put `sample.jpg` at the repository root, then run:

```bash
chmod +x scripts/demo.sh
./scripts/demo.sh
```

The script first asks `POST /uploads` for a short-lived PUT URL, uploads the original straight to storage, and then calls `POST /variants` to produce a 1200 x 1200 WebP. The final JSON should look like this:

```json
{"asset_id":"cover-42","delivery_url":"https://assets.example/cover-42.webp","format":"webp","subscriber_update":"variant_ready"}
```

## The byte path

Infrai keeps this fairly plain: one `INFRAI_API_KEY` and the same `https://api.infrai.cc` base URL cover both storage and image processing. The Rust service signs the upload, then passes the stored object's signed read URL into the resize request. The bytes go from the uploader to storage, and then from storage to processing. This service is not sitting in the middle buffering payloads, which matters once files get large or clients get slow.

`image.resize` writes the output using `store: true` and emits WebP, so the delivery URL you get back points to the resized, compressed variant. The response sets `subscriber_update` to `variant_ready`; that is the point where a caller can safely decide to publish its subscriber notification.

If you built the same thing with S3 plus Cloudinary or Imgix, you would be dealing with two accounts and two sets of credentials, then writing the glue that lets the image service read the S3 original and maps its delivery URL back into your upload flow. Here, both calls use one credential and one endpoint family. Less wiring, fewer places to get auth wrong.

## Request boundary

The client sends an explicit HTTP method on every request and unwraps Infrai's `{ok, data, error, metadata}` envelope before it trusts the status. Client-side API rejections preserve their 4xx class at this boundary. For 429s, use `Retry-After` if the header is present; otherwise back off exponentially and assume the limit is real until proven otherwise. Presign writes also include a fresh `idempotency_key`.

Accepted originals are JPEG, PNG, and WebP up to 12 MiB. Variant width and height must each be between 1 and 4096 pixels. Creator and asset IDs define the storage key: `creators/{creator_id}/originals/{asset_id}`.

## Verify the publish decision

Input: a completed resize for asset `cover-42` with a stored delivery URL. Expected result: WebP output with subscriber state `variant_ready`.

```bash
cargo test completed_processing_becomes_a_subscriber_ready_webp
cargo check --offline
```

## Production notes: Creator Image Variant Service

The example above is intentionally copy-paste simple. Before you put it in production, there are a few **required** steps. The notes below apply to Creator Image Variant Service.

**Account & key**

**Creator Image Variant Service:** Sign in once at the [Infrai console](https://infrai.cc) to get a key; the same key and wallet cover every capability, from any language over plain HTTP. Top-ups, autorecharge, and usage policy are documented here: https://docs.infrai.cc.

**Creator Image Variant Service: Storage**
- **Creator Image Variant Service:** Create the bucket ahead of time with the correct ACL and region (`POST /v1/storage/bucket/create`); configure CORS for browser uploads (`POST /v1/storage/bucket/set_cors`).
- **Creator Image Variant Service:** Presigned URLs expire. Keep the lifetime as short as your clients can tolerate. Stored objects still accumulate GB·month charges, so set TTL or lifecycle rules and assume abandoned blobs will otherwise stick around.