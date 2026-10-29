#!/usr/bin/env bash
# Free-path MQTT demo publisher for TopicBench color-rule / payload exercises.
# Does not use Pro generators — mosquitto_pub only. Defaults: LocalHost profile creds.
set -euo pipefail
HOST="${MQTT_HOST:-127.0.0.1}"
PORT="${MQTT_PORT:-1883}"
USER="${MQTT_USER:-mqtt}"
PASS="${MQTT_PASS:-mqtt}"
INTERVAL="${INTERVAL_S:-1.5}"

pub() {
  local topic="$1" payload="$2"
  mosquitto_pub -h "$HOST" -p "$PORT" -u "$USER" -P "$PASS" -t "$topic" -m "$payload" -r
  echo "→ $topic  $payload"
}

echo "Publishing demo payloads to $HOST:$PORT (Ctrl+C to stop)"
n=0
while true; do
  n=$((n + 1))
  # Nested dict fields for color rules (meta.channel / channels[i].name)
  ch=$(( n % 2 ))
  if [[ $ch -eq 0 ]]; then meta=Line-A; else meta=Line-B; fi
  south=$(( n % 3 ))
  if [[ $south -eq 0 ]]; then c1=CH-South; else c1=CH-East; fi
  cpu=$(awk -v n="$n" 'BEGIN{printf "%.1f", 50+40*sin(n/5)}')
  pub "topicbench/sim/color/nested" \
    "{\"meta\":{\"channel\":\"$meta\"},\"channels\":[{\"name\":\"CH-North\"},{\"name\":\"$c1\"}],\"ok\":true,\"cpu\":$cpu}"

  # One sample of each common whole-payload shape
  pub "topicbench/sim/demo/bool_toggle" "$([ $((n%2)) -eq 0 ] && echo true || echo false)"
  pub "topicbench/sim/demo/constant" "42.$((n%10))"
  pub "topicbench/sim/demo/counter" "$n"
  case $((n%3)) in 0) e=a;; 1) e=b;; *) e=c;; esac
  pub "topicbench/sim/demo/enum_cycle" "\"$e\""
  pub "topicbench/sim/demo/iso_timestamp" "\"$(date -u +%Y-%m-%dT%H:%M:%SZ)\""
  pub "topicbench/sim/demo/sine" "$(awk -v n="$n" 'BEGIN{printf "%.3f", 50+50*sin(n/3)}')"
  pub "topicbench/lab/sensor" \
    "{\"meta\":{\"channel\":\"Line-A\"},\"sensors\":{\"temp\":$((20+n%6)).$((n%10)),\"humidity\":$((35+n%20))},\"channels\":[{\"name\":\"CH-North\"},{\"name\":\"CH-South\"}]}"
  pub "lab/temperature" "$(awk -v n="$n" 'BEGIN{printf "%.1f", 28+4*sin(n/4)}')"
  sleep "$INTERVAL"
done
