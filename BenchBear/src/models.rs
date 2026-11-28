use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    pub id: i64,
    pub code: String,
    pub name: String,
    pub area: String,
    pub rack: String,
    pub drawer: String,
    pub description: String,
    pub barcode_value: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationWithStats {
    #[serde(flatten)]
    pub location: Location,
    pub part_count: i64,
    pub total_qty: i64,
    pub total_value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertLocation {
    pub code: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub area: String,
    #[serde(default)]
    pub rack: String,
    #[serde(default)]
    pub drawer: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub barcode_value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Supplier {
    pub id: i64,
    pub name: String,
    pub website: String,
    pub account_url: String,
    pub currency: String,
    pub lead_time_days: i64,
    pub shipping_notes: String,
    pub notes: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupplierWithStats {
    #[serde(flatten)]
    pub supplier: Supplier,
    pub part_count: i64,
    pub total_value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertSupplier {
    pub name: String,
    #[serde(default)]
    pub website: String,
    #[serde(default)]
    pub account_url: String,
    #[serde(default = "default_usd")]
    pub currency: String,
    #[serde(default = "default_lead_time")]
    pub lead_time_days: i64,
    #[serde(default)]
    pub shipping_notes: String,
    #[serde(default)]
    pub notes: String,
}

fn default_usd() -> String {
    "USD".to_string()
}

fn default_lead_time() -> i64 {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Part {
    pub id: i64,
    pub sku: String,
    pub category: String,
    pub name: String,
    pub spec: String,
    pub package: String,
    pub qty: i64,
    pub min_qty: i64,
    pub unit_cost: f64,
    pub currency: String,
    pub location_id: Option<i64>,
    pub location_code: Option<String>,
    pub location_name: Option<String>,
    pub primary_supplier_id: Option<i64>,
    pub primary_supplier_name: Option<String>,
    pub mpn: String,
    pub supplier_sku: String,
    pub product_url: String,
    pub datasheet_url: String,
    pub barcode_value: String,
    pub projects_tags: String,
    #[serde(default)]
    pub description: String,
    pub notes: String,
    #[serde(default)]
    pub image_path: String,
    #[serde(default)]
    pub pinout_image_path: String,
    pub created_at: String,
    pub updated_at: String,
    pub extended_value: f64,
    pub low_stock: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertPart {
    pub sku: String,
    pub category: String,
    pub name: String,
    #[serde(default)]
    pub spec: String,
    #[serde(default)]
    pub package: String,
    #[serde(default)]
    pub qty: i64,
    #[serde(default = "default_min_qty")]
    pub min_qty: i64,
    #[serde(default)]
    pub unit_cost: f64,
    #[serde(default = "default_usd")]
    pub currency: String,
    #[serde(default)]
    pub location_code: Option<String>,
    #[serde(default)]
    pub supplier_name: Option<String>,
    #[serde(default)]
    pub mpn: String,
    #[serde(default)]
    pub supplier_sku: String,
    #[serde(default)]
    pub product_url: String,
    #[serde(default)]
    pub datasheet_url: String,
    #[serde(default)]
    pub barcode_value: String,
    #[serde(default)]
    pub projects_tags: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub image_path: String,
    #[serde(default)]
    pub pinout_image_path: String,
    #[serde(default)]
    pub image_url: String,
    #[serde(default)]
    pub pinout_image_url: String,
}

fn default_min_qty() -> i64 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartSupplier {
    pub id: i64,
    pub part_id: i64,
    pub supplier_id: i64,
    pub supplier_name: String,
    pub supplier_sku: String,
    pub mpn: String,
    pub unit_cost: f64,
    pub moq: i64,
    pub product_url: String,
    pub last_purchased_at: String,
    pub is_preferred: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertPartSupplier {
    pub supplier_id: Option<i64>,
    pub supplier_name: Option<String>,
    #[serde(default)]
    pub supplier_sku: String,
    #[serde(default)]
    pub mpn: String,
    #[serde(default)]
    pub unit_cost: f64,
    #[serde(default = "default_moq")]
    pub moq: i64,
    #[serde(default)]
    pub product_url: String,
    #[serde(default)]
    pub is_preferred: bool,
}

fn default_moq() -> i64 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockMovement {
    pub id: i64,
    pub part_id: i64,
    pub sku: String,
    pub part_name: String,
    pub delta: i64,
    pub old_qty: i64,
    pub new_qty: i64,
    pub unit_cost: Option<f64>,
    pub supplier_id: Option<i64>,
    pub supplier_name: Option<String>,
    pub reason: String,
    pub project_slug: String,
    pub note: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartDetail {
    pub part: Part,
    pub suppliers: Vec<PartSupplier>,
    pub movements: Vec<StockMovement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdjustStockReq {
    pub delta: i64,
    #[serde(default = "default_reason")]
    pub reason: String,
    #[serde(default)]
    pub note: String,
    pub unit_cost: Option<f64>,
    pub supplier_id: Option<i64>,
    pub project_slug: Option<String>,
}

fn default_reason() -> String {
    "adjust".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MovePartReq {
    pub location_code: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BomItemReport {
    pub sku: String,
    pub part_name: String,
    pub need: i64,
    pub have: i64,
    pub shortage: i64,
    pub unit_cost: f64,
    pub line_cost: f64,
    pub shortage_cost: f64,
    pub ok: bool,
    pub location_code: String,
    pub supplier_name: String,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BomReport {
    pub slug: String,
    pub title: String,
    pub status: String,
    pub notes: String,
    pub total_cost: f64,
    pub shortage_cost: f64,
    pub ready: bool,
    pub updated_at: String,
    pub items: Vec<BomItemReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertBomItem {
    pub sku: String,
    pub qty_needed: i64,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertBom {
    pub slug: String,
    pub title: String,
    #[serde(default = "default_bom_status")]
    pub status: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub items: Vec<UpsertBomItem>,
}

fn default_bom_status() -> String {
    "active".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeductBomReq {
    #[serde(default = "default_mult")]
    pub multiplier: i64,
    #[serde(default)]
    pub note: String,
}

fn default_mult() -> i64 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryValuation {
    pub category: String,
    pub sku_count: i64,
    pub total_units: i64,
    pub total_value: f64,
    pub low_stock_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationValuation {
    pub location_code: String,
    pub location_name: String,
    pub sku_count: i64,
    pub total_units: i64,
    pub total_value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupplierValuation {
    pub supplier_name: String,
    pub sku_count: i64,
    pub total_value: f64,
    pub low_stock_count: i64,
    pub reorder_est_cost: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValuationReport {
    pub total_skus: i64,
    pub total_units: i64,
    pub total_value: f64,
    pub low_stock_skus: i64,
    pub estimated_restock_cost: f64,
    pub by_category: Vec<CategoryValuation>,
    pub by_location: Vec<LocationValuation>,
    pub by_supplier: Vec<SupplierValuation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReorderItem {
    pub sku: String,
    pub part_name: String,
    pub category: String,
    pub qty: i64,
    pub min_qty: i64,
    pub suggested_order_qty: i64,
    pub unit_cost: f64,
    pub est_order_cost: f64,
    pub location_code: String,
    pub supplier_sku: String,
    pub mpn: String,
    pub product_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReorderGroup {
    pub supplier_id: Option<i64>,
    pub supplier_name: String,
    pub website: String,
    pub currency: String,
    pub lead_time_days: i64,
    pub group_total_cost: f64,
    pub items: Vec<ReorderItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelPresetInfo {
    pub id: String,
    pub name: String,
    pub width_mm: f64,
    pub height_mm: f64,
    pub width_pt: f64,
    pub height_pt: f64,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BarcodeResolveResult {
    pub raw_code: String,
    pub normalized_code: String,
    pub match_kind: String, // "part" | "location" | "supplier_bag" | "unknown"
    pub part: Option<Part>,
    pub location: Option<LocationWithStats>,
    pub parsed_mpn: Option<String>,
    pub parsed_supplier_sku: Option<String>,
    pub parsed_qty: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecodedBarcode {
    pub format: String,
    pub raw_text: String,
    pub engine: String,
    pub resolved: BarcodeResolveResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotBom {
    pub slug: String,
    pub title: String,
    pub status: String,
    pub notes: String,
    pub updated_at: String,
    pub items: Vec<UpsertBomItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub schema_version: i64,
    pub exported_at: String,
    pub locations: Vec<Location>,
    pub suppliers: Vec<Supplier>,
    pub parts: Vec<Part>,
    pub part_suppliers: Vec<PartSupplier>,
    pub boms: Vec<SnapshotBom>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateImage {
    pub url: String,
    pub label: String,
    pub likely_pinout: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductUrlDraft {
    pub url: String,
    pub fetched: bool,
    pub blocked: bool,
    pub warning: String,
    pub suggested_sku: String,
    pub category: String,
    pub name: String,
    pub spec: String,
    #[serde(default)]
    pub description: String,
    pub package: String,
    pub unit_cost: f64,
    pub pack_qty: i64,
    pub total_price: f64,
    pub currency: String,
    pub supplier_name: String,
    pub supplier_sku: String,
    pub mpn: String,
    pub datasheet_url: String,
    #[serde(default)]
    pub images: Vec<CandidateImage>,
}
