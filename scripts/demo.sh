#!/usr/bin/env bash
set -euo pipefail

api="${SERVICE_URL:-http://127.0.0.1:3000}"
ticket="$(curl --fail --silent --show-error --request POST "$api/uploads" \
  --header 'content-type: application/json' \
  --data '{"creator_id":"maker-7","asset_id":"cover-42","content_type":"image/jpeg"}')"
upload_url="$(printf '%s' "$ticket" | sed -n 's/.*"upload_url":"\([^"]*\)".*/\1/p')"

curl --fail --silent --show-error --request PUT "$upload_url" \
  --header 'content-type: image/jpeg' \
  --data-binary '@sample.jpg'
curl --fail --silent --show-error --request POST "$api/variants" \
  --header 'content-type: application/json' \
  --data '{"creator_id":"maker-7","asset_id":"cover-42","width":1200,"height":1200}'
printf '\n'

