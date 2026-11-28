use anyhow::Result;
use axum::extract::{DefaultBodyLimit, FromRequest, Multipart, Path as AxumPath, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use base64::Engine;
use serde::Deserialize;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

use crate::barcodes::decode::{decode_image_bytes, zbar_available};
use crate::barcodes::labels::{
    build_bin_label, build_part_label, render_label_svg, render_printable_html, BarcodeStyle,
    LabelPreset,
};
use crate::barcodes::pdf::render_labels_pdf;
use crate::barcodes::phomemo;
use crate::config::AppConfig;
use crate::db::Database;
use crate::export;
use crate::models::*;
use crate::scrape;
use crate::ui;
use crate::update;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Database>,
    pub cfg: AppConfig,
    pub update_cache: Arc<tokio::sync::Mutex<Option<(std::time::Instant, update::UpdateCheckResult)>>>,
}

pub async fn run_server(host: String, port: u16, cfg: AppConfig, db: Arc<Database>) -> Result<()> {
    let state = AppState {
        db,
        cfg: cfg.clone(),
        update_cache: Arc::new(tokio::sync::Mutex::new(None)),
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/assets/logo.png", get(logo_handler))
        .route("/api/images/:filename", get(serve_image_handler))
        .route("/api/proxy-image", get(proxy_image_handler))
        .route("/api/health", get(health_handler))
        .route("/api/updates/check", get(check_updates_handler))
        .route("/api/parts", get(list_parts_handler).post(create_part_handler))
        .route(
            "/api/parts/fetch-url",
            post(fetch_url_handler).layer(DefaultBodyLimit::max(8 * 1024 * 1024)),
        )
        .route(
            "/api/parts/:sku",
            get(get_part_handler)
                .put(update_part_handler)
                .delete(delete_part_handler),
        )
        .route("/api/parts/:sku/image", post(upload_part_image_handler))
        .route("/api/parts/:sku/adjust", post(adjust_stock_handler))
        .route("/api/parts/:sku/move", post(move_part_handler))
        .route("/api/parts/:sku/suppliers", post(add_part_supplier_handler))
        .route(
            "/api/parts/:sku/suppliers/:ps_id",
            delete(delete_part_supplier_handler),
        )
        .route(
            "/api/locations",
            get(list_locations_handler).post(upsert_location_handler),
        )
        .route(
            "/api/locations/:code",
            put(update_location_handler).delete(delete_location_handler),
        )
        .route("/api/locations/:code/parts", get(location_parts_handler))
        .route(
            "/api/suppliers",
            get(list_suppliers_handler).post(upsert_supplier_handler),
        )
        .route(
            "/api/suppliers/:id",
            put(update_supplier_handler).delete(delete_supplier_handler),
        )
        .route("/api/boms", get(list_boms_handler).post(upsert_bom_handler))
        .route("/api/boms/:slug/deduct", post(deduct_bom_handler))
        .route("/api/valuation", get(valuation_handler))
        .route("/api/reorder", get(reorder_handler))
        .route("/api/movements", get(movements_handler))
        .route("/api/labels/presets", get(label_presets_handler))
        .route("/api/labels/part/:file", get(part_label_handler))
        .route("/api/labels/bin/:file", get(bin_label_handler))
        .route("/api/labels/batch.pdf", post(batch_labels_pdf_handler))
        .route("/api/printers/phomemo/devices", get(phomemo_devices_handler))
        .route("/api/printers/phomemo/print", post(phomemo_print_handler))
        .route("/api/barcodes/decode", post(decode_barcode_handler))
        .route("/api/barcodes/resolve", get(resolve_barcode_handler))
        .route("/api/export", post(export_handler))
        .layer(cors)
        .with_state(state);

    let addr = format!("{}:{}", host, port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!(":: [BenchBear] Inventory & Barcode Hub live on http://{}", addr);
    axum::serve(listener, app).await?;
    Ok(())
}

type ApiResult<T> = std::result::Result<T, (StatusCode, Json<serde_json::Value>)>;

fn err_resp(status: StatusCode, msg: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({ "error": msg.into() })))
}

async fn index_handler(State(st): State<AppState>) -> Html<String> {
    Html(ui::render_app_html(&st.cfg))
}

async fn logo_handler() -> Response {
    const LOGO_PNG: &[u8] = include_bytes!("../docs/Bear-built-logo.png");
    let mut resp = LOGO_PNG.into_response();
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("image/png"),
    );
    resp.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=86400"),
    );
    resp
}

