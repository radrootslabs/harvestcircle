#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
platform_root="$(cd "${script_dir}/.." && pwd -P)"
repo_root="$(git -C "${script_dir}" rev-parse --show-toplevel)"
date_utc="$(date -u +%F)"

require_command() {
  if command -v "$1" >/dev/null 2>&1; then
    return
  fi
  echo "missing required command: $1" >&2
  exit 1
}

require_command /usr/libexec/PlistBuddy
require_command mktemp
require_command readlink

app_path="$("${script_dir}/build_host.sh")"
plist_path="${app_path}/Contents/Info.plist"
executable_name="$(
  /usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "${plist_path}"
)"
bundle_name="$(
  /usr/libexec/PlistBuddy -c 'Print :CFBundleName' "${plist_path}"
)"
bundle_id="$(
  /usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "${plist_path}"
)"
icon_name="$(
  /usr/libexec/PlistBuddy -c 'Print :CFBundleIconFile' "${plist_path}"
)"

[[ "${bundle_name}" == "Radroots" ]] || {
  echo "unexpected CFBundleName: ${bundle_name}" >&2
  exit 1
}

[[ "${bundle_id}" == "org.radroots.app.macos" ]] || {
  echo "unexpected CFBundleIdentifier: ${bundle_id}" >&2
  exit 1
}

[[ -x "${app_path}/Contents/MacOS/${executable_name}" ]] || {
  echo "missing bundle executable: ${app_path}/Contents/MacOS/${executable_name}" >&2
  exit 1
}

[[ -f "${app_path}/Contents/Resources/${icon_name}.icns" ]] || {
  echo "missing bundle icon: ${app_path}/Contents/Resources/${icon_name}.icns" >&2
  exit 1
}

release_app_path="$(
  CONFIGURATION=Release "${script_dir}/build_host.sh"
)"
[[ "${release_app_path}" == "${platform_root}/.derived-data/Build/Products/Release/Radroots.app" ]] || {
  echo "unexpected release bundle path: ${release_app_path}" >&2
  exit 1
}
[[ -x "${release_app_path}/Contents/MacOS/Radroots" ]] || {
  echo "missing release bundle executable: ${release_app_path}/Contents/MacOS/Radroots" >&2
  exit 1
}

tmp_root="$(mktemp -d)"
runner_pid=""
degraded_runner_pid=""
startup_wait_attempts="${RADROOTS_APP_HOST_STARTUP_ATTEMPTS:-300}"

wait_for_log_event() {
  local structured_log_file="$1"
  local expected_event="$2"
  local runner_pid="$3"
  local event_verified=false

  for _ in $(seq 1 "${startup_wait_attempts}"); do
    if [[ -f "${structured_log_file}" ]] && grep -q "\"event\":\"${expected_event}\"" "${structured_log_file}" 2>/dev/null; then
      event_verified=true
      break
    fi

    if ! kill -0 "${runner_pid}" 2>/dev/null; then
      wait "${runner_pid}"
      exit $?
    fi

    sleep 0.1
  done

  [[ "${event_verified}" == "true" ]] || {
    echo "${expected_event} was not recorded by run_host.sh" >&2
    exit 1
  }
}

assert_latest_alias() {
  local latest_log_path="$1"

  [[ -e "${latest_log_path}" ]] || {
    echo "missing latest structured log alias: ${latest_log_path}" >&2
    exit 1
  }

  [[ "$(readlink "${latest_log_path}")" == "${date_utc}.jsonl" ]] || {
    echo "latest structured log alias does not point at ${date_utc}.jsonl" >&2
    exit 1
  }
}

assert_raw_logs_exist() {
  local stdout_file="$1"
  local stderr_file="$2"

  [[ -f "${stdout_file}" ]] || {
    echo "missing raw stdout log: ${stdout_file}" >&2
    exit 1
  }

  [[ -f "${stderr_file}" ]] || {
    echo "missing raw stderr log: ${stderr_file}" >&2
    exit 1
  }
}

terminate_runner() {
  local runner_pid="$1"

  if [[ -n "${runner_pid}" ]] && kill -0 "${runner_pid}" 2>/dev/null; then
    kill "${runner_pid}" 2>/dev/null || true
  fi

  set +e
  wait "${runner_pid}"
  local exit_code="$?"
  set -e
  [[ "${exit_code}" == "0" ]] || [[ "${exit_code}" == "143" ]] || [[ "${exit_code}" == "130" ]] || {
    echo "unexpected runner exit code after termination: ${exit_code}" >&2
    exit 1
  }
}

