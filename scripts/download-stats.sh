#!/usr/bin/env bash
# Show download counts across all GitHub releases, grouped by platform.
# Requires: gh (GitHub CLI), jq
set -euo pipefail

REPO="parallax-labs/context-harness"

# Fetch all releases with asset download counts in one API call
data=$(gh api "repos/${REPO}/releases" --paginate --jq '
  [ .[] | .tag_name as $tag | .assets[] |
    select(.name | test("\\.sha256$") | not) |
    { tag: $tag, name: .name, downloads: .download_count,
      platform: (
        if   (.name | test("linux-x86_64-musl")) then "linux-x86_64-musl"
        elif (.name | test("linux-x86_64"))      then "linux-x86_64"
        elif (.name | test("linux-aarch64"))      then "linux-aarch64"
        elif (.name | test("macos-aarch64"))      then "macos-aarch64"
        elif (.name | test("macos-x86_64"))       then "macos-x86_64"
        elif (.name | test("windows"))            then "windows-x86_64"
        else "other"
        end
      )
    }
  ]
')

printf "\n── Downloads by Release ──────────────────────────────────────\n\n"
printf "%-10s %-22s %s\n" "RELEASE" "PLATFORM" "DOWNLOADS"
printf "%-10s %-22s %s\n" "-------" "--------" "---------"

echo "$data" | jq -r '
  group_by(.tag) | sort_by(.[0].tag) | reverse[] |
  .[0].tag as $tag |
  sort_by(.platform)[] |
  [$tag, .platform, (.downloads | tostring)] | @tsv
' | while IFS=$'\t' read -r tag platform count; do
  printf "%-10s %-22s %s\n" "$tag" "$platform" "$count"
done

printf "\n── Totals by Platform ───────────────────────────────────────\n\n"
printf "%-22s %s\n" "PLATFORM" "TOTAL"
printf "%-22s %s\n" "--------" "-----"

echo "$data" | jq -r '
  group_by(.platform) | map({
    platform: .[0].platform,
    total: (map(.downloads) | add)
  }) | sort_by(.total) | reverse[] |
  [.platform, (.total | tostring)] | @tsv
' | while IFS=$'\t' read -r platform total; do
  printf "%-22s %s\n" "$platform" "$total"
done

grand_total=$(echo "$data" | jq '[.[].downloads] | add')
printf "\n%-22s %s\n\n" "GRAND TOTAL" "$grand_total"
