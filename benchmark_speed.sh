#!/usr/bin/env bash
# Showcase: Rust JSON:API vs Drupal JSON:API response times
# Randomizes which server is queried first to avoid warm-up bias.

RUST="http://vps-bb463b0a.vps.ovh.net:3000"
DRUPAL="http://vps-bb463b0a.vps.ovh.net"

BOLD='\033[1m'
BLUE='\033[0;34m'
RESET='\033[0m'

measure() {
    local label="$1"
    local url="$2"
    local result
    result=$(curl -sg --max-time 10 -o /dev/null \
        -w "%{time_total}s  |  %{size_download} bytes  |  HTTP %{http_code}" "$url")
    printf "  %-10s %s\n" "$label" "$result"
}

run_test() {
    local title="$1"
    local path="$2"

    echo ""
    printf "${BOLD}${BLUE}▶ %s${RESET}\n" "$title"

    if (( RANDOM % 2 == 0 )); then
        measure "Rust  " "${RUST}${path}"
        measure "Drupal" "${DRUPAL}${path}"
    else
        measure "Drupal" "${DRUPAL}${path}"
        measure "Rust  " "${RUST}${path}"
    fi
}

echo ""
printf "${BOLD}Drupal JSON:API  vs  Rust JSON:API sidecar${RESET}\n"
echo "=========================================="

# 1. Default collection (50 items)
run_test "Collection — first 50 articles" \
    "/jsonapi/node/article"

# >= and < must be percent-encoded so curl's URL parser doesn't choke on them.
# Small random jitter (±300 s) is added to timestamps so each run is cache-bust.
JITTER=$(( (RANDOM % 601) - 300 ))

# 2. Filter: created on Feb 14 (119k matching nodes, returns first 50)
P2="filter%5Bfrom%5D%5Bcondition%5D%5Bpath%5D=created"
P2+="&filter%5Bfrom%5D%5Bcondition%5D%5Boperator%5D=%3E%3D"
P2+="&filter%5Bfrom%5D%5Bcondition%5D%5Bvalue%5D=$(( 1771023600 + JITTER ))"
P2+="&filter%5Bto%5D%5Bcondition%5D%5Bpath%5D=created"
P2+="&filter%5Bto%5D%5Bcondition%5D%5Boperator%5D=%3C"
P2+="&filter%5Bto%5D%5Bcondition%5D%5Bvalue%5D=$(( 1771110000 + JITTER ))"
run_test "Filter — created Feb 14 (119k matching, page 1 of 50)" \
    "/jsonapi/node/article?${P2}"

# 3. Filter: 6-hour window Feb 15 06:00-12:00 UTC (21k matching)
P3="filter%5Bfrom%5D%5Bcondition%5D%5Bpath%5D=created"
P3+="&filter%5Bfrom%5D%5Bcondition%5D%5Boperator%5D=%3E%3D"
P3+="&filter%5Bfrom%5D%5Bcondition%5D%5Bvalue%5D=$(( 1771131600 + JITTER ))"
P3+="&filter%5Bto%5D%5Bcondition%5D%5Bpath%5D=created"
P3+="&filter%5Bto%5D%5Bcondition%5D%5Boperator%5D=%3C"
P3+="&filter%5Bto%5D%5Bcondition%5D%5Bvalue%5D=$(( 1771153200 + JITTER ))"
run_test "Filter — Feb 15 06:00–12:00 UTC (21k matching)" \
    "/jsonapi/node/article?${P3}"

# 4. Filter + sort DESC + offset into resultset
P4="${P2}&sort=-created&page%5Boffset%5D=100&page%5Blimit%5D=50"
run_test "Filter + sort DESC + offset=100" \
    "/jsonapi/node/article?${P4}"

# 5. Sparse fieldset — only title + created
P5="${P3}&fields%5Bnode--article%5D=title,created"
run_test "Filter + sparse fields (title, created)" \
    "/jsonapi/node/article?${P5}"

echo ""
