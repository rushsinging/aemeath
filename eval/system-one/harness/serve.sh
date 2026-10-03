#!/usr/bin/env bash
# System One 候选引擎服务管理（#1751 阶段一评测用）
# 用法: serve.sh {start|stop|restart|status|wait} [engine|all]
# 引擎: jevos | llama-emb | clm | rsi-jev | kev | laya
# 约定: 所有服务 nohup 后台运行，PID/日志落 runtime/logs/，wait 子命令带超时轮询健康检查。
set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EVAL_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
RUNTIME="$EVAL_ROOT/runtime"
LOG_DIR="$RUNTIME/logs"
PID_DIR="$RUNTIME/pids"
mkdir -p "$LOG_DIR" "$PID_DIR"

# 端口分配（参考 local-jev-bench 的逐引擎独占端口约定）
port_of() {
  case "$1" in
    jevos)    echo 8017 ;;
    llama-emb) echo 8090 ;;
    clm)      echo 8700 ;;
    rsi-jev)  echo 8200 ;;
    kev)      echo 8009 ;;
    laya)     echo 8000 ;;
    *) return 1 ;;
  esac
}

health_url_of() {
  case "$1" in
    jevos)    echo "http://127.0.0.1:8017/health" ;;
    llama-emb) echo "http://127.0.0.1:8090/health" ;;
    clm)      echo "http://127.0.0.1:8700/health" ;;
    rsi-jev)  echo "http://127.0.0.1:8200/health" ;;
    kev)      echo "http://127.0.0.1:8009/v1/models" ;;
    laya)     echo "http://127.0.0.1:8000/health" ;;
    *) return 1 ;;
  esac
}

# qwen3:8b 的 GGUF 直接复用 Ollama 已下载 blob（Q4_K_M, 5.2GB），避免重复下载
ollama_qwen3_blob() {
  ls -S ~/.ollama/models/blobs/sha256-* 2>/dev/null | head -1
}

is_running() { # $1=engine → 0 若 PID 存活且端口监听
  local pid_file="$PID_DIR/$1.pid"
  [[ -f "$pid_file" ]] || return 1
  local pid; pid="$(cat "$pid_file")"
  [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null
}

start_engine() {
  local engine="$1" port pid_file log_file
  port="$(port_of "$engine")" || { echo "未知引擎: $engine" >&2; return 1; }
  pid_file="$PID_DIR/$engine.pid"
  log_file="$LOG_DIR/$engine.log"

  if is_running "$engine"; then
    echo "[$engine] 已在运行 (PID $(cat "$pid_file"))"; return 0
  fi
  # 端口被非本脚本管理的进程占用时，先清理（如历史残留 clm-serve）
  local holder; holder="$(lsof -tnP -iTCP:"$port" -sTCP:LISTEN 2>/dev/null | head -1)"
  if [[ -n "$holder" ]]; then
    echo "[$engine] 端口 $port 被 PID $holder 占用，终止后重启"
    kill "$holder" 2>/dev/null; sleep 2
    kill -9 "$holder" 2>/dev/null
  fi

  # 双重 fork + setsid 彻底脱离会话：父进程同步等待首个子进程（秒退），
  # 孙子进程在新会话中 exec 服务，调用方（含 AI agent 的 Bash 工具）立即返回。
  spawn_daemon() { # $1=pid_file $2=log_file $3=cwd，其余=命令
    local _pid_file="$1" _log_file="$2" _cwd="$3"; shift 3
    python3 - "$_pid_file" "$_log_file" "$_cwd" "$@" <<'PYEOF'
import os, sys
pid_file, log_file, cwd, argv = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4:]
pid = os.fork()
if pid:
    os.waitpid(pid, 0)
    sys.exit(0)
os.setsid()
pid2 = os.fork()
if pid2:
    with open(pid_file, "w") as fh:
        fh.write(str(pid2))
    os._exit(0)
fd = os.open(log_file, os.O_WRONLY | os.O_CREAT | os.O_APPEND)
os.dup2(fd, 1); os.dup2(fd, 2)
devnull = os.open(os.devnull, os.O_RDONLY); os.dup2(devnull, 0)
os.chdir(cwd)
os.execvp(argv[0], argv)
PYEOF
  }

  case "$engine" in
    jevos)
      spawn_daemon "$pid_file" "$log_file" "$RUNTIME/jevos/jev" ./jev serve --port "$port"
      ;;
    llama-emb)
      local blob; blob="$(ollama_qwen3_blob)"
      [[ -n "$blob" ]] || { echo "[llama-emb] 未找到 Ollama qwen3 blob" >&2; return 1; }
      spawn_daemon "$pid_file" "$log_file" "$RUNTIME" llama-server -m "$blob" --embedding --pooling last --port "$port" -c 2048 --host 127.0.0.1
      ;;
    clm)
      spawn_daemon "$pid_file" "$log_file" "$RUNTIME/clm" .venv/bin/clm-serve --port "$port" \
        --emb-url http://127.0.0.1:8090/v1/embeddings --emb-model qwen3-8b \
        --device cpu --no-ui
      ;;
    rsi-jev)
      spawn_daemon "$pid_file" "$log_file" "$RUNTIME/rsi-jev/repo" "$RUNTIME/rsi-jev/.venv/bin/python" scripts/serve.py \
        --ckpt shgao/rsi-jev-v3.0-qwen3.5-2b --port "$port"
      ;;
    kev)
      spawn_daemon "$pid_file" "$log_file" "$RUNTIME/kev" "$HOME/.local/bin/uv" run --extra serve python -m kev.serve \
        --run jaredpalmer/kev-0.8b --port "$port"
      ;;
    laya)
      spawn_daemon "$pid_file" "$log_file" "$RUNTIME/laya" env LAYA_DEVICE=mps LAYA_PRELOAD=1 \
        .venv/bin/laya-serve --port "$port"
      ;;
  esac
  echo "[$engine] 已启动 (PID $(cat "$pid_file"), 端口 $port)，日志: $log_file"
}

