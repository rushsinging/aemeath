#!/usr/bin/env bash
# 守卫：provider 的所有生产 HTTP 请求发送 / 错误响应体读取必须经由
# agent/features/provider/src/adapters/http_attempt.rs 的 HttpAttemptExecutor，
# 且 error_log 模块的 HTTP/network 诊断日志 API（log_network_error /
# log_http_error / ErrorLogContext / LlmApiErrorRecord）只能由 http_attempt.rs
# 调用；其余 driver 只允许使用 error_log::log_stream_protocol_error 这一窄
# protocol 日志 API。
#
# 扫描范围是整个 agent/features/provider/src（不仅 adapters/），精确放行
# adapters/http_attempt.rs（唯一允许发送请求 / 读取失败响应体的 executor）
# 与 adapters/error_log.rs（诊断 API 的原生定义处，仅豁免"自引用"检查项）。
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$SCRIPT_DIR/../.." && pwd)}"
if [ -n "${AEMEATH_PROJECT_DIR:-}" ] && [ ! -d "${AEMEATH_PROJECT_DIR}/.agents/hooks" ]; then
  ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
fi
cd "$ROOT"

PROVIDER_SRC_DIR="agent/features/provider/src"
ADAPTERS_DIR="$PROVIDER_SRC_DIR/adapters"
EXECUTOR_FILE="$ADAPTERS_DIR/http_attempt.rs"
ERROR_LOG_FILE="$ADAPTERS_DIR/error_log.rs"

fail=0

DRIVER_FILES_LIST="$(mktemp)"
DRIVER_FILES_EXCEPT_ERROR_LOG_LIST="$(mktemp)"
trap 'rm -f "$DRIVER_FILES_LIST" "$DRIVER_FILES_EXCEPT_ERROR_LOG_LIST"' EXIT

# Production driver files: the whole provider crate source tree, excluding
# only the executor itself (the sole file allowed to touch raw
# reqwest send/execute/text/json/bytes/chunk) and this crate's test-only
# conventions — `*_tests.rs` / `tests.rs` / `.../tests/*` files. Everything
# else (including error_log.rs, domain/, ports.rs, published_language.rs,
# lib.rs, ...) is scanned as production driver code; error_log.rs only gets
# a narrow additional exemption below for the "no self-reference" check.
find "$PROVIDER_SRC_DIR" -type f -name '*.rs' \
  ! -path "$EXECUTOR_FILE" \
  ! -name '*_tests.rs' \
  ! -name 'tests.rs' \
  ! -path '*/tests/*' \
  | sort >"$DRIVER_FILES_LIST"

grep -vF "$ERROR_LOG_FILE" "$DRIVER_FILES_LIST" >"$DRIVER_FILES_EXCEPT_ERROR_LOG_LIST" || true # guard-registry:scope.provider.error-log-self-reference

