#!/usr/bin/env python3
"""
wiki_config.py - Centralized configuration loader for Kodiak ProjectsWiki.

Loads ~/.config/kodiak/wiki.toml with fallback to environment variables and
clean defaults. Does not hardcode personal usernames or directories.
"""
import os
import json
from pathlib import Path
from typing import Dict, Any, List

DEFAULT_CATEGORIES = {
    "Hardware & Electronics": [
        "hardware", "pcb", "circuit", "kicad", "schematic", "sensor",
        "display", "eink", "oled", "tft", "lcd", "led", "neopixel"
    ],
    "Embedded & Firmware": [
        "esp32", "rp2040", "pico", "arduino", "stm32", "firmware",
        "iot", "wifi", "ble", "bluetooth", "lora", "repeater"
    ],
    "Software & Tools": [
        "software", "cli", "app", "python", "rust", "desktop", "web",
        "tool", "server", "backend", "frontend", "linux"
    ],
    "Robotics & Mechanical": [
        "robot", "mechanical", "motor", "cad", "3d", "stl", "step",
        "engine", "stepper", "servo", "cnc"
    ],
    "Audio & Media": [
        "audio", "midi", "synth", "sampler", "sound", "dsp", "i2s", "music"
    ],
    "General & Prototypes": []
}

def _parse_toml_fallback(content: str) -> Dict[str, Any]:
    """Basic fallback TOML parser for simple string/int/list values."""
    res: Dict[str, Any] = {}
    current_section = None
    for line in content.splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("[") and line.endswith("]"):
            current_section = line[1:-1].strip()
            if current_section not in res:
                res[current_section] = {}
            continue
        if "=" in line:
            key, val = line.split("=", 1)
            key = key.strip().strip('"').strip("'")
            val = val.strip()
            # String parsing
            if val.startswith('"') and val.endswith('"'):
                parsed_val = val[1:-1]
            elif val.startswith("'") and val.endswith("'"):
                parsed_val = val[1:-1]
            elif val.isdigit():
                parsed_val = int(val)
            elif val.startswith("[") and val.endswith("]"):
                inner = val[1:-1].strip()
                if not inner:
                    parsed_val = []
                else:
                    parsed_val = [item.strip().strip('"').strip("'") for item in inner.split(",") if item.strip()]
            else:
                parsed_val = val
            
            if current_section:
                res[current_section][key] = parsed_val
            else:
                res[key] = parsed_val
    return res

def load_wiki_config(config_path: str = None) -> Dict[str, Any]:
    """Loads ~/.config/kodiak/wiki.toml or component fallback."""
    cfg: Dict[str, Any] = {
        "projects_dir": None,
        "site_domain": "wiki.local",
        "ollama_url": "http://127.0.0.1:11434",
        "ollama_model": "qwen2.5-coder:7b",
        "port_web": 8000,
        "port_api": 8765,
        "overrides_file": str(Path("~/.config/kodiak/project_overrides.json").expanduser()),
        "categories": DEFAULT_CATEGORIES.copy(),
    }

    target = Path(config_path).expanduser() if config_path else Path("~/.config/kodiak/wiki.toml").expanduser()
    
    if target.is_file():
        try:
            try:
                import tomllib
                with open(target, "rb") as f:
                    file_cfg = tomllib.load(f)
            except ImportError:
                try:
                    import tomli as tomllib
                    with open(target, "rb") as f:
                        file_cfg = tomllib.load(f)
                except ImportError:
                    with open(target, "r", encoding="utf-8") as f:
                        file_cfg = _parse_toml_fallback(f.read())
            
            for k in ("site_domain", "ollama_url", "ollama_model", "port_web", "port_api"):
                if k in file_cfg:
                    cfg[k] = file_cfg[k]
            
            if "projects_dir" in file_cfg and file_cfg["projects_dir"]:
                cfg["projects_dir"] = str(Path(file_cfg["projects_dir"]).expanduser())
            if "overrides_file" in file_cfg and file_cfg["overrides_file"]:
                cfg["overrides_file"] = str(Path(file_cfg["overrides_file"]).expanduser())
            if "categories" in file_cfg and isinstance(file_cfg["categories"], dict):
                cfg["categories"] = file_cfg["categories"]
        except Exception as e:
            print(f":: [ProjectsWiki] Warning: Failed parsing {target}: {e}")

    # Environment variable overrides
    if "PROJECTS_DIR" in os.environ and os.environ["PROJECTS_DIR"]:
        cfg["projects_dir"] = os.path.realpath(os.path.expanduser(os.environ["PROJECTS_DIR"]))
    if "WIKI_DOMAIN" in os.environ and os.environ["WIKI_DOMAIN"]:
        cfg["site_domain"] = os.environ["WIKI_DOMAIN"]
    if "WIKI_PORT_WEB" in os.environ and os.environ["WIKI_PORT_WEB"]:
        try:
            cfg["port_web"] = int(os.environ["WIKI_PORT_WEB"])
        except ValueError:
            pass
    if "WIKI_PORT_API" in os.environ and os.environ["WIKI_PORT_API"]:
        try:
            cfg["port_api"] = int(os.environ["WIKI_PORT_API"])
        except ValueError:
            pass
    if "OLLAMA_MODEL" in os.environ and os.environ["OLLAMA_MODEL"]:
        cfg["ollama_model"] = os.environ["OLLAMA_MODEL"]
    if "OLLAMA_URL" in os.environ and os.environ["OLLAMA_URL"]:
        cfg["ollama_url"] = os.environ["OLLAMA_URL"]

    # If projects_dir is still unresolved, locate default directory safely
    if not cfg["projects_dir"] or not os.path.isdir(cfg["projects_dir"]):
        std_p = Path("~/Projects").expanduser()
        if std_p.is_dir():
            cfg["projects_dir"] = str(std_p)
        else:
            cfg["projects_dir"] = str(Path("~/Projects").expanduser())

    return cfg

def load_overrides(cfg: Dict[str, Any] = None) -> Dict[str, Any]:
    """Loads external project overrides from configured path."""
    if cfg is None:
        cfg = load_wiki_config()
    
    path = Path(cfg["overrides_file"]).expanduser()
    if path.is_file():
        try:
            with open(path, "r", encoding="utf-8") as f:
                return json.load(f)
        except Exception:
            pass
    
    # Fallback to local project_overrides.json if present
    local_ovr = Path(__file__).resolve().parent / "project_overrides.json"
    if local_ovr.is_file():
        try:
            with open(local_ovr, "r", encoding="utf-8") as f:
                return json.load(f)
        except Exception:
            pass

    return {}

def save_overrides(overrides: Dict[str, Any], cfg: Dict[str, Any] = None) -> bool:
    """Saves external project overrides to configured path, creating directories if needed."""
    if cfg is None:
        cfg = load_wiki_config()
    
    path = Path(cfg["overrides_file"]).expanduser()
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        with open(path, "w", encoding="utf-8") as f:
            json.dump(overrides, f, indent=2)
        return True
    except Exception as e:
        print(f":: [ProjectsWiki] Error saving overrides to {path}: {e}")
        return False
