#!/usr/bin/env python3
"""
wiki_config.py - Centralized configuration loader for Projects & Parts Wiki.

Supports standalone config (~/.config/projects-parts-wiki/wiki.toml or ./wiki.toml)
with fallback to Kodiak config (~/.config/kodiak/wiki.toml), environment variables,
and clean defaults.
"""
import json
import os
from pathlib import Path
from typing import Any, Dict

WIKI_DIR = Path(__file__).resolve().parent

DEFAULT_CATEGORIES = {
    "Hardware & Electronics": [
        "hardware", "pcb", "circuit", "kicad", "schematic", "sensor",
        "display", "eink", "oled", "tft", "lcd", "led", "neopixel",
    ],
    "Embedded & Firmware": [
        "esp32", "rp2040", "pico", "arduino", "stm32", "firmware",
        "iot", "wifi", "ble", "bluetooth", "lora", "repeater",
    ],
    "Software & Tools": [
        "software", "cli", "app", "python", "rust", "desktop", "web",
        "tool", "server", "backend", "frontend", "linux",
    ],
    "Robotics & Mechanical": [
        "robot", "mechanical", "motor", "cad", "3d", "stl", "step",
        "engine", "stepper", "servo", "cnc",
    ],
    "Audio & Media": [
        "audio", "midi", "synth", "sampler", "sound", "dsp", "i2s", "music",
    ],
    "General & Prototypes": [],
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
            if val.startswith('"') and val.endswith('"'):
                parsed_val: Any = val[1:-1]
            elif val.startswith("'") and val.endswith("'"):
                parsed_val = val[1:-1]
            elif val.isdigit():
                parsed_val = int(val)
            elif val.startswith("[") and val.endswith("]"):
                inner = val[1:-1].strip()
                if not inner:
                    parsed_val = []
                else:
                    parsed_val = [
                        item.strip().strip('"').strip("'")
                        for item in inner.split(",")
                        if item.strip()
                    ]
            else:
                parsed_val = val

            if current_section:
                res[current_section][key] = parsed_val
            else:
                res[key] = parsed_val
    return res


def _resolve_config_file(config_path: str = None) -> Path | None:
    if config_path:
        p = Path(config_path).expanduser()
        return p if p.is_file() else None

    env_cfg = os.environ.get("WIKI_CONFIG")
    if env_cfg:
        p = Path(env_cfg).expanduser()
        if p.is_file():
            return p

    candidates = [
        Path("~/.config/benchbear/wiki.toml").expanduser(),
        WIKI_DIR / "wiki.toml",
        Path("~/.config/projects-parts-wiki/wiki.toml").expanduser(),
        Path("~/.config/kodiak/wiki.toml").expanduser(),
    ]
    for candidate in candidates:
        if candidate.is_file():
            return candidate
    return None


def _default_overrides_path() -> str:
    benchbear_ovr = Path("~/.config/benchbear/project_overrides.json").expanduser()
    standalone_ovr = Path("~/.config/projects-parts-wiki/project_overrides.json").expanduser()
    kodiak_ovr = Path("~/.config/kodiak/project_overrides.json").expanduser()
    if benchbear_ovr.is_file():
        return str(benchbear_ovr)
    if standalone_ovr.is_file():
        return str(standalone_ovr)
    if kodiak_ovr.is_file():
        return str(kodiak_ovr)
    return str(benchbear_ovr)


def _default_data_dir() -> str:
    """Use portable data beside BenchBear when writable, else a Kodiak user-data path."""
    local_data = WIKI_DIR / "data"
    if os.access(WIKI_DIR, os.W_OK):
        return str(local_data)
    if os.name == "nt":
        base = Path(os.environ.get("LOCALAPPDATA", Path.home() / "AppData/Local"))
        return str(base / "kodiak" / "projectswiki-data")
    return str(Path("~/.local/share/kodiak/projectswiki-data").expanduser())


def load_wiki_config(config_path: str = None) -> Dict[str, Any]:
    """Loads standalone BenchBear or Kodiak wiki.toml with environment variable overrides."""
    cfg: Dict[str, Any] = {
        "site_name": "BenchBear",
        "projects_dir": None,
        "site_domain": "benchbear.localhost",
        "ollama_url": "http://127.0.0.1:11434",
        "ollama_model": "qwen2.5-coder:7b",
        "port_web": 8000,
        "port_api": 8765,
        "port_inventory": 8090,
        "data_dir": _default_data_dir(),
        "default_currency": "USD",
        "default_label_preset": "2x1",
        "overrides_file": _default_overrides_path(),
        "categories": DEFAULT_CATEGORIES.copy(),
    }

    target = _resolve_config_file(config_path)
    if target is not None:
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

            for k in (
                "site_name",
                "site_domain",
                "ollama_url",
                "ollama_model",
                "port_web",
                "port_api",
                "port_inventory",
                "default_currency",
                "default_label_preset",
            ):
                if k in file_cfg and file_cfg[k] not in (None, ""):
                    cfg[k] = file_cfg[k]

            if "projects_dir" in file_cfg and file_cfg["projects_dir"]:
                cfg["projects_dir"] = str(Path(file_cfg["projects_dir"]).expanduser())
            if "data_dir" in file_cfg and file_cfg["data_dir"]:
                data_p = Path(file_cfg["data_dir"]).expanduser()
                if not data_p.is_absolute():
                    data_p = (WIKI_DIR / data_p).resolve()
                cfg["data_dir"] = str(data_p)
            if "overrides_file" in file_cfg and file_cfg["overrides_file"]:
                cfg["overrides_file"] = str(Path(file_cfg["overrides_file"]).expanduser())
            if "categories" in file_cfg and isinstance(file_cfg["categories"], dict):
                cfg["categories"] = file_cfg["categories"]
        except Exception as e:
            print(f":: [BenchBear] Warning: Failed parsing {target}: {e}")

    # Environment variable overrides
    if os.environ.get("PROJECTS_DIR"):
        cfg["projects_dir"] = os.path.realpath(os.path.expanduser(os.environ["PROJECTS_DIR"]))
    if os.environ.get("WIKI_DOMAIN"):
        cfg["site_domain"] = os.environ["WIKI_DOMAIN"]
    if os.environ.get("WIKI_SITE_NAME"):
        cfg["site_name"] = os.environ["WIKI_SITE_NAME"]
    if os.environ.get("WIKI_PORT_WEB"):
        try:
            cfg["port_web"] = int(os.environ["WIKI_PORT_WEB"])
        except ValueError:
            pass
    if os.environ.get("WIKI_PORT_API"):
        try:
            cfg["port_api"] = int(os.environ["WIKI_PORT_API"])
        except ValueError:
            pass
    if os.environ.get("WIKI_PORT_INVENTORY"):
        try:
            cfg["port_inventory"] = int(os.environ["WIKI_PORT_INVENTORY"])
        except ValueError:
            pass
    if os.environ.get("WIKI_DATA_DIR"):
        data_p = Path(os.environ["WIKI_DATA_DIR"]).expanduser()
        if not data_p.is_absolute():
            data_p = (WIKI_DIR / data_p).resolve()
        cfg["data_dir"] = str(data_p)
    if os.environ.get("WIKI_CURRENCY"):
        cfg["default_currency"] = os.environ["WIKI_CURRENCY"]
    if os.environ.get("WIKI_LABEL_PRESET"):
        cfg["default_label_preset"] = os.environ["WIKI_LABEL_PRESET"]
    if os.environ.get("OLLAMA_MODEL"):
        cfg["ollama_model"] = os.environ["OLLAMA_MODEL"]
    if os.environ.get("OLLAMA_URL"):
        cfg["ollama_url"] = os.environ["OLLAMA_URL"]

    if not cfg["projects_dir"] or not os.path.isdir(cfg["projects_dir"]):
        std_p = Path("~/Projects").expanduser()
        cfg["projects_dir"] = str(std_p)

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

    local_ovr = WIKI_DIR / "project_overrides.json"
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
        print(f":: [BenchBear] Error saving overrides to {path}: {e}")
        return False
