#!/usr/bin/env bash
# Upload files as assets of the release tagged $RELEASE_TAG, replacing any
# existing asset with the same name. Plain uploads API + GITHUB_TOKEN —
# deliberately avoids release-management actions, whose release PATCH calls
# are rejected for bot-created releases.
set -euo pipefail

api="https://api.github.com/repos/${GITHUB_REPOSITORY}"
auth=(-H "Authorization: Bearer ${GITHUB_TOKEN}")

release=$(curl -sf "${auth[@]}" "$api/releases/tags/${RELEASE_TAG}")
release_id=$(jq -r .id <<<"$release")

for file in "$@"; do
    name=$(basename "$file")
    existing=$(jq -r ".assets[] | select(.name == \"$name\") | .id" <<<"$release")
    if [ -n "$existing" ]; then
        echo "replacing existing asset $name (id $existing)"
        curl -sf -X DELETE "${auth[@]}" "$api/releases/assets/$existing"
    fi
    echo "uploading $name"
    curl -sf -X POST "${auth[@]}" \
        -H "Content-Type: application/octet-stream" \
        --data-binary @"$file" \
        "https://uploads.github.com/repos/${GITHUB_REPOSITORY}/releases/${release_id}/assets?name=${name}" \
        >/dev/null
done
