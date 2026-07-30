#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
date_utc="$(date -u +%F)"

require_command() {
  if command -v "$1" >/dev/null 2>&1; then
    return
  fi
  echo "missing required command: $1" >&2
  exit 1
}

require_env() {
  local name="$1"

  if [[ -z "${!name:-}" ]]; then
    echo "missing required environment variable: ${name}" >&2
    exit 1
  fi
}

forward_signal() {
  local signal="$1"

  if [[ -n "${app_pid:-}" ]] && kill -0 "${app_pid}" 2>/dev/null; then
    kill "-${signal}" "${app_pid}" 2>/dev/null || kill "${app_pid}" 2>/dev/null || true
  fi
}

require_command awk
require_command wc
require_command /usr/libexec/PlistBuddy
require_env RADROOTS_APP_LOCAL_LOG_ROOT
require_env RADROOTS_APP_NOSTR_RELAY_URLS

if [[ -n "${RADROOTS_APP_HOST_BUNDLE_PATH:-}" ]]; then
  app_path="${RADROOTS_APP_HOST_BUNDLE_PATH}"
else
  app_path="$("${script_dir}/build_host.sh")"
fi
plist_path="${app_path}/Contents/Info.plist"
executable_name="$(
  /usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "${plist_path}"
)"
executable_path="${app_path}/Contents/MacOS/${executable_name}"
app_log_root="${RADROOTS_APP_LOCAL_LOG_ROOT}/apps/local/app/app-macos-native"
structured_log_file="${app_log_root}/${date_utc}.jsonl"
latest_log_path="${app_log_root}/latest.jsonl"
stdout_file="${app_log_root}/raw/stdout.${date_utc}.log"
stderr_file="${app_log_root}/raw/stderr.${date_utc}.log"
startup_attempts="${RADROOTS_APP_HOST_STARTUP_ATTEMPTS:-300}"

mkdir -p "${app_log_root}/raw"
export RUST_LOG="${RADROOTS_APP_RUST_LOG:-info}"

structured_log_first_fresh_line=1
if [[ -f "${structured_log_file}" ]]; then
  existing_structured_log_lines="$(wc -l < "${structured_log_file}")"
  structured_log_first_fresh_line="$((existing_structured_log_lines + 1))"
fi

trap 'forward_signal TERM' TERM
trap 'forward_signal INT' INT
trap 'forward_signal HUP' HUP

stop_app_with_error() {
  local message="$1"

  if [[ -n "${app_pid:-}" ]] && kill -0 "${app_pid}" 2>/dev/null; then
    kill "${app_pid}" 2>/dev/null || true
    wait "${app_pid}" || true
  fi

  echo "${message}" >&2
  exit 1
}

fresh_log_contains_event() {
  local expected_event="$1"

  [[ -f "${structured_log_file}" ]] || return 1
  awk \
    -v first_line="${structured_log_first_fresh_line}" \
    -v event="\"event\":\"${expected_event}\"" \
    'NR >= first_line && index($0, event) { found = 1 } END { exit found ? 0 : 1 }' \
    "${structured_log_file}"
}

"${executable_path}" "$@" >>"${stdout_file}" 2>>"${stderr_file}" &
app_pid="$!"

launch_confirmed=false
for _ in $(seq 1 "${startup_attempts}"); do
  if fresh_log_contains_event "runtime.launch"; then
    launch_confirmed=true
    break
  fi

  if ! kill -0 "${app_pid}" 2>/dev/null; then
    wait "${app_pid}"
    exit $?
  fi

  sleep 0.1
done

if [[ "${launch_confirmed}" != "true" ]]; then
  stop_app_with_error "app launch did not emit runtime.launch within startup timeout"
fi

fresh_log_contains_event "logging.initialized" || {
  stop_app_with_error "app launch did not emit logging.initialized in ${structured_log_file}"
}

[[ -e "${latest_log_path}" ]] || {
  stop_app_with_error "app launch did not create latest structured log alias: ${latest_log_path}"
}

[[ -f "${stdout_file}" ]] || {
  stop_app_with_error "app launch did not create raw stdout log: ${stdout_file}"
}

[[ -f "${stderr_file}" ]] || {
  stop_app_with_error "app launch did not create raw stderr log: ${stderr_file}"
}

printf 'radroots_studio_app run: ready\n'
printf 'relays=%s\n' "${RADROOTS_APP_NOSTR_RELAY_URLS}"
printf 'structured_log=%s\n' "${structured_log_file}"
printf 'latest_log=%s\n' "${latest_log_path}"
printf 'stdout_log=%s\n' "${stdout_file}"
printf 'stderr_log=%s\n' "${stderr_file}"

wait "${app_pid}"