# Reliably strip only the trailing inline `#[cfg(test)] mod <name> { ... }`
# test module that this crate conventionally appends at the bottom of a
# file — NEVER just "everything from the first #[cfg(test)] onward".
#
# A `#[cfg(test)]` marker (column 0 — this crate's convention for the
# top-level trailing test module) is only treated as a truncation point when
# the next non-blank, non-attribute line is an inline `mod NAME {` block
# opener. Mid-file `#[cfg(test)] mod x;` declarations (semicolon form — an
# external test submodule reference, e.g. `#[cfg(test)] mod foo_tests;` or
# `#[cfg(test)] #[path = "..."] mod bar;`) are explicitly NOT truncation
# points, so production code that follows them is never swallowed. When
# multiple qualifying markers exist, the LAST one wins (the true trailing
# test module), so a mid-file inline test-helper block followed by more
# production code and then the real trailing `mod tests {` still cuts in
# the right place.
strip_test_tail() {
  awk '
    { lines[NR] = $0 }
    END {
      cut = 0
      for (i = 1; i <= NR; i++) {
        if (lines[i] ~ /^#\[cfg\(test\)\][[:space:]]*$/) {
          j = i + 1
          while (j <= NR && (lines[j] ~ /^[[:space:]]*$/ || lines[j] ~ /^[[:space:]]*#\[[^]]*\][[:space:]]*$/)) {
            j++
          }
          if (j <= NR && lines[j] ~ /^mod[[:space:]]+[A-Za-z_][A-Za-z0-9_]*[[:space:]]*\{[[:space:]]*$/) {
            cut = i
          }
        }
      }
      last = (cut == 0) ? NR : cut - 1
      for (i = 1; i <= last; i++) print lines[i]
    }
  ' "$1"
}

report_pattern() {
  local message="$1"
  local pattern="$2"
  local file_list="$3"
  local hit=0
  local file matches
  while IFS= read -r file; do
    [ -n "$file" ] || continue
    matches="$(strip_test_tail "$file" | grep -nE "$pattern" || true)"
    if [ -n "$matches" ]; then
      hit=1
      while IFS= read -r line; do
        printf '%s:%s\n' "$file" "$line" >&2
      done <<<"$matches"
    fi
  done <"$file_list"
  if [ "$hit" -ne 0 ]; then
    printf '[architecture] %s\n' "$message" >&2
    fail=1
  fi
}

report_pattern \
  "HTTP requests must be sent through HttpAttemptExecutor::execute (adapters/http_attempt.rs); drivers may not call RequestBuilder::send()/Client::execute() directly." \
  '\.send\(\)|\.execute\(' \
  "$DRIVER_FILES_LIST"

# Response body reads must go through HttpAttemptExecutor's BoundedErrorBody
# (failure path) or HttpAttemptExecutor::read_success_json (success path).
# Detects reqwest::Response::text()/json()/bytes()/chunk() — all four take
# no arguments when reading a response body, which also naturally excludes
# `RequestBuilder::json(&body)` (always called with an argument) from ever
# matching. Detection is cross-line: a driver cannot dodge the guard by
# formatting `.json()` and `.await` onto separate lines (idiomatic rustfmt
# chain style). Full-line `//`/`///` comments are dropped before scanning so
# prose that merely *mentions* `response.json().await` (e.g. doc comments
# explaining why HttpAttemptExecutor exists) never false-positives.
# `BoundedErrorBody::text()` is a sync accessor with no `.await` and is
# exempt because the pattern requires `.await` to co-occur.
scan_body_read_bypass() {
  awk '
    {
      raw = $0
      if (raw ~ /^[[:space:]]*\/\//) { raw = "" }
      lines[NR] = raw
    }
    END {
      n = NR
      for (i = 1; i <= n; i++) {
        cur = lines[i]
        if (cur == "") continue
        # POSIX/BWK awk regex has no \b word-boundary support, so "await"
        # as a whole word is matched by requiring the next char to be
        # non-identifier (or end of line) instead. `(::<[^)]*>)?` allows an
        # optional turbofish (`.json::<T>()`) between the method name and
        # its empty argument list, since that is the real reqwest signature
        # a driver could use to dodge detection via an explicit type arg.
        if (cur ~ /\.(text|json|bytes|chunk)(::<[^)]*>)?\(\)[[:space:]]*\.await([^A-Za-z0-9_]|$)/) {
          print i ":" cur
          continue
        }
        if (cur ~ /\.(text|json|bytes|chunk)(::<[^)]*>)?\(\)[[:space:]]*$/) {
          j = i + 1
          while (j <= n && lines[j] ~ /^[[:space:]]*$/) j++
          if (j <= n && lines[j] ~ /^[[:space:]]*\.await([^A-Za-z0-9_]|$)/) {
            print i ":" cur " -> " lines[j]
          }
        }
      }
    }
  '
}

report_body_read_bypass() {
  local message="response bodies must be read through HttpAttemptExecutor (BoundedErrorBody for failures, read_success_json for success); drivers may not call reqwest Response::text()/json()/bytes()/chunk() directly, including split across lines (BoundedErrorBody::text() is a sync accessor with no .await and is exempt)."
  local file_list="$1"
  local hit=0
  local file matches
  while IFS= read -r file; do
    [ -n "$file" ] || continue
    matches="$(strip_test_tail "$file" | scan_body_read_bypass || true)"
    if [ -n "$matches" ]; then
      hit=1
      while IFS= read -r line; do
        printf '%s:%s\n' "$file" "$line" >&2
      done <<<"$matches"
    fi
  done <"$file_list"
  if [ "$hit" -ne 0 ]; then
    printf '[architecture] %s\n' "$message" >&2
    fail=1
  fi
}

report_body_read_bypass "$DRIVER_FILES_LIST"

# error_log.rs itself legitimately defines log_network_error/log_http_error/
# ErrorLogContext/LlmApiErrorRecord; exclude it from the "no other driver may
# reference these" scan.
report_pattern \
  "log_network_error / log_http_error / ErrorLogContext / LlmApiErrorRecord are HTTP/network diagnostics private to http_attempt.rs + error_log.rs; other drivers must call error_log::log_stream_protocol_error instead." \
  'log_network_error|log_http_error|LlmApiErrorRecord|ErrorLogContext' \
  "$DRIVER_FILES_EXCEPT_ERROR_LOG_LIST"

if [ "$fail" -ne 0 ]; then
  exit 2
fi

echo "Provider HttpAttemptExecutor guard OK."
