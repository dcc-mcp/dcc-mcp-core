#!/usr/bin/env bash
# Fetch data from an API — demonstrates a shell script in ClawHub format.
# Uses curl (declared in metadata.openclaw.requires.bins).

URL="${1:-https://httpbin.org/get}"

result=$(curl -s -w '\n%{http_code}' "$URL")
http_code=$(echo "$result" | tail -1)
body=$(echo "$result" | sed '$d')

if [ "$http_code" -ge 200 ] && [ "$http_code" -lt 300 ]; then
    echo "{\"success\": true, \"message\": \"Fetched $URL (HTTP $http_code)\", \"context\": {\"status\": $http_code}}"
else
    echo "{\"success\": false, \"message\": \"Request failed: HTTP $http_code\"}"
fi