async fn health_handler(State(st): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    let (parts, locations, suppliers, boms) = st
        .db
        .counts()
        .map_err(|e| err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({
        "status": "ok",
        "version": update::CURRENT_VERSION,
        "db_path": st.cfg.db_path,
        "data_dir": st.cfg.data_dir,
        "wiki_url": format!("http://127.0.0.1:{}", st.cfg.port_web),
        "zbar_available": zbar_available(),
        "counts": {
            "parts": parts,
            "locations": locations,
            "suppliers": suppliers,
            "boms": boms
        }
    })))
}

#[derive(Deserialize)]
struct PartsQuery {
    q: Option<String>,
    category: Option<String>,
    location: Option<String>,
    supplier: Option<String>,
    low_stock: Option<String>,
    sort: Option<String>,
}

async fn list_parts_handler(
    State(st): State<AppState>,
    Query(q): Query<PartsQuery>,
) -> ApiResult<Json<Vec<Part>>> {
    let low = matches!(
        q.low_stock.as_deref(),
        Some("1") | Some("true") | Some("yes")
    );
    let parts = st
        .db
        .list_parts_sorted(
            q.q.as_deref(),
            q.category.as_deref(),
            q.location.as_deref(),
            q.supplier.as_deref(),
            low,
            q.sort.as_deref(),
        )
        .map_err(|e| err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(parts))
}

async fn create_part_handler(
    State(st): State<AppState>,
    Json(req): Json<UpsertPart>,
) -> ApiResult<Json<Part>> {
    let mut part = st
        .db
        .upsert_part(&req)
        .map_err(|e| err_resp(StatusCode::BAD_REQUEST, e.to_string()))?;
    if !req.image_url.trim().is_empty() {
        if let Ok(fname) = scrape::download_and_save_image(req.image_url.trim(), &part.sku, "part", &st.cfg.data_dir).await {
            if let Ok(updated) = st.db.set_part_image(&part.sku, "part", &fname) {
                part = updated;
            }
        }
    }
    if !req.pinout_image_url.trim().is_empty() {
        if let Ok(fname) = scrape::download_and_save_image(req.pinout_image_url.trim(), &part.sku, "pinout", &st.cfg.data_dir).await {
            if let Ok(updated) = st.db.set_part_image(&part.sku, "pinout", &fname) {
                part = updated;
            }
        }
    }
    let _ = export::sync_portable(&st.db, &st.cfg);
    Ok(Json(part))
}

async fn get_part_handler(
    State(st): State<AppState>,
    AxumPath(sku): AxumPath<String>,
) -> ApiResult<Json<PartDetail>> {
    let detail = st
        .db
        .get_part_detail(&sku)
        .map_err(|e| err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| err_resp(StatusCode::NOT_FOUND, format!("Part not found: {}", sku)))?;
    Ok(Json(detail))
}

async fn update_part_handler(
    State(st): State<AppState>,
    AxumPath(sku): AxumPath<String>,
    Json(mut req): Json<UpsertPart>,
) -> ApiResult<Json<Part>> {
    if req.sku.trim().is_empty() {
        req.sku = sku;
    }
    let mut part = st
        .db
        .upsert_part(&req)
        .map_err(|e| err_resp(StatusCode::BAD_REQUEST, e.to_string()))?;
    if !req.image_url.trim().is_empty() {
        if let Ok(fname) = scrape::download_and_save_image(req.image_url.trim(), &part.sku, "part", &st.cfg.data_dir).await {
            if let Ok(updated) = st.db.set_part_image(&part.sku, "part", &fname) {
                part = updated;
            }
        }
    }
    if !req.pinout_image_url.trim().is_empty() {
        if let Ok(fname) = scrape::download_and_save_image(req.pinout_image_url.trim(), &part.sku, "pinout", &st.cfg.data_dir).await {
            if let Ok(updated) = st.db.set_part_image(&part.sku, "pinout", &fname) {
                part = updated;
            }
        }
    }
    let _ = export::sync_portable(&st.db, &st.cfg);
    Ok(Json(part))
}

