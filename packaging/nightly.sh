#!/usr/bin/env bash
# Decide before building or assigning a nightly version. Checkout must include all tags.
set -euo pipefail

if [ "$GITHUB_EVENT_NAME" != schedule ]; then
  echo 'build=true' >>"$GITHUB_OUTPUT"
  exit 0
fi

# /releases/latest excludes prereleases. Include every published release instead,
# including nightlies, and sort by publication time across all API pages.
latest_tag="$(gh api --paginate --slurp "repos/$GITHUB_REPOSITORY/releases?per_page=100" \
  --jq '[.[][] | select(.draft == false and .published_at != null)] | sort_by(.published_at) | last | .tag_name // empty')"
if [ -n "$latest_tag" ]; then
  # Resolve the tag itself, not target_commitish (which can be a moving branch).
  released_sha="$(git rev-parse --verify "refs/tags/$latest_tag^{commit}")"
  if [ "$released_sha" = "$GITHUB_SHA" ]; then
    echo 'build=false' >>"$GITHUB_OUTPUT"
    echo "### Nightly skipped: no new commits since $latest_tag" >>"$GITHUB_STEP_SUMMARY"
    exit 0
  fi
fi

echo 'build=true' >>"$GITHUB_OUTPUT"
