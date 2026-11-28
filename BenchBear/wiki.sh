#!/usr/bin/env bash
# ==============================================================================
# BenchBear Launcher & Background Service Controller
# ==============================================================================
set -euo pipefail

WIKI_DIR="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")" && pwd)"

# Load configuration from wiki_config.py
if [[ -f "$WIKI_DIR/wiki_config.py" ]]; then
    eval "$(/usr/bin/python3 -B -c '
import shlex, sys
sys.path.insert(0, sys.argv[1])
from wiki_config import load_wiki_config, ensure_workspace_ready
cfg = load_wiki_config()
ws = str(ensure_workspace_ready())
for k, v in (
    ("PROJECTS_DIR", cfg["projects_dir"]),
    ("HOST_NAME", cfg["site_domain"]),
    ("PORT_WEB", cfg["port_web"]),
    ("PORT_API", cfg["port_api"]),
    ("PORT_INV", cfg["port_inventory"]),
    ("DATA_DIR", cfg["data_dir"]),
    ("WORKSPACE_DIR", ws),
):
    print(f"export {k}={shlex.quote(str(v))}")
' "$WIKI_DIR" 2>/dev/null || true)"
fi

export PROJECTS_DIR="${PROJECTS_DIR:-$HOME/Projects}"
HOST_NAME="${HOST_NAME:-benchbear.localhost}"
PORT_WEB="${PORT_WEB:-8000}"
PORT_API="${PORT_API:-8765}"
PORT_INV="${PORT_INV:-8090}"
DATA_DIR="${DATA_DIR:-$WIKI_DIR/data}"
WORKSPACE_DIR="${WORKSPACE_DIR:-$WIKI_DIR}"

LOCAL_VENV="$HOME/.local/share/benchbear/venv"
KODIAK_VENV="$HOME/.local/share/kodiak/benchbear-venv"
RUST_BIN="${RUST_BIN:-$WIKI_DIR/target/release/benchbear}"
if [[ ! -x "$RUST_BIN" ]]; then
    if [[ -x "$WIKI_DIR/benchbear" ]]; then
        RUST_BIN="$WIKI_DIR/benchbear"
    elif [[ -x "$WIKI_DIR/bin/benchbear" ]]; then
        RUST_BIN="$WIKI_DIR/bin/benchbear"
    elif command -v benchbear >/dev/null 2>&1; then
        RUST_BIN="$(command -v benchbear)"
    fi
fi
SYSCTL_CONF="/etc/sysctl.d/90-wiki-local.conf"
SERVICE_FILE="$HOME/.config/systemd/user/benchbear.service"
LOG_FILE="/tmp/benchbear.log"
PID_FILE="/tmp/benchbear.pids"

export NO_MKDOCS_2_WARNING=1

