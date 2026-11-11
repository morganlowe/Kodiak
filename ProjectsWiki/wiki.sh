#!/usr/bin/env bash
# ==============================================================================
# Kodiak ProjectsWiki Launcher & Background Service Controller
# ==============================================================================
set -euo pipefail

WIKI_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Load configuration from wiki_config.py / ~/.config/kodiak/wiki.toml
if [[ -f "$WIKI_DIR/wiki_config.py" ]]; then
    eval "$(/usr/bin/python3 -c '
from wiki_config import load_wiki_config
cfg = load_wiki_config()
print(f"export PROJECTS_DIR=\"{cfg[\"projects_dir\"]}\"")
print(f"export HOST_NAME=\"{cfg[\"site_domain\"]}\"")
print(f"export PORT_WEB=\"{cfg[\"port_web\"]}\"")
print(f"export PORT_API=\"{cfg[\"port_api\"]}\"")
' 2>/dev/null || true)"
fi

export PROJECTS_DIR="${PROJECTS_DIR:-$HOME/Projects}"
HOST_NAME="${HOST_NAME:-wiki.local}"
PORT_WEB="${PORT_WEB:-8000}"
PORT_API="${PORT_API:-8765}"

LOCAL_VENV="$HOME/.local/share/kodiak/projectswiki-venv"
SYSCTL_CONF="/etc/sysctl.d/90-wiki-local.conf"
SERVICE_FILE="$HOME/.config/systemd/user/projects-wiki.service"
LOG_FILE="/tmp/projects-wiki.log"
PID_FILE="/tmp/projects-wiki.pids"

export NO_MKDOCS_2_WARNING=1

find_mkdocs() {
    if [[ -x "$LOCAL_VENV/bin/mkdocs" ]]; then
        echo "$LOCAL_VENV/bin/mkdocs"
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

ensure_dependencies() {
    if find_mkdocs >/dev/null 2>&1; then
        return 0
    fi
    echo ":: [ProjectsWiki] Setting up local Python environment at $LOCAL_VENV..."
    mkdir -p "$(dirname "$LOCAL_VENV")"
    /usr/bin/python3 -m venv "$LOCAL_VENV"
    "$LOCAL_VENV/bin/pip" install --upgrade pip
    "$LOCAL_VENV/bin/pip" install mkdocs-material pyyaml
}

setup_hosts_and_ports() {
    echo ":: [ProjectsWiki Setup] Optional Host & Port Configuration"
    if ! grep -qE "^[[:space:]]*127\.0\.0\.1[[:space:]]+.*\\b${HOST_NAME}\\b" /etc/hosts 2>/dev/null; then
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

stop_all() {
    systemctl --user stop projects-wiki.service 2>/dev/null || true
    if [[ -f "$PID_FILE" ]]; then
        while read -r pid; do
            [[ -n "$pid" ]] && kill "$pid" 2>/dev/null || true
        done < "$PID_FILE"
        rm -f "$PID_FILE"
    fi
    pkill -f "sync_api.py" 2>/dev/null || true
    pkill -f "mkdocs serve" 2>/dev/null || true
    sleep 0.5
}

sync_catalog() {
    local python_bin
    python_bin="$(find_python)"
    cd "$WIKI_DIR"
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
    if grep -qE "^[[:space:]]*127\.0\.0\.1[[:space:]]+.*\\b${HOST_NAME}\\b" /etc/hosts 2>/dev/null; then
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
    # If the user-level systemd service is active, avoid duplicate conflicting background process
    if systemctl --user is-active --quiet projects-wiki.service 2>/dev/null; then
        echo ":: ProjectsWiki is already running as a systemd service (projects-wiki.service)."
        resolve_runtime_urls
        echo "============================================================"
        echo "  [OK] ProjectsWiki is LIVE via systemd service!"
        echo "  Link:      ${URL}"
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

    cd "$WIKI_DIR"
    : > "$LOG_FILE"

    nohup env WIKI_REDIRECT_8000="$REDIRECT_8000" WIKI_SYNC_PORT="$PORT_API" "$python_bin" "$WIKI_DIR/sync_api.py" >> "$LOG_FILE" 2>&1 &
    local api_pid=$!

    nohup env NO_MKDOCS_2_WARNING=1 "$mkdocs_bin" serve -a "$BIND_ADDR" >> "$LOG_FILE" 2>&1 &
    local mk_pid=$!

    printf "%s\n%s\n" "$api_pid" "$mk_pid" > "$PID_FILE"

    # Wait up to 5 seconds to confirm server is listening
    for _ in {1..15}; do
        if ss -tln 2>/dev/null | grep -qE "${BIND_ADDR}\\b"; then
            break
        fi
        sleep 0.3
    done

    if kill -0 "$mk_pid" 2>/dev/null; then
        echo "============================================================"
        echo "  [OK] ProjectsWiki is LIVE in the background!"
        echo "  Link:      ${URL}"
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

    # Clean up any leftover background/orphaned processes from 'wiki start' before binding
    if [[ -f "$PID_FILE" ]]; then
        while read -r pid; do
            [[ -n "$pid" ]] && kill "$pid" 2>/dev/null || true
        done < "$PID_FILE"
        rm -f "$PID_FILE"
    fi
    pkill -f "sync_api.py" 2>/dev/null || true
    pkill -f "mkdocs serve" 2>/dev/null || true
    sleep 0.5

    sync_catalog
    resolve_runtime_urls

    cd "$WIKI_DIR"
    env WIKI_REDIRECT_8000="$REDIRECT_8000" WIKI_SYNC_PORT="$PORT_API" "$python_bin" "$WIKI_DIR/sync_api.py" &
    local api_pid=$!
    trap "kill $api_pid 2>/dev/null || true" EXIT INT TERM
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
        echo ":: [ProjectsWiki] Stopped background server."
        ;;
    restart)
        if systemctl --user is-active --quiet projects-wiki.service 2>/dev/null; then
            echo ":: Restarting projects-wiki.service via systemd..."
            systemctl --user restart projects-wiki.service
            echo ":: [ProjectsWiki] Restarted."
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
        echo ":: [ProjectsWiki] Catalog synced."
        ;;
    build)
        build_documentation "${@:2}"
        echo ":: [ProjectsWiki] Documentation built and catalog synced."
        ;;
    status)
        if systemctl --user is-active --quiet projects-wiki.service 2>/dev/null; then
            echo ":: [ProjectsWiki] Running (systemd service: projects-wiki.service)"
            exit 0
        fi
        if [[ -f "$PID_FILE" ]]; then
            local running=0
            while IFS= read -r pid; do
                if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
                    running=1
                fi
            done < "$PID_FILE"
            if [[ "$running" -eq 1 ]]; then
                echo ":: [ProjectsWiki] Running (background PID)"
                exit 0
            fi
        fi
        echo ":: [ProjectsWiki] Stopped"
        exit 3
        ;;
    *)
        echo "Usage: wiki [start | stop | restart | status | sync | build | logs | setup]"
        exit 1
        ;;
esac