async fn delete_part_handler(
    State(st): State<AppState>,
    AxumPath(sku): AxumPath<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let deleted = st
        .db
        .delete_part(&sku)
        .map_err(|e| err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !deleted {
        return Err(err_resp(StatusCode::NOT_FOUND, format!("SKU not found: {}", sku)));
    }
    let _ = export::sync_portable(&st.db, &st.cfg);
    Ok(Json(serde_json::json!({ "deleted": true, "sku": sku })))
}

async fn adjust_stock_handler(
    State(st): State<AppState>,
    AxumPath(sku): AxumPath<String>,
    Json(req): Json<AdjustStockReq>,
) -> ApiResult<Json<Part>> {
    let part = st
        .db
        .adjust_stock(&sku, &req)
        .map_err(|e| err_resp(StatusCode::BAD_REQUEST, e.to_string()))?;
    let _ = export::sync_portable(&st.db, &st.cfg);
    Ok(Json(part))
}

async fn move_part_handler(
    State(st): State<AppState>,
    AxumPath(sku): AxumPath<String>,
    Json(req): Json<MovePartReq>,
) -> ApiResult<Json<Part>> {
    let part = st
        .db
        .move_part(&sku, &req)
        .map_err(|e| err_resp(StatusCode::BAD_REQUEST, e.to_string()))?;
    let _ = export::sync_portable(&st.db, &st.cfg);
    Ok(Json(part))
}

async fn add_part_supplier_handler(
    State(st): State<AppState>,
    AxumPath(sku): AxumPath<String>,
    Json(req): Json<UpsertPartSupplier>,
) -> ApiResult<Json<Vec<PartSupplier>>> {
    let rows = st
        .db
        .add_part_supplier(&sku, &req)
        .map_err(|e| err_resp(StatusCode::BAD_REQUEST, e.to_string()))?;
    let _ = export::sync_portable(&st.db, &st.cfg);
    Ok(Json(rows))
}

async fn delete_part_supplier_handler(
    State(st): State<AppState>,
    AxumPath((sku, ps_id)): AxumPath<(String, i64)>,
) -> ApiResult<Json<serde_json::Value>> {
    let ok = st
        .db
        .delete_part_supplier(&sku, ps_id)
        .map_err(|e| err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let _ = export::sync_portable(&st.db, &st.cfg);
    Ok(Json(serde_json::json!({ "deleted": ok })))
}

async fn list_locations_handler(
    State(st): State<AppState>,
) -> ApiResult<Json<Vec<LocationWithStats>>> {
    let locs = st
        .db
        .list_locations()
        .map_err(|e| err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(locs))
}

async fn upsert_location_handler(
    State(st): State<AppState>,
    Json(req): Json<UpsertLocation>,
) -> ApiResult<Json<LocationWithStats>> {
    let loc = st
        .db
        .upsert_location(&req)
        .map_err(|e| err_resp(StatusCode::BAD_REQUEST, e.to_string()))?;
    let _ = export::sync_portable(&st.db, &st.cfg);
    Ok(Json(loc))
}

async fn update_location_handler(
    State(st): State<AppState>,
    AxumPath(code): AxumPath<String>,
    Json(mut req): Json<UpsertLocation>,
) -> ApiResult<Json<LocationWithStats>> {
    if req.code.trim().is_empty() {
        req.code = code;
    }
    let loc = st
        .db
        .upsert_location(&req)
        .map_err(|e| err_resp(StatusCode::BAD_REQUEST, e.to_string()))?;
    let _ = export::sync_portable(&st.db, &st.cfg);
    Ok(Json(loc))
}

async fn delete_location_handler(
    State(st): State<AppState>,
    AxumPath(code): AxumPath<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let ok = st
        .db
        .delete_location(&code)
        .map_err(|e| err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let _ = export::sync_portable(&st.db, &st.cfg);
    Ok(Json(serde_json::json!({ "deleted": ok })))
}

async fn location_parts_handler(
    State(st): State<AppState>,
    AxumPath(code): AxumPath<String>,
) -> ApiResult<Json<Vec<Part>>> {
    let parts = st
        .db
        .list_parts(None, None, Some(&code), None, false)
        .map_err(|e| err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(parts))
}

async fn list_suppliers_handler(
    State(st): State<AppState>,
) -> ApiResult<Json<Vec<SupplierWithStats>>> {
    let rows = st
        .db
        .list_suppliers()
        .map_err(|e| err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(rows))
}

async fn upsert_supplier_handler(
    State(st): State<AppState>,
    Json(req): Json<UpsertSupplier>,
) -> ApiResult<Json<SupplierWithStats>> {
    let row = st
        .db
        .upsert_supplier(&req)
        .map_err(|e| err_resp(StatusCode::BAD_REQUEST, e.to_string()))?;
    let _ = export::sync_portable(&st.db, &st.cfg);
    Ok(Json(row))
}

async fn update_supplier_handler(
    State(st): State<AppState>,
    AxumPath(id): AxumPath<i64>,
    Json(req): Json<UpsertSupplier>,
) -> ApiResult<Json<SupplierWithStats>> {
    let row = st
        .db
        .update_supplier(id, &req)
        .map_err(|e| err_resp(StatusCode::BAD_REQUEST, e.to_string()))?;
    let _ = export::sync_portable(&st.db, &st.cfg);
    Ok(Json(row))
}

async fn delete_supplier_handler(
    State(st): State<AppState>,
    AxumPath(id): AxumPath<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    let ok = st
        .db
        .delete_supplier(id)
        .map_err(|e| err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let _ = export::sync_portable(&st.db, &st.cfg);
    Ok(Json(serde_json::json!({ "deleted": ok })))
}

async fn list_boms_handler(State(st): State<AppState>) -> ApiResult<Json<Vec<BomReport>>> {
    let boms = st
        .db
        .list_boms()
        .map_err(|e| err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(boms))
}

async fn upsert_bom_handler(
    State(st): State<AppState>,
    Json(req): Json<UpsertBom>,
) -> ApiResult<Json<BomReport>> {
    let bom = st
        .db
        .upsert_bom(&req)
        .map_err(|e| err_resp(StatusCode::BAD_REQUEST, e.to_string()))?;
    let _ = export::sync_portable(&st.db, &st.cfg);
    Ok(Json(bom))
}

async fn deduct_bom_handler(
    State(st): State<AppState>,
    AxumPath(slug): AxumPath<String>,
    Json(req): Json<DeductBomReq>,
) -> ApiResult<Json<BomReport>> {
    let bom = st
        .db
        .deduct_bom(&slug, &req)
        .map_err(|e| err_resp(StatusCode::BAD_REQUEST, e.to_string()))?;
    let _ = export::sync_portable(&st.db, &st.cfg);
    Ok(Json(bom))
}

async fn valuation_handler(State(st): State<AppState>) -> ApiResult<Json<ValuationReport>> {
    let val = st
        .db
        .compute_valuation()
        .map_err(|e| err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(val))
}

async fn reorder_handler(State(st): State<AppState>) -> ApiResult<Json<Vec<ReorderGroup>>> {
    let rows = st
        .db
        .compute_reorder_list()
        .map_err(|e| err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
struct MovementsQuery {
    limit: Option<i64>,
}

async fn movements_handler(
    State(st): State<AppState>,
    Query(q): Query<MovementsQuery>,
) -> ApiResult<Json<Vec<StockMovement>>> {
    let rows = st
        .db
        .list_movements(q.limit.unwrap_or(100))
        .map_err(|e| err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(rows))
}

async fn label_presets_handler() -> Json<Vec<LabelPresetInfo>> {
    Json(LabelPreset::all_presets())
}

#[derive(Deserialize)]
struct LabelQuery {
    barcode: Option<String>,
    preset: Option<String>,
    w_mm: Option<f64>,
    h_mm: Option<f64>,
    w_in: Option<f64>,
    h_in: Option<f64>,
    density: Option<u8>,
    media: Option<String>,
    copies: Option<usize>,
    print: Option<String>,
}

async fn part_label_handler(
    State(st): State<AppState>,
    AxumPath(file): AxumPath<String>,
    Query(q): Query<LabelQuery>,
) -> ApiResult<Response> {
    let (sku, ext) = split_label_file(&file);
    let part = st
        .db
        .get_part(&sku)
        .map_err(|e| err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| err_resp(StatusCode::NOT_FOUND, format!("SKU not found: {}", sku)))?;

    let preset = LabelPreset::from_dimensions(
        q.preset.as_deref().or(Some(&st.cfg.default_label_preset)),
        q.w_mm,
        q.h_mm,
        q.w_in,
        q.h_in,
    );
    let style = BarcodeStyle::from_str(q.barcode.as_deref().unwrap_or("combo"));
    let spec = build_part_label(&part, preset, style)
        .map_err(|e| err_resp(StatusCode::BAD_REQUEST, e.to_string()))?;

    respond_label(&spec, &ext, &q)
}

async fn bin_label_handler(
    State(st): State<AppState>,
    AxumPath(file): AxumPath<String>,
    Query(q): Query<LabelQuery>,
) -> ApiResult<Response> {
    let (code, ext) = split_label_file(&file);
    let loc = st
        .db
        .get_location(&code)
        .map_err(|e| err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| err_resp(StatusCode::NOT_FOUND, format!("Location not found: {}", code)))?;
    let parts = st
        .db
        .list_parts(None, None, Some(&loc.location.code), None, false)
        .map_err(|e| err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let preset = LabelPreset::from_dimensions(
        q.preset.as_deref().or(Some("ql62x42")),
        q.w_mm,
        q.h_mm,
        q.w_in,
        q.h_in,
    );
    let style = BarcodeStyle::from_str(q.barcode.as_deref().unwrap_or("combo"));
    let spec = build_bin_label(&loc, &parts, preset, style)
        .map_err(|e| err_resp(StatusCode::BAD_REQUEST, e.to_string()))?;

    respond_label(&spec, &ext, &q)
}

#[derive(Deserialize)]
struct BatchLabelItem {
    kind: String, // "part" | "bin"
    id: String,
    #[serde(default = "default_one")]
    copies: usize,
}

fn default_one() -> usize {
    1
}

#[derive(Deserialize)]
struct BatchLabelReq {
    #[serde(default = "default_preset")]
    preset: String,
    #[serde(default = "default_barcode")]
    barcode: String,
    w_mm: Option<f64>,
    h_mm: Option<f64>,
    w_in: Option<f64>,
    h_in: Option<f64>,
    items: Vec<BatchLabelItem>,
}

fn default_preset() -> String {
    "2x1".to_string()
}

fn default_barcode() -> String {
    "combo".to_string()
}

async fn batch_labels_pdf_handler(
    State(st): State<AppState>,
    Json(req): Json<BatchLabelReq>,
) -> ApiResult<Response> {
    let preset = LabelPreset::from_dimensions(Some(&req.preset), req.w_mm, req.h_mm, req.w_in, req.h_in);
    let style = BarcodeStyle::from_str(&req.barcode);
    let mut specs = Vec::new();

    for item in &req.items {
        let copies = item.copies.clamp(1, 50);
        if item.kind.eq_ignore_ascii_case("bin") || item.kind.eq_ignore_ascii_case("location") {
            if let Some(loc) = st
                .db
                .get_location(&item.id)
                .map_err(|e| err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
            {
                let parts = st
                    .db
                    .list_parts(None, None, Some(&loc.location.code), None, false)
                    .map_err(|e| err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
                let spec = build_bin_label(&loc, &parts, preset, style)
                    .map_err(|e| err_resp(StatusCode::BAD_REQUEST, e.to_string()))?;
                for _ in 0..copies {
                    specs.push(spec.clone());
                }
            }
        } else if let Some(part) = st
            .db
            .get_part(&item.id)
            .map_err(|e| err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        {
            let spec = build_part_label(&part, preset, style)
                .map_err(|e| err_resp(StatusCode::BAD_REQUEST, e.to_string()))?;
            for _ in 0..copies {
                specs.push(spec.clone());
            }
        }
    }

    if specs.is_empty() {
        return Err(err_resp(
            StatusCode::BAD_REQUEST,
            "No valid parts or locations matched batch label request",
        ));
    }

    let pdf_bytes = render_labels_pdf(&specs)
        .map_err(|e| err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let mut resp = pdf_bytes.into_response();
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/pdf"),
    );
    resp.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("inline; filename=\"labels-batch.pdf\""),
    );
    Ok(resp)
}

fn split_label_file(file: &str) -> (String, String) {
    if let Some(id) = file.strip_suffix(".pdf") {
        (id.to_string(), "pdf".to_string())
    } else if let Some(id) = file.strip_suffix(".png") {
        (id.to_string(), "png".to_string())
    } else if let Some(id) = file.strip_suffix(".phomemo") {
        (id.to_string(), "phomemo".to_string())
    } else if let Some(id) = file.strip_suffix(".svg") {
        (id.to_string(), "svg".to_string())
    } else if let Some(id) = file.strip_suffix(".html") {
        (id.to_string(), "html".to_string())
    } else {
        (file.to_string(), "svg".to_string())
    }
}

fn respond_label(
    spec: &crate::barcodes::labels::LabelSpec,
    ext: &str,
    q: &LabelQuery,
) -> ApiResult<Response> {
    if ext == "pdf" {
        let pdf = render_labels_pdf(std::slice::from_ref(spec))
            .map_err(|e| err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let mut resp = pdf.into_response();
        resp.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/pdf"),
        );
        return Ok(resp);
    }
    if ext == "png" {
        let png = phomemo::render_label_png(spec, phomemo::PHOMEMO_DPI)
            .map_err(|e| err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let mut resp = png.into_response();
        resp.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("image/png"),
        );
        return Ok(resp);
    }
    if ext == "phomemo" {
        let luma = phomemo::rasterize_label_spec_to_luma(spec, phomemo::PHOMEMO_DPI);
        let pkt = phomemo::encode_m110_packet(
            &luma,
            q.density.unwrap_or(10),
            q.media.as_deref().unwrap_or("gap"),
            q.copies.unwrap_or(1),
        );
        let mut resp = pkt.into_response();
        resp.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream"),
        );
        return Ok(resp);
    }
    if ext == "html" || q.print.as_deref() == Some("1") {
        return Ok(Html(render_printable_html(spec)).into_response());
    }
    let svg = render_label_svg(spec);
    let mut resp = svg.into_response();
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("image/svg+xml; charset=utf-8"),
    );
    Ok(resp)
}

async fn phomemo_devices_handler() -> Json<serde_json::Value> {
    let devices = phomemo::list_thermal_devices();
    Json(serde_json::json!({
        "devices": devices,
        "hints": {
            "linux_usb": "Linux exposes USB printers as /dev/usb/lp0 (add user to 'lp' group for passwordless access).",
            "linux_bt": "Bind paired M110/M120/M200 via: sudo rfcomm bind 0 <BT_MAC> 1 (creates /dev/rfcomm0).",
            "macos": "Paired/USB printers appear under /dev/cu.* (e.g. /dev/cu.PhomemoM110 or /dev/cu.usbmodem*).",
            "windows": "Paired Bluetooth SPP or USB serial ports appear as COM3, COM4, etc."
        }
    }))
}

#[derive(Deserialize)]
struct PhomemoPrintReq {
    device_path: String,
    #[serde(default = "default_kind_part")]
    kind: String,
    id: String,
    #[serde(default = "default_phomemo_preset")]
    preset: String,
    w_mm: Option<f64>,
    h_mm: Option<f64>,
    w_in: Option<f64>,
    h_in: Option<f64>,
    #[serde(default = "default_barcode")]
    barcode: String,
    #[serde(default = "default_density")]
    density: u8,
    #[serde(default = "default_media")]
    media_type: String,
    #[serde(default = "default_one")]
    copies: usize,
    #[serde(default)]
    image_base64: String,
}

fn default_kind_part() -> String {
    "part".to_string()
}

fn default_phomemo_preset() -> String {
    "m110_40x30".to_string()
}

fn default_density() -> u8 {
    10
}

fn default_media() -> String {
    "gap".to_string()
}

async fn phomemo_print_handler(
    State(st): State<AppState>,
    Json(req): Json<PhomemoPrintReq>,
) -> ApiResult<Json<serde_json::Value>> {
    phomemo::validate_thermal_device_path(&req.device_path)
        .map_err(|e| err_resp(StatusCode::BAD_REQUEST, e.to_string()))?;

    let luma = if !req.image_base64.trim().is_empty() {
        let b64 = req
            .image_base64
            .split_once(',')
            .map(|(_, r)| r)
            .unwrap_or(&req.image_base64);
        let raw_bytes = base64::engine::general_purpose::STANDARD
            .decode(b64.trim())
            .map_err(|e| err_resp(StatusCode::BAD_REQUEST, format!("Invalid base64 image: {}", e)))?;
        phomemo::decode_image_bytes_to_luma(&raw_bytes)
            .map_err(|e| err_resp(StatusCode::BAD_REQUEST, e.to_string()))?
    } else {
        let preset = LabelPreset::from_dimensions(Some(&req.preset), req.w_mm, req.h_mm, req.w_in, req.h_in);
        let style = BarcodeStyle::from_str(&req.barcode);
        let spec = if req.kind.eq_ignore_ascii_case("bin") || req.kind.eq_ignore_ascii_case("location") {
            let loc = st
                .db
                .get_location(&req.id)
                .map_err(|e| err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
                .ok_or_else(|| err_resp(StatusCode::NOT_FOUND, format!("Location not found: {}", req.id)))?;
            let parts = st
                .db
                .list_parts(None, None, Some(&loc.location.code), None, false)
                .map_err(|e| err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            build_bin_label(&loc, &parts, preset, style)
                .map_err(|e| err_resp(StatusCode::BAD_REQUEST, e.to_string()))?
        } else {
            let part = st
                .db
                .get_part(&req.id)
                .map_err(|e| err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
                .ok_or_else(|| err_resp(StatusCode::NOT_FOUND, format!("SKU not found: {}", req.id)))?;
            build_part_label(&part, preset, style)
                .map_err(|e| err_resp(StatusCode::BAD_REQUEST, e.to_string()))?
        };
        phomemo::rasterize_label_spec_to_luma(&spec, phomemo::PHOMEMO_DPI)
    };

    let width_dots = luma.width().min(phomemo::M110_MAX_WIDTH_DOTS);
    let height_dots = if luma.width() > phomemo::M110_MAX_WIDTH_DOTS {
        ((luma.height() as f64) * (phomemo::M110_MAX_WIDTH_DOTS as f64) / (luma.width() as f64))
            .round()
            .max(1.0) as u32
    } else {
        luma.height()
    };

    let packet = phomemo::encode_m110_packet(&luma, req.density, &req.media_type, req.copies);
    let bytes_sent = phomemo::write_packet_to_device(&req.device_path, &packet)
        .map_err(|e| err_resp(StatusCode::BAD_REQUEST, e.to_string()))?;

    Ok(Json(serde_json::json!({
        "ok": true,
        "device_path": req.device_path,
        "bytes_sent": bytes_sent,
        "width_dots": width_dots,
        "height_dots": height_dots,
        "copies": req.copies.clamp(1, 50)
    })))
}

#[derive(Deserialize)]
struct DecodeJsonReq {
    image_base64: String,
}

async fn decode_barcode_handler(
    State(st): State<AppState>,
    headers: axum::http::HeaderMap,
    request: axum::extract::Request,
) -> ApiResult<Json<serde_json::Value>> {
    let ct = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let img_bytes = if ct.starts_with("multipart/form-data") {
        let mut mp = Multipart::from_request(request, &st)
            .await
            .map_err(|e| err_resp(StatusCode::BAD_REQUEST, e.to_string()))?;
        let mut found = None;
        while let Some(field) = mp
            .next_field()
            .await
            .map_err(|e| err_resp(StatusCode::BAD_REQUEST, e.to_string()))?
        {
            let data = field
                .bytes()
                .await
                .map_err(|e| err_resp(StatusCode::BAD_REQUEST, e.to_string()))?;
            if !data.is_empty() {
                found = Some(data.to_vec());
                break;
            }
        }
        found.ok_or_else(|| err_resp(StatusCode::BAD_REQUEST, "No image file in multipart body"))?
    } else {
        let body = axum::body::to_bytes(request.into_body(), 10 * 1024 * 1024)
            .await
            .map_err(|e| err_resp(StatusCode::BAD_REQUEST, e.to_string()))?;
        let req: DecodeJsonReq = serde_json::from_slice(&body)
            .map_err(|e| err_resp(StatusCode::BAD_REQUEST, format!("Invalid JSON: {}", e)))?;
        let b64 = req
            .image_base64
            .split_once(',')
            .map(|(_, r)| r)
            .unwrap_or(&req.image_base64);
        base64::engine::general_purpose::STANDARD
            .decode(b64.trim())
            .map_err(|e| err_resp(StatusCode::BAD_REQUEST, format!("Invalid base64: {}", e)))?
    };

    let decoded = decode_image_bytes(&img_bytes, &st.db)
        .map_err(|e| err_resp(StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(serde_json::json!({
        "count": decoded.len(),
        "barcodes": decoded
    })))
}

#[derive(Deserialize)]
struct ResolveQuery {
    code: String,
}

async fn resolve_barcode_handler(
    State(st): State<AppState>,
    Query(q): Query<ResolveQuery>,
) -> ApiResult<Json<BarcodeResolveResult>> {
    let res = st
        .db
        .resolve_barcode(&q.code)
        .map_err(|e| err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(res))
}

async fn export_handler(State(st): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    export::sync_all(&st.db, &st.cfg)
        .map_err(|e| err_resp(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
struct FetchUrlReq {
    url: String,
    #[serde(default)]
    html: Option<String>,
}

async fn fetch_url_handler(
    State(st): State<AppState>,
    Json(req): Json<FetchUrlReq>,
) -> ApiResult<Json<ProductUrlDraft>> {
    let draft = if let Some(raw_html) = req.html.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        scrape::parse_product_html(&req.url, raw_html, &st.db)
    } else {
        scrape::fetch_product_url(&req.url, &st.db).await
    }
    .map_err(|e| err_resp(StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(draft))
}

async fn serve_image_handler(
    State(st): State<AppState>,
    AxumPath(filename): AxumPath<String>,
) -> ApiResult<Response> {
    if filename.is_empty()
        || filename.contains("..")
        || filename.contains('/')
        || filename.contains('\\')
    {
        return Err(err_resp(StatusCode::BAD_REQUEST, "Invalid image filename"));
    }
    let path = st.cfg.data_dir.join("images").join(&filename);
    let bytes = std::fs::read(&path)
        .map_err(|_| err_resp(StatusCode::NOT_FOUND, "Image not found"))?;
    let lower = filename.to_ascii_lowercase();
    let ct = if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else {
        "image/jpeg"
    };
    let mut resp = bytes.into_response();
    resp.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(ct));
    resp.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=3600"),
    );
    Ok(resp)
}

#[derive(Deserialize)]
struct ProxyImageQuery {
    url: String,
}

async fn proxy_image_handler(Query(q): Query<ProxyImageQuery>) -> ApiResult<Response> {
    let (bytes, ct) = scrape::fetch_proxy_image_bytes(&q.url)
        .await
        .map_err(|e| err_resp(StatusCode::BAD_REQUEST, e.to_string()))?;
    let mut resp = bytes.into_response();
    resp.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(ct));
    resp.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=1800"),
    );
    Ok(resp)
}

#[derive(Deserialize)]
struct UploadImageJsonReq {
    #[serde(default = "default_img_kind")]
    kind: String,
    #[serde(default)]
    url: String,
}

fn default_img_kind() -> String {
    "part".to_string()
}

async fn upload_part_image_handler(
    State(st): State<AppState>,
    AxumPath(sku): AxumPath<String>,
    headers: axum::http::HeaderMap,
    request: axum::extract::Request,
) -> ApiResult<Json<Part>> {
    let ct = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let (kind, filename) = if ct.starts_with("multipart/form-data") {
        let mut mp = Multipart::from_request(request, &st)
            .await
            .map_err(|e| err_resp(StatusCode::BAD_REQUEST, e.to_string()))?;
        let mut kind = "part".to_string();
        let mut img_bytes = None;
        while let Some(field) = mp
            .next_field()
            .await
            .map_err(|e| err_resp(StatusCode::BAD_REQUEST, e.to_string()))?
        {
            let fname = field.name().unwrap_or("").to_string();
            if fname == "kind" {
                kind = field.text().await.unwrap_or_else(|_| "part".to_string());
            } else {
                let data = field
                    .bytes()
                    .await
                    .map_err(|e| err_resp(StatusCode::BAD_REQUEST, e.to_string()))?;
                if !data.is_empty() {
                    img_bytes = Some(data.to_vec());
                }
            }
        }
        let bytes = img_bytes
            .ok_or_else(|| err_resp(StatusCode::BAD_REQUEST, "No image file provided"))?;
        let saved = scrape::save_uploaded_image(&bytes, &sku, &kind, &st.cfg.data_dir)
            .map_err(|e| err_resp(StatusCode::BAD_REQUEST, e.to_string()))?;
        (kind, saved)
    } else {
        let body = axum::body::to_bytes(request.into_body(), 1024 * 1024)
            .await
            .map_err(|e| err_resp(StatusCode::BAD_REQUEST, e.to_string()))?;
        let req: UploadImageJsonReq = serde_json::from_slice(&body)
            .map_err(|e| err_resp(StatusCode::BAD_REQUEST, format!("Invalid JSON: {}", e)))?;
        if req.url.trim().is_empty() {
            return Err(err_resp(StatusCode::BAD_REQUEST, "url is required"));
        }
        let saved = scrape::download_and_save_image(req.url.trim(), &sku, &req.kind, &st.cfg.data_dir)
            .await
            .map_err(|e| err_resp(StatusCode::BAD_REQUEST, e.to_string()))?;
        (req.kind, saved)
    };

    let part = st
        .db
        .set_part_image(&sku, &kind, &filename)
        .map_err(|e| err_resp(StatusCode::BAD_REQUEST, e.to_string()))?;
    let _ = export::sync_portable(&st.db, &st.cfg);
    Ok(Json(part))
}

#[derive(Deserialize)]
struct UpdateCheckQuery {
    force: Option<String>,
}

async fn check_updates_handler(
    State(st): State<AppState>,
    Query(q): Query<UpdateCheckQuery>,
) -> Json<update::UpdateCheckResult> {
    let force = matches!(q.force.as_deref(), Some("1") | Some("true") | Some("yes"));
    if !force {
        let guard = st.update_cache.lock().await;
        if let Some((ts, cached)) = guard.as_ref() {
            if cached.error.is_none() && ts.elapsed() < std::time::Duration::from_secs(600) {
                return Json(cached.clone());
            }
        }
    }

    let result = update::check_github_updates(&st.cfg.wiki_root).await;
    if result.error.is_none() {
        let mut guard = st.update_cache.lock().await;
        *guard = Some((std::time::Instant::now(), result.clone()));
    }
    Json(result)
}