find_mkdocs() {
    if [[ -x "$LOCAL_VENV/bin/mkdocs" ]]; then
        echo "$LOCAL_VENV/bin/mkdocs"
        return 0
    fi
    if [[ -x "$KODIAK_VENV/bin/mkdocs" ]]; then
        echo "$KODIAK_VENV/bin/mkdocs"
        return 0
    fi
    if command -v mkdocs >/dev/null 2>&1; then
        command -v mkdocs
        return 0
    fi
    if [[ -x "$HOME/.pyenv/shims/mkdocs" ]]; then
        echo "$HOME/.pyenv/shims/mkdocs"
        return 0
    fi
    for pyenv_mk in "$HOME"/.pyenv/versions/*/bin/mkdocs; do
        if [[ -x "$pyenv_mk" ]]; then
            echo "$pyenv_mk"
            return 0
        fi
    done
    if [[ -x "$HOME/.local/bin/mkdocs" ]]; then
        echo "$HOME/.local/bin/mkdocs"
        return 0
    fi
    return 1
}

find_python() {
    local mk
    if mk="$(find_mkdocs 2>/dev/null)"; then
        local bin_dir
        bin_dir="$(dirname "$mk")"
        if [[ -x "$bin_dir/python3" ]]; then
            echo "$bin_dir/python3"
            return 0
        elif [[ -x "$bin_dir/python" ]]; then
            echo "$bin_dir/python"
            return 0
        fi
    fi
    if [[ -x "$LOCAL_VENV/bin/python3" ]]; then
        echo "$LOCAL_VENV/bin/python3"
        return 0
    fi
    command -v python3 || echo "/usr/bin/python3"
}

ensure_rust_binary() {
    if [[ -x "$RUST_BIN" ]]; then
        return 0
    fi
    if command -v cargo >/dev/null 2>&1 && [[ -f "$WIKI_DIR/Cargo.toml" ]]; then
        echo ":: [BenchBear] Building Rust Inventory & Barcode binary (release)..."
        (cd "$WIKI_DIR" && cargo build --release)
        return $?
    fi
    echo ":: [BenchBear] Warning: cargo not found; skipping Rust binary build."
    return 1
}

ensure_dependencies() {
    if ! find_mkdocs >/dev/null 2>&1; then
        echo ":: [BenchBear] Setting up local Python environment at $LOCAL_VENV..."
        mkdir -p "$(dirname "$LOCAL_VENV")"
        /usr/bin/python3 -m venv "$LOCAL_VENV"
        "$LOCAL_VENV/bin/pip" install --upgrade pip
        "$LOCAL_VENV/bin/pip" install mkdocs-material pyyaml
    fi
    ensure_rust_binary || true
}

setup_hosts_and_ports() {
    echo ":: [BenchBear Setup] Optional Host & Port Configuration"
    if [[ "$HOST_NAME" == *.localhost ]]; then
        echo ":: ${HOST_NAME} resolves to loopback automatically; no hosts-file change is needed."
    elif ! grep -qE "^[[:space:]]*127\.0\.0\.1[[:space:]]+.*\\b${HOST_NAME}\\b" /etc/hosts 2>/dev/null; then
        read -r -p ":: Add '127.0.0.1 ${HOST_NAME}' to /etc/hosts? (Requires sudo) [y/N]: " resp
        if [[ "$resp" =~ ^[Yy] ]]; then
            echo "127.0.0.1 ${HOST_NAME}" | sudo tee -a /etc/hosts >/dev/null || true
            echo ":: Added ${HOST_NAME} to /etc/hosts."
        else
            echo ":: Skipping /etc/hosts. Access available at http://127.0.0.1:${PORT_WEB}"
        fi
    else
        echo ":: ${HOST_NAME} is already configured in /etc/hosts."
    fi

    local min_port
    min_port="$(sysctl -n net.ipv4.ip_unprivileged_port_start 2>/dev/null || echo 1024)"
    if [[ "$min_port" -gt 80 ]]; then
        echo ":: Optional: Enabling unprivileged port 80 allows accessing http://${HOST_NAME} without specifying port ${PORT_WEB}."
        read -r -p ":: Configure sysctl net.ipv4.ip_unprivileged_port_start=80? (Requires sudo) [y/N]: " resp
        if [[ "$resp" =~ ^[Yy] ]]; then
            if echo "net.ipv4.ip_unprivileged_port_start = 80" | sudo tee "$SYSCTL_CONF" >/dev/null; then
                sudo sysctl -w net.ipv4.ip_unprivileged_port_start=80 >/dev/null || true
                echo ":: Port 80 access enabled."
            fi
        else
            echo ":: Using default unprivileged port ${PORT_WEB}."
        fi
    fi
}

install_shell_integration() {
    mkdir -p "$HOME/.local/bin"
    ln -sf "$WIKI_DIR/wiki.sh" "$HOME/.local/bin/wiki"

    local fish_cfg="$HOME/.config/fish/config.fish"
    if [[ -f "$fish_cfg" ]]; then
        if grep -q "alias wiki=" "$fish_cfg"; then
            sed -i "s|alias wiki=.*|alias wiki=\"$WIKI_DIR/wiki.sh\"|g" "$fish_cfg"
        else
            echo "alias wiki=\"$WIKI_DIR/wiki.sh\"" >> "$fish_cfg"
        fi
    fi
}

stop_processes() {
    if [[ -f "$PID_FILE" ]]; then
        while read -r pid; do
            [[ -n "$pid" ]] && kill "$pid" 2>/dev/null || true
        done < "$PID_FILE"
        rm -f "$PID_FILE"
    fi
    pkill -f "sync_api.py" 2>/dev/null || true
    pkill -f "mkdocs serve" 2>/dev/null || true
    pkill -f "benchbear.*serve" 2>/dev/null || true
    sleep 0.5
}

stop_all() {
    systemctl --user stop benchbear.service 2>/dev/null || true
    stop_processes
}

sync_catalog() {
    local python_bin
    python_bin="$(find_python)"
    cd "$WORKSPACE_DIR"
    if [[ -x "$RUST_BIN" ]]; then
        "$RUST_BIN" --wiki-root "$WORKSPACE_DIR" --data-dir "$DATA_DIR" export >/dev/null || true
    fi
    "$python_bin" "$WIKI_DIR/make_wiki.py" >/dev/null
}

build_documentation() {
    local python_bin
    python_bin="$(find_python)"
    cd "$WIKI_DIR"
    "$python_bin" "$WIKI_DIR/build_wiki.py" "$@"
    sync_catalog
}

resolve_runtime_urls() {
    local min_port
    min_port="$(sysctl -n net.ipv4.ip_unprivileged_port_start 2>/dev/null || echo 1024)"
    BIND_ADDR="127.0.0.1:${PORT_WEB}"
    REDIRECT_8000="0"
    URL="http://127.0.0.1:${PORT_WEB}"

    local has_host=0
    if [[ "$HOST_NAME" == *.localhost ]] || grep -qE "^[[:space:]]*127\.0\.0\.1[[:space:]]+.*\\b${HOST_NAME}\\b" /etc/hosts 2>/dev/null; then
        has_host=1
    fi

    if [[ "$PORT_WEB" -eq 80 ]] || [[ "$min_port" -le 80 && "$has_host" -eq 1 ]]; then
        BIND_ADDR="127.0.0.1:80"
        URL="http://${HOST_NAME}"
        REDIRECT_8000="1"
    elif [[ "$has_host" -eq 1 ]]; then
        URL="http://${HOST_NAME}:${PORT_WEB}"
    fi
}

start_background() {
    if systemctl --user is-active --quiet benchbear.service 2>/dev/null; then
        echo ":: BenchBear is already running as a systemd service (benchbear.service)."
        resolve_runtime_urls
        echo "============================================================"
        echo "  [OK] BenchBear is LIVE via systemd service!"
        echo "  Wiki:      ${URL}"
        echo "  Inventory: http://127.0.0.1:${PORT_INV}"
        echo "  Stop:      wiki stop   |   Logs: wiki logs"
        echo "============================================================"
        return 0
    fi

    stop_all
    ensure_dependencies
    install_shell_integration

    local mkdocs_bin
    mkdocs_bin="$(find_mkdocs)"
    local python_bin
    python_bin="$(find_python)"

    sync_catalog
    resolve_runtime_urls

    cd "$WORKSPACE_DIR"
    : > "$LOG_FILE"

    nohup env WIKI_REDIRECT_8000="$REDIRECT_8000" WIKI_SYNC_PORT="$PORT_API" "$python_bin" "$WIKI_DIR/sync_api.py" >> "$LOG_FILE" 2>&1 &
    local api_pid=$!

    local inv_pid=""
    if [[ -x "$RUST_BIN" ]]; then
        nohup "$RUST_BIN" --wiki-root "$WORKSPACE_DIR" --data-dir "$DATA_DIR" serve --host 127.0.0.1 --port "$PORT_INV" >> "$LOG_FILE" 2>&1 &
        inv_pid=$!
    fi

    nohup env NO_MKDOCS_2_WARNING=1 "$mkdocs_bin" serve -a "$BIND_ADDR" >> "$LOG_FILE" 2>&1 &
    local mk_pid=$!

    printf "%s\n%s\n%s\n" "$api_pid" "$inv_pid" "$mk_pid" > "$PID_FILE"

    for _ in {1..15}; do
        if command -v ss >/dev/null 2>&1 && ss -tln 2>/dev/null | grep -qE "${BIND_ADDR}\\b"; then
            break
        elif command -v lsof >/dev/null 2>&1 && lsof -iTCP:"${PORT_WEB}" -sTCP:LISTEN >/dev/null 2>&1; then
            break
        fi
        sleep 0.3
    done

    if kill -0 "$mk_pid" 2>/dev/null; then
        echo "============================================================"
        echo "  [OK] BenchBear is LIVE in the background!"
        echo "  Wiki:      ${URL}"
        echo "  Inventory: http://127.0.0.1:${PORT_INV}"
        echo "  Stop:      wiki stop   |   Logs: wiki logs"
        echo "============================================================"
    else
        echo "[ERROR] Wiki failed to start. Recent log output ($LOG_FILE):"
        tail -n 20 "$LOG_FILE"
        exit 1
    fi
}

run_daemon_foreground() {
    ensure_dependencies
    local mkdocs_bin
    mkdocs_bin="$(find_mkdocs)"
    local python_bin
    python_bin="$(find_python)"

    stop_processes
    sync_catalog
    resolve_runtime_urls

    cd "$WORKSPACE_DIR"
    env WIKI_REDIRECT_8000="$REDIRECT_8000" WIKI_SYNC_PORT="$PORT_API" "$python_bin" "$WIKI_DIR/sync_api.py" &
    local api_pid=$!

    local inv_pid=""
    if [[ -x "$RUST_BIN" ]]; then
        "$RUST_BIN" --wiki-root "$WORKSPACE_DIR" --data-dir "$DATA_DIR" serve --host 127.0.0.1 --port "$PORT_INV" &
        inv_pid=$!
    fi

    trap "kill $api_pid $inv_pid 2>/dev/null || true" EXIT INT TERM
    "$mkdocs_bin" serve -a "$BIND_ADDR"
}

cmd="${1:-start}"
case "$cmd" in
    start|serve|"")
        start_background
        ;;
    setup|install)
        ensure_dependencies
        setup_hosts_and_ports
        install_shell_integration
        start_background
        ;;
    daemon|fg)
        run_daemon_foreground
        ;;
    stop)
        stop_all
        echo ":: [BenchBear] Stopped background server."
        ;;
    restart)
        if systemctl --user is-active --quiet benchbear.service 2>/dev/null; then
            echo ":: Restarting benchbear.service via systemd..."
            systemctl --user restart benchbear.service
            echo ":: [BenchBear] Restarted."
        else
            stop_all
            start_background
        fi
        ;;
    logs)
        tail -n 50 -f "$LOG_FILE"
        ;;
    sync|rebuild)
        sync_catalog
        echo ":: [BenchBear] Catalog and inventory exported."
        ;;
    build)
        build_documentation "${@:2}"
        echo ":: [BenchBear] Documentation built and catalog synced."
        ;;
    build-rust)
        (cd "$WIKI_DIR" && cargo build --release)
        echo ":: [BenchBear] Built $RUST_BIN"
        ;;
    labels)
        ensure_rust_binary
        "$RUST_BIN" --wiki-root "$WORKSPACE_DIR" --data-dir "$DATA_DIR" label "${@:2}"
        ;;
    check-update)
        if [[ -x "$RUST_BIN" ]]; then
            "$RUST_BIN" --wiki-root "$WORKSPACE_DIR" --data-dir "$DATA_DIR" check-update "${@:2}"
        else
            echo ":: [BenchBear] Binary not built yet; run './wiki.sh build-rust' or check https://github.com/morganlowe/BenchBear/releases"
        fi
        ;;
    update)
        cd "$WIKI_DIR"
        if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
            echo ":: [BenchBear] Error: $WIKI_DIR is not a git repository; cannot auto-update."
            exit 1
        fi
        if [[ -n "$(git status --porcelain)" ]]; then
            echo ":: [BenchBear] Uncommitted local changes detected in $WIKI_DIR."
            echo ":: [BenchBear] Commit or stash your local changes before running './wiki.sh update'."
            exit 1
        fi
        echo ":: [BenchBear] Fetching latest commits and tags from GitHub..."
        git fetch --tags origin
        git pull --ff-only
        echo ":: [BenchBear] Rebuilding release binary..."
        cargo build --release
        sync_catalog
        if systemctl --user is-active --quiet benchbear.service 2>/dev/null; then
            systemctl --user restart benchbear.service
            echo ":: [BenchBear] Updated and restarted systemd service."
        else
            stop_all
            start_background
        fi
        ;;
    status)
        if systemctl --user is-active --quiet benchbear.service 2>/dev/null; then
            echo ":: [BenchBear] Running (systemd service: benchbear.service)"
            exit 0
        fi
        if [[ -f "$PID_FILE" ]]; then
            running=0
            while IFS= read -r pid; do
                if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
                    running=1
                fi
            done < "$PID_FILE"
            if [[ "$running" -eq 1 ]]; then
                echo ":: [BenchBear] Running (background PID)"
                exit 0
            fi
        fi
        echo ":: [BenchBear] Stopped"
        exit 3
        ;;
    *)
        echo "Usage: wiki [start | stop | restart | status | sync | build | build-rust | labels | check-update | update | logs | setup]"
        exit 1
        ;;
esac
