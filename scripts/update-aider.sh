#!/bin/bash

if [ -z "$1" ]; then
    echo "Error: No file path provided."
    echo "Usage: $0 <file-path>"
    exit 1
fi

FILE_PATH="$1"

if [ ! -f "$FILE_PATH" ]; then
    echo "Error: File '$FILE_PATH' does not exist."
    exit 1
fi

echo "Fetching latest version and hash..."

# Fetch the latest version from GitHub (excluding .dev tags)
LATEST_VERSION=$(curl -s https://api.github.com/repos/Aider-AI/aider/tags | jq -r 'first(.[] | select(.name | endswith(".dev") | not) | .name)' | sed 's/^v//')

if [ -z "$LATEST_VERSION" ]; then
    echo "Error: Failed to fetch the latest version."
    exit 1
fi

HASH=$(nurl https://github.com/AIder-AI/aider v$LATEST_VERSION --hash)

if [ -z "$HASH" ]; then
    echo "Error: Failed to fetch the hash."
    exit 1
fi

echo "Updating '$FILE_PATH' with version '$LATEST_VERSION' and hash '$HASH'..."

sed -i "/^[[:space:]]*version = /s|\".*\"|\"${LATEST_VERSION}\"|" "$FILE_PATH"
sed -i "/^[[:space:]]*hash = /s|\".*\"|\"${HASH}\"|" "$FILE_PATH"

echo "Update complete."
