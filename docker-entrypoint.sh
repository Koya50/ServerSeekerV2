#!/bin/sh
# Generates config.toml and masscan.conf from environment variables, then starts the app.
set -eu

: "${PGHOST:?PGHOST is required}"
: "${PGUSER:?PGUSER is required}"
: "${PGPASSWORD:?PGPASSWORD is required}"
: "${PGDATABASE:?PGDATABASE is required}"
: "${MASSCAN_RANGE:?MASSCAN_RANGE is required (e.g. 192.0.2.0/24)}"

PORT_START="${SCAN_PORT_START:-25565}"
PORT_END="${SCAN_PORT_END:-25565}"

cat > /app/config.toml <<CFG
[scanner]
repeat = ${SCAN_REPEAT:-true}
scan_delay = ${SCAN_DELAY:-60}
port_range_start = ${PORT_START}
port_range_end = ${PORT_END}
ignore_fake_players = ${IGNORE_FAKE_PLAYERS:-true}
rescan_priority = "${RESCAN_PRIORITY:-OldestFirst}"

[masscan]
config_file = "/app/masscan.conf"

[database]
host = "${PGHOST}"
port = ${PGPORT:-5432}
table = "${PGDATABASE}"
user = "${PGUSER}"
password = "${PGPASSWORD}"

[player_tracking]
enabled = false
webhook = ""
players = [""]

[country_tracking]
enabled = ${COUNTRY_TRACKING:-false}
update_frequency = ${COUNTRY_UPDATE_HOURS:-48}
ipinfo_token = "${IPINFO_TOKEN:-}"
CFG

cat > /app/masscan.conf <<CFG
rate = ${MASSCAN_RATE:-1000}
ports = ${PORT_START}-${PORT_END}
range = ${MASSCAN_RANGE}
wait = 5
CFG

exec ServerSeekerV2 --config-file /app/config.toml --mode "${MODE:-scanner}"