cleanup() {
  if [[ -n "${runner_pid:-}" ]] && kill -0 "${runner_pid}" 2>/dev/null; then
    kill "${runner_pid}" 2>/dev/null || true
    wait "${runner_pid}" || true
  fi
  if [[ -n "${degraded_runner_pid:-}" ]] && kill -0 "${degraded_runner_pid}" 2>/dev/null; then
    kill "${degraded_runner_pid}" 2>/dev/null || true
    wait "${degraded_runner_pid}" || true
  fi
  rm -rf "${tmp_root}"
}
trap cleanup EXIT

runtime_mode="localhost-dev"
nostr_relay_urls="${RADROOTS_APP_NOSTR_RELAY_URLS:-ws://127.0.0.1:8080}"
local_log_root="${tmp_root}/logs"
structured_log_file="${local_log_root}/apps/local/app/app-macos-native/${date_utc}.jsonl"
latest_log_path="${local_log_root}/apps/local/app/app-macos-native/latest.jsonl"
stdout_file="${local_log_root}/apps/local/app/app-macos-native/raw/stdout.${date_utc}.log"
stderr_file="${local_log_root}/apps/local/app/app-macos-native/raw/stderr.${date_utc}.log"

mkdir -p "$(dirname "${structured_log_file}")"
printf '%s\n' \
  '{"event":"logging.initialized","source":"stale-test-fixture"}' \
  '{"event":"runtime.launch","source":"stale-test-fixture"}' \
  >"${structured_log_file}"

RADROOTS_APP_RUNTIME_MODE="${runtime_mode}" \
RADROOTS_APP_NOSTR_RELAY_URLS="${nostr_relay_urls}" \
RADROOTS_APP_LOCAL_LOG_ROOT="${local_log_root}" \
RADROOTS_APP_HOST_BUNDLE_PATH="${app_path}" \
RADROOTS_APP_HOST_STARTUP_ATTEMPTS="${startup_wait_attempts}" \
"${script_dir}/run_host.sh" &
runner_pid="$!"

wait_for_log_event "${structured_log_file}" "runtime.launch" "${runner_pid}"
wait_for_log_event "${structured_log_file}" "logging.initialized" "${runner_pid}"
[[ "$(grep -c '"event":"runtime.launch"' "${structured_log_file}")" -ge 2 ]] || {
  echo "run_host.sh accepted a stale runtime.launch event" >&2
  exit 1
}
assert_latest_alias "${latest_log_path}"
assert_raw_logs_exist "${stdout_file}" "${stderr_file}"
terminate_runner "${runner_pid}"
runner_pid=""

degraded_log_root="${tmp_root}/degraded-logs"
degraded_structured_log_file="${degraded_log_root}/apps/local/app/app-macos-native/${date_utc}.jsonl"
degraded_latest_log_path="${degraded_log_root}/apps/local/app/app-macos-native/latest.jsonl"
degraded_stdout_file="${degraded_log_root}/apps/local/app/app-macos-native/raw/stdout.${date_utc}.log"
degraded_stderr_file="${degraded_log_root}/apps/local/app/app-macos-native/raw/stderr.${date_utc}.log"

env -u HOME \
  RADROOTS_APP_RUNTIME_MODE="${runtime_mode}" \
  RADROOTS_APP_NOSTR_RELAY_URLS="${nostr_relay_urls}" \
  RADROOTS_APP_LOCAL_LOG_ROOT="${degraded_log_root}" \
  RADROOTS_APP_HOST_BUNDLE_PATH="${app_path}" \
  RADROOTS_APP_HOST_STARTUP_ATTEMPTS="${startup_wait_attempts}" \
  "${script_dir}/run_host.sh" &
degraded_runner_pid="$!"

wait_for_log_event "${degraded_structured_log_file}" "runtime.launch" "${degraded_runner_pid}"
wait_for_log_event "${degraded_structured_log_file}" "runtime.degraded" "${degraded_runner_pid}"
wait_for_log_event "${degraded_structured_log_file}" "logging.initialized" "${degraded_runner_pid}"
assert_latest_alias "${degraded_latest_log_path}"
assert_raw_logs_exist "${degraded_stdout_file}" "${degraded_stderr_file}"
grep -q '"startup_issue":"desktop runtime roots require HOME for macos"' "${degraded_structured_log_file}" || {
  echo "runtime.degraded did not record the expected startup issue" >&2
  exit 1
}
terminate_runner "${degraded_runner_pid}"
degraded_runner_pid=""