stop_engine() {
  local engine="$1" pid_file="$PID_DIR/$1.pid"
  if [[ -f "$pid_file" ]]; then
    local pid; pid="$(cat "$pid_file")"
    kill "$pid" 2>/dev/null && echo "[$engine] 已停止 (PID $pid)"
    rm -f "$pid_file"
  else
    echo "[$engine] 无 PID 文件"
  fi
}

status_engine() {
  local engine="$1" url; url="$(health_url_of "$engine")" || return 1
  local state="STOPPED"
  is_running "$engine" && state="RUNNING(PID $(cat "$PID_DIR/$engine.pid"))"
  local health; health="$(curl -s -m 3 "$url" 2>/dev/null | head -c 120)"
  printf '%-10s %-18s %s\n' "$engine" "$state" "${health:-health 无响应}"
}

wait_engine() { # $1=engine $2=超时秒（默认 120）——轮询直到健康或超时，快速失败不挂前台
  local engine="$1" timeout="${2:-120}" url; url="$(health_url_of "$engine")" || return 1
  local elapsed=0
  while (( elapsed < timeout )); do
    if curl -s -m 3 "$url" 2>/dev/null | grep -q .; then
      echo "[$engine] 健康 (${elapsed}s)"; return 0
    fi
    sleep 3; (( elapsed += 3 ))
  done
  echo "[$engine] 健康检查超时 (${timeout}s)，见 $LOG_DIR/$engine.log" >&2; return 1
}

ENGINES=(jevos llama-emb clm rsi-jev kev laya)
cmd="${1:-status}"; target="${2:-all}"
case "$cmd" in
  start|stop|restart|status|wait) ;;
  *) echo "用法: $0 {start|stop|restart|status|wait} [${ENGINES[*]}|all]" >&2; exit 2 ;;
esac

run_for() {
  local fn="$1" e
  if [[ "$target" == "all" ]]; then
    for e in "${ENGINES[@]}"; do "$fn" "$e"; done
  else
    "$fn" "$target"
  fi
}

case "$cmd" in
  start)   run_for start_engine ;;
  stop)    run_for stop_engine ;;
  restart) run_for stop_engine; run_for start_engine ;;
  status)  run_for status_engine ;;
  wait)    wait_engine "$target" "${3:-120}" ;;
esac
