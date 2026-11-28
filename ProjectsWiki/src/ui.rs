use crate::config::AppConfig;

pub fn render_app_html(cfg: &AppConfig) -> String {
    let wiki_port = cfg.port_web;
    let default_preset = &cfg.default_label_preset;
    let template = include_str!("ui_page.html");
    template
        .replace("__WIKI_PORT__", &wiki_port.to_string())
        .replace("__DEFAULT_PRESET__", default_preset)
}
