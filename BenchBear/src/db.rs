use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use regex::Regex;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::models::*;

pub struct Database {
    conn: Mutex<Connection>,
    #[allow(dead_code)]
    pub db_path: Option<PathBuf>,
    pub audit_log_path: Option<PathBuf>,
}

impl Database {
    pub fn open(db_path: &Path, audit_log_path: Option<&Path>) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(db_path)
            .with_context(|| format!("Failed to open SQLite DB at {}", db_path.display()))?;
        configure_connection(&conn)?;
        let db = Self {
            conn: Mutex::new(conn),
            db_path: Some(db_path.to_path_buf()),
            audit_log_path: audit_log_path.map(|p| p.to_path_buf()),
        };
        db.init_schema()?;
        db.seed_default_suppliers()?;
        Ok(db)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        configure_connection(&conn)?;
        let db = Self {
            conn: Mutex::new(conn),
            db_path: None,
            audit_log_path: None,
        };
        db.init_schema()?;
        db.seed_default_suppliers()?;
        Ok(db)
    }

    fn init_schema(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS locations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                code TEXT NOT NULL UNIQUE,
                name TEXT NOT NULL DEFAULT '',
                area TEXT NOT NULL DEFAULT '',
                rack TEXT NOT NULL DEFAULT '',
                drawer TEXT NOT NULL DEFAULT '',
                description TEXT NOT NULL DEFAULT '',
                barcode_value TEXT NOT NULL UNIQUE,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS suppliers (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                website TEXT NOT NULL DEFAULT '',
                account_url TEXT NOT NULL DEFAULT '',
                currency TEXT NOT NULL DEFAULT 'USD',
                lead_time_days INTEGER NOT NULL DEFAULT 3,
                shipping_notes TEXT NOT NULL DEFAULT '',
                notes TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS parts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                sku TEXT NOT NULL UNIQUE,
                category TEXT NOT NULL,
                name TEXT NOT NULL,
                spec TEXT NOT NULL DEFAULT '',
                package TEXT NOT NULL DEFAULT '',
                qty INTEGER NOT NULL DEFAULT 0,
                min_qty INTEGER NOT NULL DEFAULT 1,
                unit_cost REAL NOT NULL DEFAULT 0.0,
                currency TEXT NOT NULL DEFAULT 'USD',
                location_id INTEGER REFERENCES locations(id) ON DELETE SET NULL,
                primary_supplier_id INTEGER REFERENCES suppliers(id) ON DELETE SET NULL,
                mpn TEXT NOT NULL DEFAULT '',
                supplier_sku TEXT NOT NULL DEFAULT '',
                product_url TEXT NOT NULL DEFAULT '',
                datasheet_url TEXT NOT NULL DEFAULT '',
                barcode_value TEXT NOT NULL UNIQUE,
                projects_tags TEXT NOT NULL DEFAULT '-',
                description TEXT NOT NULL DEFAULT '',
                notes TEXT NOT NULL DEFAULT '',
                image_path TEXT NOT NULL DEFAULT '',
                pinout_image_path TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS part_suppliers (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                part_id INTEGER NOT NULL REFERENCES parts(id) ON DELETE CASCADE,
                supplier_id INTEGER NOT NULL REFERENCES suppliers(id) ON DELETE CASCADE,
                supplier_sku TEXT NOT NULL DEFAULT '',
                mpn TEXT NOT NULL DEFAULT '',
                unit_cost REAL NOT NULL DEFAULT 0.0,
                moq INTEGER NOT NULL DEFAULT 1,
                product_url TEXT NOT NULL DEFAULT '',
                last_purchased_at TEXT NOT NULL DEFAULT '',
                is_preferred INTEGER NOT NULL DEFAULT 0,
                UNIQUE(part_id, supplier_id, supplier_sku)
            );

            CREATE TABLE IF NOT EXISTS stock_movements (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                part_id INTEGER NOT NULL REFERENCES parts(id) ON DELETE CASCADE,
                delta INTEGER NOT NULL,
                old_qty INTEGER NOT NULL,
                new_qty INTEGER NOT NULL,
                unit_cost REAL,
                supplier_id INTEGER REFERENCES suppliers(id) ON DELETE SET NULL,
                reason TEXT NOT NULL DEFAULT 'adjust',
                project_slug TEXT NOT NULL DEFAULT '',
                note TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS boms (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                slug TEXT NOT NULL UNIQUE,
                title TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'active',
                notes TEXT NOT NULL DEFAULT '',
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS bom_items (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                bom_id INTEGER NOT NULL REFERENCES boms(id) ON DELETE CASCADE,
                sku TEXT NOT NULL,
                qty_needed INTEGER NOT NULL DEFAULT 1,
                notes TEXT NOT NULL DEFAULT '',
                UNIQUE(bom_id, sku)
            );

            CREATE INDEX IF NOT EXISTS idx_parts_category ON parts(category);
            CREATE INDEX IF NOT EXISTS idx_parts_location ON parts(location_id);
            CREATE INDEX IF NOT EXISTS idx_parts_supplier ON parts(primary_supplier_id);
            CREATE INDEX IF NOT EXISTS idx_parts_mpn ON parts(mpn);
            CREATE INDEX IF NOT EXISTS idx_movements_part ON stock_movements(part_id, created_at DESC);
            "#,
        )?;
        migrate_parts_image_columns(&conn)?;
        Ok(())
    }

    fn seed_default_suppliers(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM suppliers", [], |r| r.get(0))?;
        if count > 0 {
            return Ok(());
        }
        let now = now_iso();
        let defaults = [
            ("DigiKey", "https://www.digikey.com", "https://www.digikey.com/en/mylists", "USD", 3, "Free shipping thresholds / USPS / FedEx"),
            ("Mouser", "https://www.mouser.com", "https://www.mouser.com/Cart/", "USD", 3, "Standard ground / 2-day"),
            ("LCSC", "https://www.lcsc.com", "https://www.lcsc.com/", "USD", 7, "Consolidate with JLCPCB PCB orders"),
            ("Amazon", "https://www.amazon.com", "https://www.amazon.com/gp/css/order-history", "USD", 2, "Prime / fast hardware assortments"),
            ("AliExpress", "https://www.aliexpress.com", "https://www.aliexpress.com/", "USD", 14, "Bulk modules and cables"),
            ("McMaster-Carr", "https://www.mcmaster.com", "https://www.mcmaster.com/", "USD", 2, "Next-day mechanical fasteners & stock"),
            ("Adafruit", "https://www.adafruit.com", "https://www.adafruit.com/", "USD", 4, "Maker breakouts & STEMMA QT"),
            ("SparkFun", "https://www.sparkfun.com", "https://www.sparkfun.com/", "USD", 4, "Qwiic boards & dev kits"),
        ];
        for (name, web, acct, curr, lead, ship) in defaults {
            conn.execute(
                "INSERT OR IGNORE INTO suppliers (name, website, account_url, currency, lead_time_days, shipping_notes, notes, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, '', ?7, ?7)",
                params![name, web, acct, curr, lead, ship, now],
            )?;
        }
        Ok(())
    }

    fn append_audit(&self, action: &str, detail: serde_json::Value) {
        if let Some(ref p) = self.audit_log_path {
            if let Some(parent) = p.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(p) {
                let line = serde_json::json!({
                    "ts": now_iso(),
                    "tool": action,
                    "detail": detail,
                });
                let _ = writeln!(f, "{}", line);
            }
        }
    }

    pub fn counts(&self) -> Result<(i64, i64, i64, i64)> {
        let conn = self.conn.lock().unwrap();
        let parts: i64 = conn.query_row("SELECT COUNT(*) FROM parts", [], |r| r.get(0))?;
        let locs: i64 = conn.query_row("SELECT COUNT(*) FROM locations", [], |r| r.get(0))?;
        let sups: i64 = conn.query_row("SELECT COUNT(*) FROM suppliers", [], |r| r.get(0))?;
        let boms: i64 = conn.query_row("SELECT COUNT(*) FROM boms", [], |r| r.get(0))?;
        Ok((parts, locs, sups, boms))
    }

    pub fn list_locations(&self) -> Result<Vec<LocationWithStats>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT l.id, l.code, l.name, l.area, l.rack, l.drawer, l.description, l.barcode_value,
                    l.created_at, l.updated_at,
                    COUNT(p.id) AS part_count,
                    COALESCE(SUM(p.qty), 0) AS total_qty,
                    COALESCE(SUM(p.qty * p.unit_cost), 0.0) AS total_value
             FROM locations l
             LEFT JOIN parts p ON p.location_id = l.id
             GROUP BY l.id
             ORDER BY l.code ASC",
        )?;
        let rows = stmt.query_map([], map_location_with_stats)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn get_location(&self, code: &str) -> Result<Option<LocationWithStats>> {
        let conn = self.conn.lock().unwrap();
        let norm = normalize_location_code(code);
        conn.query_row(
            "SELECT l.id, l.code, l.name, l.area, l.rack, l.drawer, l.description, l.barcode_value,
                    l.created_at, l.updated_at,
                    COUNT(p.id) AS part_count,
                    COALESCE(SUM(p.qty), 0) AS total_qty,
                    COALESCE(SUM(p.qty * p.unit_cost), 0.0) AS total_value
             FROM locations l
             LEFT JOIN parts p ON p.location_id = l.id
             WHERE UPPER(l.code) = UPPER(?1) OR UPPER(l.code) = UPPER(?2)
             GROUP BY l.id",
            params![code.trim(), norm],
            map_location_with_stats,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn upsert_location(&self, req: &UpsertLocation) -> Result<LocationWithStats> {
        let code = normalize_location_code(&req.code);
        if code.is_empty() {
            return Err(anyhow!("Location code cannot be empty"));
        }
        let name = if req.name.trim().is_empty() {
            req.code.trim().to_string()
        } else {
            req.name.trim().to_string()
        };
        let barcode = if req.barcode_value.trim().is_empty() {
            format!("BIN:{}", code)
        } else {
            req.barcode_value.trim().to_string()
        };
        let now = now_iso();
        {
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO locations (code, name, area, rack, drawer, description, barcode_value, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
                 ON CONFLICT(code) DO UPDATE SET
                    name = excluded.name,
                    area = excluded.area,
                    rack = excluded.rack,
                    drawer = excluded.drawer,
                    description = excluded.description,
                    barcode_value = excluded.barcode_value,
                    updated_at = excluded.updated_at",
                params![
                    code,
                    name,
                    req.area.trim(),
                    req.rack.trim(),
                    req.drawer.trim(),
                    req.description.trim(),
                    barcode,
                    now
                ],
            )?;
        }
        self.append_audit("upsert_location", serde_json::json!({"code": code, "name": name}));
        self.get_location(&code)?
            .ok_or_else(|| anyhow!("Failed to load location after upsert"))
    }

    pub fn delete_location(&self, code: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let norm = normalize_location_code(code);
        let affected = conn.execute(
            "DELETE FROM locations WHERE UPPER(code) = UPPER(?1) OR UPPER(code) = UPPER(?2)",
            params![code.trim(), norm],
        )?;
        Ok(affected > 0)
    }

    pub fn list_suppliers(&self) -> Result<Vec<SupplierWithStats>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT s.id, s.name, s.website, s.account_url, s.currency, s.lead_time_days,
                    s.shipping_notes, s.notes, s.created_at, s.updated_at,
                    COUNT(p.id) AS part_count,
                    COALESCE(SUM(p.qty * p.unit_cost), 0.0) AS total_value
             FROM suppliers s
             LEFT JOIN parts p ON p.primary_supplier_id = s.id
             GROUP BY s.id
             ORDER BY LOWER(s.name) ASC",
        )?;
        let rows = stmt.query_map([], map_supplier_with_stats)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn upsert_supplier(&self, req: &UpsertSupplier) -> Result<SupplierWithStats> {
        let name = req.name.trim();
        if name.is_empty() {
            return Err(anyhow!("Supplier name cannot be empty"));
        }
        let currency = if req.currency.trim().is_empty() {
            "USD".to_string()
        } else {
            req.currency.trim().to_uppercase()
        };
        let now = now_iso();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO suppliers (name, website, account_url, currency, lead_time_days, shipping_notes, notes, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
             ON CONFLICT(name) DO UPDATE SET
                website = excluded.website,
                account_url = excluded.account_url,
                currency = excluded.currency,
                lead_time_days = excluded.lead_time_days,
                shipping_notes = excluded.shipping_notes,
                notes = excluded.notes,
                updated_at = excluded.updated_at",
            params![
                name,
                req.website.trim(),
                req.account_url.trim(),
                currency,
                req.lead_time_days,
                req.shipping_notes.trim(),
                req.notes.trim(),
                now
            ],
        )?;
        let out = conn.query_row(
            "SELECT s.id, s.name, s.website, s.account_url, s.currency, s.lead_time_days,
                    s.shipping_notes, s.notes, s.created_at, s.updated_at,
                    COUNT(p.id) AS part_count,
                    COALESCE(SUM(p.qty * p.unit_cost), 0.0) AS total_value
             FROM suppliers s
             LEFT JOIN parts p ON p.primary_supplier_id = s.id
             WHERE LOWER(s.name) = LOWER(?1)
             GROUP BY s.id",
            params![name],
            map_supplier_with_stats,
        )?;
        drop(conn);
        self.append_audit("upsert_supplier", serde_json::json!({"name": name}));
        Ok(out)
    }

    pub fn update_supplier(&self, id: i64, req: &UpsertSupplier) -> Result<SupplierWithStats> {
        let name = req.name.trim();
        if name.is_empty() {
            return Err(anyhow!("Supplier name cannot be empty"));
        }
        let now = now_iso();
        let conn = self.conn.lock().unwrap();
        let affected = conn.execute(
            "UPDATE suppliers
             SET name = ?1, website = ?2, account_url = ?3, currency = ?4,
                 lead_time_days = ?5, shipping_notes = ?6, notes = ?7, updated_at = ?8
             WHERE id = ?9",
            params![
                name,
                req.website.trim(),
                req.account_url.trim(),
                if req.currency.trim().is_empty() { "USD" } else { req.currency.trim() },
                req.lead_time_days,
                req.shipping_notes.trim(),
                req.notes.trim(),
                now,
                id
            ],
        )?;
        if affected == 0 {
            return Err(anyhow!("Supplier id {} not found", id));
        }
        let out = conn.query_row(
            "SELECT s.id, s.name, s.website, s.account_url, s.currency, s.lead_time_days,
                    s.shipping_notes, s.notes, s.created_at, s.updated_at,
                    COUNT(p.id) AS part_count,
                    COALESCE(SUM(p.qty * p.unit_cost), 0.0) AS total_value
             FROM suppliers s
             LEFT JOIN parts p ON p.primary_supplier_id = s.id
             WHERE s.id = ?1
             GROUP BY s.id",
            params![id],
            map_supplier_with_stats,
        )?;
        Ok(out)
    }

    pub fn delete_supplier(&self, id: i64) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let affected = conn.execute("DELETE FROM suppliers WHERE id = ?1", params![id])?;
        Ok(affected > 0)
    }

    pub fn list_parts(
        &self,
        query: Option<&str>,
        category: Option<&str>,
        location: Option<&str>,
        supplier: Option<&str>,
        low_stock_only: bool,
    ) -> Result<Vec<Part>> {
        self.list_parts_sorted(query, category, location, supplier, low_stock_only, None)
    }

    pub fn list_parts_sorted(
        &self,
        query: Option<&str>,
        category: Option<&str>,
        location: Option<&str>,
        supplier: Option<&str>,
        low_stock_only: bool,
        sort: Option<&str>,
    ) -> Result<Vec<Part>> {
        let order_clause = match sort.unwrap_or("").trim().to_ascii_lowercase().as_str() {
            "created_desc" => "p.created_at DESC, p.id DESC",
            "created_asc" => "p.created_at ASC, p.id ASC",
            "updated_desc" => "p.updated_at DESC, p.id DESC",
            "updated_asc" => "p.updated_at ASC, p.id ASC",
            "name_asc" => "LOWER(p.name) ASC, p.sku ASC",
            "sku_asc" => "p.sku ASC",
            "qty_asc" => "p.qty ASC, p.sku ASC",
            "qty_desc" => "p.qty DESC, p.sku ASC",
            "value_desc" => "(p.qty * p.unit_cost) DESC, p.sku ASC",
            _ => "p.category ASC, p.sku ASC",
        };
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!("{PART_SELECT_SQL} ORDER BY {order_clause}"))?;
        let rows = stmt.query_map([], map_part)?;
        let q = query.unwrap_or("").trim().to_lowercase();
        let q_tokens: Vec<&str> = q.split_whitespace().collect();
        let cat = category.unwrap_or("").trim().to_lowercase();
        let loc = location.unwrap_or("").trim().to_lowercase();
        let loc_norm = normalize_location_code(location.unwrap_or("")).to_lowercase();
        let sup = supplier.unwrap_or("").trim().to_lowercase();

        let mut out = Vec::new();
        for r in rows {
            let p = r?;
            if low_stock_only && !p.low_stock {
                continue;
            }
            if !cat.is_empty() && p.category.to_lowercase() != cat {
                continue;
            }
            if !loc.is_empty() {
                let lc = p.location_code.as_deref().unwrap_or("").to_lowercase();
                let ln = p.location_name.as_deref().unwrap_or("").to_lowercase();
                if lc != loc && lc != loc_norm && ln != loc {
                    continue;
                }
            }
            if !sup.is_empty() {
                let sn = p.primary_supplier_name.as_deref().unwrap_or("").to_lowercase();
                if sn != sup {
                    continue;
                }
            }
            if !q_tokens.is_empty() {
                let haystack = format!(
                    "{} {} {} {} {} {} {} {} {} {} {} {}",
                    p.sku,
                    p.category,
                    p.name,
                    p.spec,
                    p.package,
                    p.location_code.as_deref().unwrap_or(""),
                    p.primary_supplier_name.as_deref().unwrap_or(""),
                    p.mpn,
                    p.supplier_sku,
                    p.projects_tags,
                    p.description,
                    p.notes
                )
                .to_lowercase();
                if !q_tokens.iter().all(|tok| haystack.contains(tok)) {
                    continue;
                }
            }
            out.push(p);
        }
        Ok(out)
    }

    pub fn get_part(&self, sku: &str) -> Result<Option<Part>> {
        let conn = self.conn.lock().unwrap();
        get_part_inner(&conn, sku)
    }

    pub fn get_part_detail(&self, sku: &str) -> Result<Option<PartDetail>> {
        let conn = self.conn.lock().unwrap();
        let Some(part) = get_part_inner(&conn, sku)? else {
            return Ok(None);
        };
        let mut s_stmt = conn.prepare(
            "SELECT ps.id, ps.part_id, ps.supplier_id, s.name, ps.supplier_sku, ps.mpn,
                    ps.unit_cost, ps.moq, ps.product_url, ps.last_purchased_at, ps.is_preferred
             FROM part_suppliers ps
             JOIN suppliers s ON s.id = ps.supplier_id
             WHERE ps.part_id = ?1
             ORDER BY ps.is_preferred DESC, ps.unit_cost ASC",
        )?;
        let suppliers = s_stmt
            .query_map(params![part.id], map_part_supplier)?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut m_stmt = conn.prepare(
            "SELECT m.id, m.part_id, p.sku, p.name, m.delta, m.old_qty, m.new_qty,
                    m.unit_cost, m.supplier_id, s.name, m.reason, m.project_slug, m.note, m.created_at
             FROM stock_movements m
             JOIN parts p ON p.id = m.part_id
             LEFT JOIN suppliers s ON s.id = m.supplier_id
             WHERE m.part_id = ?1
             ORDER BY m.id DESC
             LIMIT 50",
        )?;
        let movements = m_stmt
            .query_map(params![part.id], map_movement)?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(Some(PartDetail {
            part,
            suppliers,
            movements,
        }))
    }

    pub fn upsert_part(&self, req: &UpsertPart) -> Result<Part> {
        let sku = req.sku.trim().to_uppercase();
        if sku.is_empty() {
            return Err(anyhow!("SKU cannot be empty"));
        }
        let category = normalize_category(&req.category);
        let name = req.name.trim();
        if name.is_empty() {
            return Err(anyhow!("Part name cannot be empty"));
        }
        let barcode = if req.barcode_value.trim().is_empty() {
            format!("SKU:{}", sku)
        } else {
            req.barcode_value.trim().to_string()
        };
        let currency = if req.currency.trim().is_empty() {
            "USD".to_string()
        } else {
            req.currency.trim().to_uppercase()
        };
        let projects_tags = if req.projects_tags.trim().is_empty() {
            "-".to_string()
        } else {
            req.projects_tags.trim().to_string()
        };
        let now = now_iso();

        let (part_id, is_new, old_qty) = {
            let conn = self.conn.lock().unwrap();
            let loc_id = match req.location_code.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                Some(c) => Some(ensure_location_id(&conn, c, &now)?),
                None => None,
            };
            let sup_id = match req.supplier_name.as_deref().map(str::trim).filter(|s| !s.is_empty() && *s != "-") {
                Some(s) => Some(ensure_supplier_id(&conn, s, &now)?),
                None => None,
            };

            let existing: Option<(i64, i64)> = conn
                .query_row(
                    "SELECT id, qty FROM parts WHERE UPPER(sku) = UPPER(?1)",
                    params![sku],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;

            if let Some((eid, eqty)) = existing {
                conn.execute(
                    "UPDATE parts SET
                        category = ?1, name = ?2, spec = ?3, package = ?4,
                        qty = ?5, min_qty = ?6, unit_cost = ?7, currency = ?8,
                        location_id = ?9, primary_supplier_id = ?10,
                        mpn = ?11, supplier_sku = ?12, product_url = ?13, datasheet_url = ?14,
                        barcode_value = ?15, projects_tags = ?16, description = ?17, notes = ?18,
                        image_path = CASE WHEN ?19 = '__CLEAR__' THEN '' WHEN ?19 <> '' THEN ?19 ELSE image_path END,
                        pinout_image_path = CASE WHEN ?20 = '__CLEAR__' THEN '' WHEN ?20 <> '' THEN ?20 ELSE pinout_image_path END,
                        updated_at = ?21
                     WHERE id = ?22",
                    params![
                        category,
                        name,
                        req.spec.trim(),
                        req.package.trim(),
                        req.qty.max(0),
                        req.min_qty.max(0),
                        req.unit_cost.max(0.0),
                        currency,
                        loc_id,
                        sup_id,
                        req.mpn.trim(),
                        req.supplier_sku.trim(),
                        req.product_url.trim(),
                        req.datasheet_url.trim(),
                        barcode,
                        projects_tags,
                        req.description.trim(),
                        req.notes.trim(),
                        req.image_path.trim(),
                        req.pinout_image_path.trim(),
                        now,
                        eid
                    ],
                )?;
                if let Some(sid) = sup_id {
                    let _ = upsert_part_supplier_inner(
                        &conn,
                        eid,
                        sid,
                        req.supplier_sku.trim(),
                        req.mpn.trim(),
                        req.unit_cost.max(0.0),
                        1,
                        req.product_url.trim(),
                        true,
                    );
                }
                (eid, false, eqty)
            } else {
                let init_img = if req.image_path.trim() == "__CLEAR__" { "" } else { req.image_path.trim() };
                let init_pin = if req.pinout_image_path.trim() == "__CLEAR__" { "" } else { req.pinout_image_path.trim() };
                conn.execute(
                    "INSERT INTO parts (
                        sku, category, name, spec, package, qty, min_qty, unit_cost, currency,
                        location_id, primary_supplier_id, mpn, supplier_sku, product_url,
                        datasheet_url, barcode_value, projects_tags, description, notes, image_path, pinout_image_path,
                        created_at, updated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?22)",
                    params![
                        sku,
                        category,
                        name,
                        req.spec.trim(),
                        req.package.trim(),
                        req.qty.max(0),
                        req.min_qty.max(0),
                        req.unit_cost.max(0.0),
                        currency,
                        loc_id,
                        sup_id,
                        req.mpn.trim(),
                        req.supplier_sku.trim(),
                        req.product_url.trim(),
                        req.datasheet_url.trim(),
                        barcode,
                        projects_tags,
                        req.description.trim(),
                        req.notes.trim(),
                        init_img,
                        init_pin,
                        now
                    ],
                )?;
                let nid = conn.last_insert_rowid();
                if let Some(sid) = sup_id {
                    let _ = upsert_part_supplier_inner(
                        &conn,
                        nid,
                        sid,
                        req.supplier_sku.trim(),
                        req.mpn.trim(),
                        req.unit_cost.max(0.0),
                        1,
                        req.product_url.trim(),
                        true,
                    );
                }
                conn.execute(
                    "INSERT INTO stock_movements (part_id, delta, old_qty, new_qty, unit_cost, supplier_id, reason, project_slug, note, created_at)
                     VALUES (?1, ?2, 0, ?2, ?3, ?4, 'initial', '', 'Initial part entry', ?5)",
                    params![nid, req.qty.max(0), req.unit_cost.max(0.0), sup_id, now],
                )?;
                (nid, true, 0)
            }
        };

        if !is_new && old_qty != req.qty.max(0) {
            let conn = self.conn.lock().unwrap();
            let new_q = req.qty.max(0);
            conn.execute(
                "INSERT INTO stock_movements (part_id, delta, old_qty, new_qty, unit_cost, supplier_id, reason, project_slug, note, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL, 'edit', '', 'Updated via part editor', ?6)",
                params![part_id, new_q - old_qty, old_qty, new_q, req.unit_cost.max(0.0), now],
            )?;
        }

        self.append_audit("upsert_part", serde_json::json!({"sku": sku, "qty": req.qty, "is_new": is_new}));
        self.get_part(&sku)?
            .ok_or_else(|| anyhow!("Part not found after upsert"))
    }

    pub fn set_part_image(&self, sku: &str, kind: &str, filename: &str) -> Result<Part> {
        let now = now_iso();
        let col = if kind.eq_ignore_ascii_case("pinout") {
            "pinout_image_path"
        } else {
            "image_path"
        };
        {
            let conn = self.conn.lock().unwrap();
            let sql = format!("UPDATE parts SET {} = ?1, updated_at = ?2 WHERE UPPER(sku) = UPPER(?3)", col);
            let affected = conn.execute(&sql, params![filename.trim(), now, sku.trim()])?;
            if affected == 0 {
                return Err(anyhow!("SKU not found: {}", sku));
            }
        }
        self.append_audit("set_part_image", serde_json::json!({"sku": sku, "kind": kind, "file": filename}));
        self.get_part(sku)?.ok_or_else(|| anyhow!("SKU not found: {}", sku))
    }

    pub fn delete_part(&self, sku: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let affected = conn.execute("DELETE FROM parts WHERE UPPER(sku) = UPPER(?1)", params![sku.trim()])?;
        drop(conn);
        if affected > 0 {
            self.append_audit("delete_part", serde_json::json!({"sku": sku.trim()}));
        }
        Ok(affected > 0)
    }

    pub fn adjust_stock(&self, sku: &str, req: &AdjustStockReq) -> Result<Part> {
        let now = now_iso();
        let (old_qty, new_qty, canonical_sku) = {
            let mut conn = self.conn.lock().unwrap();
            let tx = conn.transaction()?;
            let (part_id, c_sku, cur_qty, cur_cost): (i64, String, i64, f64) = tx
                .query_row(
                    "SELECT id, sku, qty, unit_cost FROM parts WHERE UPPER(sku) = UPPER(?1)",
                    params![sku.trim()],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .optional()?
                .ok_or_else(|| anyhow!("SKU not found: {}", sku))?;

            let new_qty = (cur_qty + req.delta).max(0);
            let effective_cost = req.unit_cost.unwrap_or(cur_cost);

            if req.unit_cost.is_some() && req.delta > 0 {
                tx.execute(
                    "UPDATE parts SET qty = ?1, unit_cost = ?2, updated_at = ?3 WHERE id = ?4",
                    params![new_qty, effective_cost.max(0.0), now, part_id],
                )?;
            } else {
                tx.execute(
                    "UPDATE parts SET qty = ?1, updated_at = ?2 WHERE id = ?3",
                    params![new_qty, now, part_id],
                )?;
            }

            tx.execute(
                "INSERT INTO stock_movements (part_id, delta, old_qty, new_qty, unit_cost, supplier_id, reason, project_slug, note, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    part_id,
                    req.delta,
                    cur_qty,
                    new_qty,
                    req.unit_cost.or(Some(cur_cost)),
                    req.supplier_id,
                    req.reason.trim(),
                    req.project_slug.as_deref().unwrap_or(""),
                    req.note.trim(),
                    now
                ],
            )?;
            tx.commit()?;
            (cur_qty, new_qty, c_sku)
        };

        self.append_audit(
            "adjust_stock",
            serde_json::json!({
                "sku": canonical_sku,
                "delta": req.delta,
                "old_qty": old_qty,
                "new_qty": new_qty,
                "reason": req.reason,
                "note": req.note,
            }),
        );
        self.get_part(&canonical_sku)?
            .ok_or_else(|| anyhow!("Failed to load part after stock adjustment"))
    }

    pub fn move_part(&self, sku: &str, req: &MovePartReq) -> Result<Part> {
        let now = now_iso();
        let norm_loc = normalize_location_code(&req.location_code);
        if norm_loc.is_empty() {
            return Err(anyhow!("Target location code cannot be empty"));
        }
        let canonical_sku = {
            let mut conn = self.conn.lock().unwrap();
            let tx = conn.transaction()?;
            let (part_id, c_sku, cur_qty): (i64, String, i64) = tx
                .query_row(
                    "SELECT id, sku, qty FROM parts WHERE UPPER(sku) = UPPER(?1)",
                    params![sku.trim()],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?
                .ok_or_else(|| anyhow!("SKU not found: {}", sku))?;
            let loc_id = ensure_location_id(&tx, &req.location_code, &now)?;
            tx.execute(
                "UPDATE parts SET location_id = ?1, updated_at = ?2 WHERE id = ?3",
                params![loc_id, now, part_id],
            )?;
            let note = if req.note.trim().is_empty() {
                format!("Moved to {}", norm_loc)
            } else {
                format!("Moved to {}: {}", norm_loc, req.note.trim())
            };
            tx.execute(
                "INSERT INTO stock_movements (part_id, delta, old_qty, new_qty, unit_cost, supplier_id, reason, project_slug, note, created_at)
                 VALUES (?1, 0, ?2, ?2, NULL, NULL, 'move', '', ?3, ?4)",
                params![part_id, cur_qty, note, now],
            )?;
            tx.commit()?;
            c_sku
        };

        self.append_audit(
            "move_part",
            serde_json::json!({"sku": canonical_sku, "location_code": norm_loc, "note": req.note}),
        );
        self.get_part(&canonical_sku)?
            .ok_or_else(|| anyhow!("Part not found after move"))
    }

    pub fn add_part_supplier(&self, sku: &str, req: &UpsertPartSupplier) -> Result<Vec<PartSupplier>> {
        let now = now_iso();
        let conn = self.conn.lock().unwrap();
        let part_id: i64 = conn
            .query_row(
                "SELECT id FROM parts WHERE UPPER(sku) = UPPER(?1)",
                params![sku.trim()],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| anyhow!("SKU not found: {}", sku))?;

        let supplier_id = if let Some(sid) = req.supplier_id {
            sid
        } else if let Some(ref sname) = req.supplier_name {
            ensure_supplier_id(&conn, sname, &now)?
        } else {
            return Err(anyhow!("supplier_id or supplier_name is required"));
        };

        upsert_part_supplier_inner(
            &conn,
            part_id,
            supplier_id,
            req.supplier_sku.trim(),
            req.mpn.trim(),
            req.unit_cost.max(0.0),
            req.moq.max(1),
            req.product_url.trim(),
            req.is_preferred,
        )?;

        if req.is_preferred {
            conn.execute(
                "UPDATE parts SET primary_supplier_id = ?1, supplier_sku = CASE WHEN ?2 <> '' THEN ?2 ELSE supplier_sku END,
                 mpn = CASE WHEN ?3 <> '' THEN ?3 ELSE mpn END,
                 unit_cost = CASE WHEN ?4 > 0 THEN ?4 ELSE unit_cost END,
                 product_url = CASE WHEN ?5 <> '' THEN ?5 ELSE product_url END,
                 updated_at = ?6 WHERE id = ?7",
                params![
                    supplier_id,
                    req.supplier_sku.trim(),
                    req.mpn.trim(),
                    req.unit_cost.max(0.0),
                    req.product_url.trim(),
                    now,
                    part_id
                ],
            )?;
        }

        let mut stmt = conn.prepare(
            "SELECT ps.id, ps.part_id, ps.supplier_id, s.name, ps.supplier_sku, ps.mpn,
                    ps.unit_cost, ps.moq, ps.product_url, ps.last_purchased_at, ps.is_preferred
             FROM part_suppliers ps
             JOIN suppliers s ON s.id = ps.supplier_id
             WHERE ps.part_id = ?1
             ORDER BY ps.is_preferred DESC, ps.unit_cost ASC",
        )?;
        let rows = stmt
            .query_map(params![part_id], map_part_supplier)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn delete_part_supplier(&self, sku: &str, ps_id: i64) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let affected = conn.execute(
            "DELETE FROM part_suppliers
             WHERE id = ?1 AND part_id = (SELECT id FROM parts WHERE UPPER(sku) = UPPER(?2))",
            params![ps_id, sku.trim()],
        )?;
        Ok(affected > 0)
    }

    pub fn list_movements(&self, limit: i64) -> Result<Vec<StockMovement>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT m.id, m.part_id, p.sku, p.name, m.delta, m.old_qty, m.new_qty,
                    m.unit_cost, m.supplier_id, s.name, m.reason, m.project_slug, m.note, m.created_at
             FROM stock_movements m
             JOIN parts p ON p.id = m.part_id
             LEFT JOIN suppliers s ON s.id = m.supplier_id
             ORDER BY m.id DESC
             LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![limit.clamp(1, 500)], map_movement)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn list_part_suppliers_all(&self) -> Result<Vec<PartSupplier>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT ps.id, ps.part_id, ps.supplier_id, s.name, ps.supplier_sku, ps.mpn,
                    ps.unit_cost, ps.moq, ps.product_url, ps.last_purchased_at, ps.is_preferred
             FROM part_suppliers ps
             JOIN suppliers s ON s.id = ps.supplier_id
             ORDER BY ps.part_id ASC, ps.is_preferred DESC",
        )?;
        let rows = stmt
            .query_map([], map_part_supplier)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn next_sku_for_category(&self, category: &str) -> Result<String> {
        let cat = normalize_category(category);
        let prefix = match cat.as_str() {
            "microcontrollers" => "MCU".to_string(),
            "passives" => "PAS".to_string(),
            "connectors" => "CON".to_string(),
            "sensors-modules" => "SEN".to_string(),
            "hardware" => "HDW".to_string(),
            other => {
                let letters: String = other
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric())
                    .take(3)
                    .collect::<String>()
                    .to_uppercase();
                if letters.is_empty() {
                    "PRT".to_string()
                } else {
                    letters
                }
            }
        };
        let like_pat = format!("{}-%", prefix);
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT sku FROM parts WHERE UPPER(sku) LIKE ?1")?;
        let rows = stmt.query_map(params![like_pat], |r| r.get::<_, String>(0))?;
        let mut max_num = 0i64;
        for row in rows {
            if let Ok(sku) = row {
                if let Some((_, num_part)) = sku.split_once('-') {
                    if let Ok(n) = num_part.trim().parse::<i64>() {
                        if n > max_num {
                            max_num = n;
                        }
                    }
                }
            }
        }
        Ok(format!("{}-{:03}", prefix, max_num + 1))
    }

    pub fn list_boms(&self) -> Result<Vec<BomReport>> {
        let parts = self.list_parts(None, None, None, None, false)?;
        let parts_by_sku: HashMap<String, Part> = parts
            .into_iter()
            .map(|p| (p.sku.to_uppercase(), p))
            .collect();

        let conn = self.conn.lock().unwrap();
        let mut b_stmt = conn.prepare(
            "SELECT id, slug, title, status, notes, updated_at FROM boms ORDER BY slug ASC",
        )?;
        let boms: Vec<(i64, String, String, String, String, String)> = b_stmt
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut i_stmt = conn.prepare(
            "SELECT sku, qty_needed, notes FROM bom_items WHERE bom_id = ?1 ORDER BY id ASC",
        )?;

        let mut reports = Vec::new();
        for (bom_id, slug, title, status, notes, updated_at) in boms {
            let raw_items: Vec<(String, i64, String)> = i_stmt
                .query_map(params![bom_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<std::result::Result<Vec<_>, _>>()?;

            let mut items = Vec::new();
            let mut total_cost = 0.0;
            let mut shortage_cost = 0.0;
            let mut ready = !raw_items.is_empty();

            for (sku, need, item_notes) in raw_items {
                let key = sku.to_uppercase();
                let part_opt = parts_by_sku.get(&key);
                let have = part_opt.map(|p| p.qty).unwrap_or(0);
                let unit_cost = part_opt.map(|p| p.unit_cost).unwrap_or(0.0);
                let shortage = (need - have).max(0);
                let line_cost = round2(unit_cost * need as f64);
                let line_shortage_cost = round2(unit_cost * shortage as f64);
                let ok = have >= need;
                if !ok {
                    ready = false;
                }
                total_cost += line_cost;
                shortage_cost += line_shortage_cost;

                items.push(BomItemReport {
                    sku: key,
                    part_name: part_opt
                        .map(|p| p.name.clone())
                        .unwrap_or_else(|| "MISSING FROM INVENTORY".to_string()),
                    need,
                    have,
                    shortage,
                    unit_cost,
                    line_cost,
                    shortage_cost: line_shortage_cost,
                    ok,
                    location_code: part_opt
                        .and_then(|p| p.location_code.clone())
                        .unwrap_or_else(|| "-".to_string()),
                    supplier_name: part_opt
                        .and_then(|p| p.primary_supplier_name.clone())
                        .unwrap_or_else(|| "-".to_string()),
                    notes: item_notes,
                });
            }

            reports.push(BomReport {
                slug,
                title,
                status,
                notes,
                total_cost: round2(total_cost),
                shortage_cost: round2(shortage_cost),
                ready,
                updated_at,
                items,
            });
        }
        Ok(reports)
    }

    pub fn upsert_bom(&self, req: &UpsertBom) -> Result<BomReport> {
        let slug = req.slug.trim().to_lowercase().replace(' ', "-");
        if slug.is_empty() {
            return Err(anyhow!("BOM slug cannot be empty"));
        }
        let title = if req.title.trim().is_empty() {
            slug.clone()
        } else {
            req.title.trim().to_string()
        };
        let now = now_iso();
        {
            let mut conn = self.conn.lock().unwrap();
            let tx = conn.transaction()?;
            tx.execute(
                "INSERT INTO boms (slug, title, status, notes, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(slug) DO UPDATE SET
                    title = excluded.title,
                    status = excluded.status,
                    notes = excluded.notes,
                    updated_at = excluded.updated_at",
                params![slug, title, req.status.trim(), req.notes.trim(), now],
            )?;
            let bom_id: i64 = tx.query_row(
                "SELECT id FROM boms WHERE slug = ?1",
                params![slug],
                |r| r.get(0),
            )?;
            tx.execute("DELETE FROM bom_items WHERE bom_id = ?1", params![bom_id])?;
            for item in &req.items {
                let s = item.sku.trim().to_uppercase();
                if s.is_empty() || item.qty_needed <= 0 {
                    continue;
                }
                tx.execute(
                    "INSERT INTO bom_items (bom_id, sku, qty_needed, notes) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(bom_id, sku) DO UPDATE SET qty_needed = bom_items.qty_needed + excluded.qty_needed",
                    params![bom_id, s, item.qty_needed, item.notes.trim()],
                )?;
            }
            tx.commit()?;
        }
        self.append_audit("upsert_bom", serde_json::json!({"slug": slug, "item_count": req.items.len()}));
        self.list_boms()?
            .into_iter()
            .find(|b| b.slug == slug)
            .ok_or_else(|| anyhow!("BOM not found after upsert"))
    }

    pub fn deduct_bom(&self, slug: &str, req: &DeductBomReq) -> Result<BomReport> {
        let mult = req.multiplier.max(1);
        let now = now_iso();
        {
            let mut conn = self.conn.lock().unwrap();
            let tx = conn.transaction()?;
            let bom_id: i64 = tx
                .query_row(
                    "SELECT id FROM boms WHERE LOWER(slug) = LOWER(?1)",
                    params![slug.trim()],
                    |r| r.get(0),
                )
                .optional()?
                .ok_or_else(|| anyhow!("BOM not found: {}", slug))?;

            let mut stmt = tx.prepare("SELECT sku, qty_needed FROM bom_items WHERE bom_id = ?1")?;
            let items: Vec<(String, i64)> = stmt
                .query_map(params![bom_id], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            drop(stmt);

            if items.is_empty() {
                return Err(anyhow!("BOM '{}' has no items to deduct", slug));
            }

            let mut plan = Vec::new();
            for (sku, need_each) in items {
                let total_need = need_each * mult;
                let (part_id, have, cost): (i64, i64, f64) = tx
                    .query_row(
                        "SELECT id, qty, unit_cost FROM parts WHERE UPPER(sku) = UPPER(?1)",
                        params![sku],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                    )
                    .optional()?
                    .ok_or_else(|| anyhow!("BOM SKU missing from inventory: {}", sku))?;
                if have < total_need {
                    return Err(anyhow!(
                        "Insufficient stock for {} (need {}, have {})",
                        sku,
                        total_need,
                        have
                    ));
                }
                plan.push((part_id, sku, have, have - total_need, total_need, cost));
            }

            let note = if req.note.trim().is_empty() {
                format!("Deducted for BOM {} x{}", slug, mult)
            } else {
                req.note.trim().to_string()
            };

            for (part_id, _sku, old_qty, new_qty, total_need, cost) in plan {
                tx.execute(
                    "UPDATE parts SET qty = ?1, updated_at = ?2 WHERE id = ?3",
                    params![new_qty, now, part_id],
                )?;
                tx.execute(
                    "INSERT INTO stock_movements (part_id, delta, old_qty, new_qty, unit_cost, supplier_id, reason, project_slug, note, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, NULL, 'bom_deduct', ?6, ?7, ?8)",
                    params![part_id, -total_need, old_qty, new_qty, cost, slug, note, now],
                )?;
            }
            tx.commit()?;
        }
        self.append_audit("deduct_bom", serde_json::json!({"slug": slug, "multiplier": mult}));
        self.list_boms()?
            .into_iter()
            .find(|b| b.slug.eq_ignore_ascii_case(slug.trim()))
            .ok_or_else(|| anyhow!("BOM not found"))
    }

    pub fn compute_valuation(&self) -> Result<ValuationReport> {
        let parts = self.list_parts(None, None, None, None, false)?;
        let mut total_units = 0i64;
        let mut total_value = 0.0f64;
        let mut low_stock_skus = 0i64;
        let mut estimated_restock_cost = 0.0f64;

        let mut by_cat: BTreeMap<String, CategoryValuation> = BTreeMap::new();
        let mut by_loc: BTreeMap<String, LocationValuation> = BTreeMap::new();
        let mut by_sup: BTreeMap<String, SupplierValuation> = BTreeMap::new();

        for p in &parts {
            total_units += p.qty;
            total_value += p.extended_value;
            let restock_qty = if p.low_stock {
                low_stock_skus += 1;
                suggested_reorder_qty(p.qty, p.min_qty)
            } else {
                0
            };
            let restock_cost = round2(restock_qty as f64 * p.unit_cost);
            estimated_restock_cost += restock_cost;

            let cat_entry = by_cat.entry(p.category.clone()).or_insert(CategoryValuation {
                category: p.category.clone(),
                sku_count: 0,
                total_units: 0,
                total_value: 0.0,
                low_stock_count: 0,
            });
            cat_entry.sku_count += 1;
            cat_entry.total_units += p.qty;
            cat_entry.total_value = round2(cat_entry.total_value + p.extended_value);
            if p.low_stock {
                cat_entry.low_stock_count += 1;
            }

            let lcode = p.location_code.clone().unwrap_or_else(|| "UNASSIGNED".to_string());
            let lname = p.location_name.clone().unwrap_or_else(|| "Unassigned".to_string());
            let loc_entry = by_loc.entry(lcode.clone()).or_insert(LocationValuation {
                location_code: lcode,
                location_name: lname,
                sku_count: 0,
                total_units: 0,
                total_value: 0.0,
            });
            loc_entry.sku_count += 1;
            loc_entry.total_units += p.qty;
            loc_entry.total_value = round2(loc_entry.total_value + p.extended_value);

            let sname = p
                .primary_supplier_name
                .clone()
                .unwrap_or_else(|| "Unassigned".to_string());
            let sup_entry = by_sup.entry(sname.clone()).or_insert(SupplierValuation {
                supplier_name: sname,
                sku_count: 0,
                total_value: 0.0,
                low_stock_count: 0,
                reorder_est_cost: 0.0,
            });
            sup_entry.sku_count += 1;
            sup_entry.total_value = round2(sup_entry.total_value + p.extended_value);
            if p.low_stock {
                sup_entry.low_stock_count += 1;
                sup_entry.reorder_est_cost = round2(sup_entry.reorder_est_cost + restock_cost);
            }
        }

        Ok(ValuationReport {
            total_skus: parts.len() as i64,
            total_units,
            total_value: round2(total_value),
            low_stock_skus,
            estimated_restock_cost: round2(estimated_restock_cost),
            by_category: by_cat.into_values().collect(),
            by_location: by_loc.into_values().collect(),
            by_supplier: by_sup.into_values().collect(),
        })
    }

    pub fn compute_reorder_list(&self) -> Result<Vec<ReorderGroup>> {
        let low_parts = self.list_parts(None, None, None, None, true)?;
        let suppliers = self.list_suppliers()?;
        let sup_map: HashMap<String, Supplier> = suppliers
            .into_iter()
            .map(|s| (s.supplier.name.to_lowercase(), s.supplier))
            .collect();

        let mut grouped: BTreeMap<String, ReorderGroup> = BTreeMap::new();
        for p in low_parts {
            let sup_name = p
                .primary_supplier_name
                .clone()
                .unwrap_or_else(|| "Unassigned".to_string());
            let sup_info = sup_map.get(&sup_name.to_lowercase());
            let order_qty = suggested_reorder_qty(p.qty, p.min_qty);
            let est_cost = round2(order_qty as f64 * p.unit_cost);

            let grp = grouped.entry(sup_name.clone()).or_insert_with(|| ReorderGroup {
                supplier_id: sup_info.map(|s| s.id),
                supplier_name: sup_name.clone(),
                website: sup_info.map(|s| s.website.clone()).unwrap_or_default(),
                currency: sup_info.map(|s| s.currency.clone()).unwrap_or_else(|| "USD".to_string()),
                lead_time_days: sup_info.map(|s| s.lead_time_days).unwrap_or(0),
                group_total_cost: 0.0,
                items: Vec::new(),
            });
            grp.group_total_cost = round2(grp.group_total_cost + est_cost);
            grp.items.push(ReorderItem {
                sku: p.sku,
                part_name: p.name,
                category: p.category,
                qty: p.qty,
                min_qty: p.min_qty,
                suggested_order_qty: order_qty,
                unit_cost: p.unit_cost,
                est_order_cost: est_cost,
                location_code: p.location_code.unwrap_or_else(|| "-".to_string()),
                supplier_sku: p.supplier_sku,
                mpn: p.mpn,
                product_url: p.product_url,
            });
        }
        Ok(grouped.into_values().collect())
    }

    pub fn resolve_barcode(&self, raw_code: &str) -> Result<BarcodeResolveResult> {
        let trimmed = raw_code.trim();
        let parsed = parse_supplier_bag_payload(trimmed);
        let normalized = if let Some(rest) = trimmed.strip_prefix("SKU:").or_else(|| trimmed.strip_prefix("sku:")) {
            rest.trim().to_uppercase()
        } else if let Some(rest) = trimmed.strip_prefix("BIN:").or_else(|| trimmed.strip_prefix("bin:")) {
            normalize_location_code(rest)
        } else {
            trimmed.to_string()
        };

        // 1. Check explicit BIN: prefix or location barcode/code match
        if trimmed.to_ascii_uppercase().starts_with("BIN:") {
            if let Some(loc) = self.get_location(&normalized)? {
                return Ok(BarcodeResolveResult {
                    raw_code: trimmed.to_string(),
                    normalized_code: normalized,
                    match_kind: "location".to_string(),
                    part: None,
                    location: Some(loc),
                    parsed_mpn: None,
                    parsed_supplier_sku: None,
                    parsed_qty: None,
                });
            }
        }

        // 2. Direct part lookup by barcode_value, SKU, MPN, supplier_sku, or part_suppliers
        {
            let conn = self.conn.lock().unwrap();
            let direct_part = conn
                .query_row(
                    &format!(
                        "{PART_SELECT_SQL}
                         WHERE UPPER(p.barcode_value) = UPPER(?1)
                            OR UPPER(p.sku) = UPPER(?1)
                            OR UPPER(p.sku) = UPPER(?2)
                            OR (p.mpn <> '' AND UPPER(p.mpn) = UPPER(?1))
                            OR (p.supplier_sku <> '' AND UPPER(p.supplier_sku) = UPPER(?1))
                         LIMIT 1"
                    ),
                    params![trimmed, normalized],
                    map_part,
                )
                .optional()?;

            if let Some(part) = direct_part {
                return Ok(BarcodeResolveResult {
                    raw_code: trimmed.to_string(),
                    normalized_code: part.sku.clone(),
                    match_kind: "part".to_string(),
                    part: Some(part),
                    location: None,
                    parsed_mpn: parsed.mpn,
                    parsed_supplier_sku: parsed.supplier_sku,
                    parsed_qty: parsed.qty,
                });
            }

            // Check part_suppliers table by supplier_sku or mpn
            let via_ps = conn
                .query_row(
                    &format!(
                        "{PART_SELECT_SQL}
                         JOIN part_suppliers ps ON ps.part_id = p.id
                         WHERE (ps.supplier_sku <> '' AND UPPER(ps.supplier_sku) = UPPER(?1))
                            OR (ps.mpn <> '' AND UPPER(ps.mpn) = UPPER(?1))
                         LIMIT 1"
                    ),
                    params![trimmed],
                    map_part,
                )
                .optional()?;

            if let Some(part) = via_ps {
                return Ok(BarcodeResolveResult {
                    raw_code: trimmed.to_string(),
                    normalized_code: part.sku.clone(),
                    match_kind: "part".to_string(),
                    part: Some(part),
                    location: None,
                    parsed_mpn: parsed.mpn,
                    parsed_supplier_sku: parsed.supplier_sku,
                    parsed_qty: parsed.qty,
                });
            }

            // If supplier bag payload extracted MPN or supplier SKU, try resolving those!
            if parsed.mpn.is_some() || parsed.supplier_sku.is_some() {
                let mpn_q = parsed.mpn.as_deref().unwrap_or("");
                let ssku_q = parsed.supplier_sku.as_deref().unwrap_or("");
                let bag_part = conn
                    .query_row(
                        &format!(
                            "{PART_SELECT_SQL}
                             LEFT JOIN part_suppliers ps ON ps.part_id = p.id
                             WHERE (?1 <> '' AND (UPPER(p.mpn) = UPPER(?1) OR UPPER(ps.mpn) = UPPER(?1)))
                                OR (?2 <> '' AND (UPPER(p.supplier_sku) = UPPER(?2) OR UPPER(ps.supplier_sku) = UPPER(?2)))
                             LIMIT 1"
                        ),
                        params![mpn_q, ssku_q],
                        map_part,
                    )
                    .optional()?;

                if let Some(part) = bag_part {
                    return Ok(BarcodeResolveResult {
                        raw_code: trimmed.to_string(),
                        normalized_code: part.sku.clone(),
                        match_kind: "part".to_string(),
                        part: Some(part),
                        location: None,
                        parsed_mpn: parsed.mpn,
                        parsed_supplier_sku: parsed.supplier_sku,
                        parsed_qty: parsed.qty,
                    });
                }
            }
        }

        // 3. Check location barcode or code without BIN: prefix
        if let Some(loc) = self.get_location(trimmed)? {
            return Ok(BarcodeResolveResult {
                raw_code: trimmed.to_string(),
                normalized_code: loc.location.code.clone(),
                match_kind: "location".to_string(),
                part: None,
                location: Some(loc),
                parsed_mpn: None,
                parsed_supplier_sku: None,
                parsed_qty: None,
            });
        }

        let kind = if parsed.mpn.is_some() || parsed.supplier_sku.is_some() {
            "supplier_bag"
        } else {
            "unknown"
        };

        Ok(BarcodeResolveResult {
            raw_code: trimmed.to_string(),
            normalized_code: normalized,
            match_kind: kind.to_string(),
            part: None,
            location: None,
            parsed_mpn: parsed.mpn,
            parsed_supplier_sku: parsed.supplier_sku,
            parsed_qty: parsed.qty,
        })
    }

    pub fn migrate_from_markdown(&self, inventory_dir: &Path, projects_dir: &Path) -> Result<(usize, usize)> {
        let mut imported_parts = 0usize;
        let mut imported_boms = 0usize;

        if inventory_dir.is_dir() {
            let mut entries: Vec<PathBuf> = fs::read_dir(inventory_dir)?
                .filter_map(|e| e.ok().map(|x| x.path()))
                .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("md"))
                .collect();
            entries.sort();

            for md_path in entries {
                let stem = md_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("general")
                    .to_string();
                if matches!(stem.as_str(), "locations" | "suppliers" | "valuation") {
                    continue;
                }
                let content = fs::read_to_string(&md_path)?;
                let (headers, rows) = parse_markdown_table(&content);
                if headers.is_empty() {
                    continue;
                }
                let hdr_map: HashMap<String, usize> = headers
                    .iter()
                    .enumerate()
                    .map(|(i, h)| (h.trim().to_lowercase(), i))
                    .collect();

                for row in rows {
                    let get = |keys: &[&str]| -> String {
                        for k in keys {
                            if let Some(&idx) = hdr_map.get(*k) {
                                if let Some(val) = row.get(idx) {
                                    return val.trim().to_string();
                                }
                            }
                        }
                        String::new()
                    };
                    let sku = get(&["sku"]);
                    if sku.is_empty() {
                        continue;
                    }
                    // Skip if SKU already exists in DB so DB edits are never clobbered
                    if self.get_part(&sku)?.is_some() {
                        continue;
                    }
                    let part_name = get(&["part", "name"]);
                    if part_name.is_empty() {
                        continue;
                    }
                    let spec = get(&["value/spec", "spec", "value"]);
                    let package = get(&["package", "footprint"]);
                    let qty = parse_i64(&get(&["qty", "quantity"]));
                    let min_qty = parse_i64(&get(&["min", "min_qty"])).max(0);
                    let unit_cost = parse_f64(&get(&["unit cost", "cost", "unit_cost"]));
                    let loc = get(&["location", "bin/drawer", "bin"]);
                    let sup = get(&["supplier"]);
                    let mpn = get(&["mpn"]);
                    let projects = get(&["projects"]);
                    let link = get(&["link", "product_url"]);

                    let req = UpsertPart {
                        sku,
                        category: stem.clone(),
                        name: part_name,
                        spec,
                        package,
                        qty,
                        min_qty,
                        unit_cost,
                        currency: "USD".to_string(),
                        location_code: if loc.is_empty() || loc == "-" { None } else { Some(loc) },
                        supplier_name: if sup.is_empty() || sup == "-" { None } else { Some(sup) },
                        mpn,
                        supplier_sku: String::new(),
                        product_url: if link == "-" { String::new() } else { link },
                        datasheet_url: String::new(),
                        barcode_value: String::new(),
                        projects_tags: if projects.is_empty() { "-".to_string() } else { projects },
                        description: String::new(),
                        notes: String::new(),
                        image_path: String::new(),
                        pinout_image_path: String::new(),
                        image_url: String::new(),
                        pinout_image_url: String::new(),
                    };
                    self.upsert_part(&req)?;
                    imported_parts += 1;
                }
            }
        }

        if projects_dir.is_dir() {
            let sku_re = Regex::new(r"`([A-Za-z0-9_-]+)`\s*x(\d+)")?;
            let mut entries: Vec<PathBuf> = fs::read_dir(projects_dir)?
                .filter_map(|e| e.ok().map(|x| x.path()))
                .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("md"))
                .collect();
            entries.sort();

            let existing_boms: std::collections::HashSet<String> = self
                .list_boms()?
                .into_iter()
                .map(|b| b.slug)
                .collect();

            for proj_path in entries {
                let slug = proj_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                if slug.is_empty() || existing_boms.contains(&slug) {
                    continue;
                }
                let txt = fs::read_to_string(&proj_path)?;
                let mut counts: BTreeMap<String, i64> = BTreeMap::new();
                for cap in sku_re.captures_iter(&txt) {
                    let sku = cap[1].to_uppercase();
                    let n: i64 = cap[2].parse().unwrap_or(1);
                    *counts.entry(sku).or_insert(0) += n;
                }
                if counts.is_empty() {
                    continue;
                }
                let title = txt
                    .lines()
                    .find_map(|l| l.strip_prefix("# ").map(|t| t.trim().to_string()))
                    .unwrap_or_else(|| slug.clone());
                let items = counts
                    .into_iter()
                    .map(|(sku, qty_needed)| UpsertBomItem {
                        sku,
                        qty_needed,
                        notes: String::new(),
                    })
                    .collect();
                self.upsert_bom(&UpsertBom {
                    slug,
                    title,
                    status: "active".to_string(),
                    notes: String::new(),
                    items,
                })?;
                imported_boms += 1;
            }
        }

        Ok((imported_parts, imported_boms))
    }

    pub fn build_snapshot(&self) -> Result<Snapshot> {
        let locations = self
            .list_locations()?
            .into_iter()
            .map(|l| l.location)
            .collect();
        let suppliers = self
            .list_suppliers()?
            .into_iter()
            .map(|s| s.supplier)
            .collect();
        let parts = self.list_parts(None, None, None, None, false)?;
        let part_suppliers = self.list_part_suppliers_all()?;
        let boms = self
            .list_boms()?
            .into_iter()
            .map(|b| SnapshotBom {
                slug: b.slug,
                title: b.title,
                status: b.status,
                notes: b.notes,
                updated_at: b.updated_at,
                items: b
                    .items
                    .into_iter()
                    .map(|i| UpsertBomItem {
                        sku: i.sku,
                        qty_needed: i.need,
                        notes: i.notes,
                    })
                    .collect(),
            })
            .collect();

        Ok(Snapshot {
            schema_version: 2,
            exported_at: now_iso(),
            locations,
            suppliers,
            parts,
            part_suppliers,
            boms,
        })
    }

    pub fn import_snapshot(&self, snap: &Snapshot) -> Result<(usize, usize, usize, usize)> {
        let mut locs = 0usize;
        let mut sups = 0usize;
        let mut parts = 0usize;
        let mut boms = 0usize;

        for s in &snap.suppliers {
            self.upsert_supplier(&UpsertSupplier {
                name: s.name.clone(),
                website: s.website.clone(),
                account_url: s.account_url.clone(),
                currency: s.currency.clone(),
                lead_time_days: s.lead_time_days,
                shipping_notes: s.shipping_notes.clone(),
                notes: s.notes.clone(),
            })?;
            sups += 1;
        }
        for l in &snap.locations {
            self.upsert_location(&UpsertLocation {
                code: l.code.clone(),
                name: l.name.clone(),
                area: l.area.clone(),
                rack: l.rack.clone(),
                drawer: l.drawer.clone(),
                description: l.description.clone(),
                barcode_value: l.barcode_value.clone(),
            })?;
            locs += 1;
        }
        for p in &snap.parts {
            self.upsert_part(&UpsertPart {
                sku: p.sku.clone(),
                category: p.category.clone(),
                name: p.name.clone(),
                spec: p.spec.clone(),
                package: p.package.clone(),
                qty: p.qty,
                min_qty: p.min_qty,
                unit_cost: p.unit_cost,
                currency: p.currency.clone(),
                location_code: p.location_code.clone(),
                supplier_name: p.primary_supplier_name.clone(),
                mpn: p.mpn.clone(),
                supplier_sku: p.supplier_sku.clone(),
                product_url: p.product_url.clone(),
                datasheet_url: p.datasheet_url.clone(),
                barcode_value: p.barcode_value.clone(),
                projects_tags: p.projects_tags.clone(),
                description: p.description.clone(),
                notes: p.notes.clone(),
                image_path: p.image_path.clone(),
                pinout_image_path: p.pinout_image_path.clone(),
                image_url: String::new(),
                pinout_image_url: String::new(),
            })?;
            parts += 1;
        }
        for b in &snap.boms {
            self.upsert_bom(&UpsertBom {
                slug: b.slug.clone(),
                title: b.title.clone(),
                status: b.status.clone(),
                notes: b.notes.clone(),
                items: b.items.clone(),
            })?;
            boms += 1;
        }
        Ok((locs, sups, parts, boms))
    }
}

const PART_SELECT_SQL: &str = "
    SELECT p.id, p.sku, p.category, p.name, p.spec, p.package, p.qty, p.min_qty,
           p.unit_cost, p.currency, p.location_id, l.code, l.name,
           p.primary_supplier_id, s.name, p.mpn, p.supplier_sku, p.product_url,
           p.datasheet_url, p.barcode_value, p.projects_tags, p.description, p.notes,
           p.image_path, p.pinout_image_path,
           p.created_at, p.updated_at
    FROM parts p
    LEFT JOIN locations l ON l.id = p.location_id
    LEFT JOIN suppliers s ON s.id = p.primary_supplier_id
";

fn migrate_parts_image_columns(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(parts)")?;
    let cols: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .filter_map(|r| r.ok())
        .collect();
    if !cols.iter().any(|c| c == "description") {
        conn.execute("ALTER TABLE parts ADD COLUMN description TEXT NOT NULL DEFAULT ''", [])?;
    }
    if !cols.iter().any(|c| c == "image_path") {
        conn.execute("ALTER TABLE parts ADD COLUMN image_path TEXT NOT NULL DEFAULT ''", [])?;
    }
    if !cols.iter().any(|c| c == "pinout_image_path") {
        conn.execute("ALTER TABLE parts ADD COLUMN pinout_image_path TEXT NOT NULL DEFAULT ''", [])?;
    }
    Ok(())
}

fn configure_connection(conn: &Connection) -> Result<()> {
    // Cloud-sync safety: DELETE journal mode eliminates lingering -wal/-shm sidecars.
    conn.execute_batch(
        "PRAGMA journal_mode = DELETE;
         PRAGMA synchronous = FULL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;",
    )?;
    Ok(())
}

fn get_part_inner(conn: &Connection, sku: &str) -> Result<Option<Part>> {
    conn.query_row(
        &format!("{PART_SELECT_SQL} WHERE UPPER(p.sku) = UPPER(?1)"),
        params![sku.trim()],
        map_part,
    )
    .optional()
    .map_err(Into::into)
}

fn ensure_location_id(conn: &Connection, raw_loc: &str, now: &str) -> Result<i64> {
    let norm = normalize_location_code(raw_loc);
    if let Some(id) = conn
        .query_row(
            "SELECT id FROM locations WHERE UPPER(code) = UPPER(?1) OR UPPER(code) = UPPER(?2)",
            params![raw_loc.trim(), norm],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
    {
        return Ok(id);
    }

    let (area, drawer) = infer_area_drawer(raw_loc.trim());
    let barcode = format!("BIN:{}", norm);
    conn.execute(
        "INSERT INTO locations (code, name, area, rack, drawer, description, barcode_value, created_at, updated_at)
         VALUES (?1, ?2, ?3, '', ?4, '', ?5, ?6, ?6)",
        params![norm, raw_loc.trim(), area, drawer, barcode, now],
    )?;
    Ok(conn.last_insert_rowid())
}

fn ensure_supplier_id(conn: &Connection, name: &str, now: &str) -> Result<i64> {
    let trimmed = name.trim();
    if let Some(id) = conn
        .query_row(
            "SELECT id FROM suppliers WHERE LOWER(name) = LOWER(?1)",
            params![trimmed],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    conn.execute(
        "INSERT INTO suppliers (name, website, account_url, currency, lead_time_days, shipping_notes, notes, created_at, updated_at)
         VALUES (?1, '', '', 'USD', 5, '', '', ?2, ?2)",
        params![trimmed, now],
    )?;
    Ok(conn.last_insert_rowid())
}

fn upsert_part_supplier_inner(
    conn: &Connection,
    part_id: i64,
    supplier_id: i64,
    supplier_sku: &str,
    mpn: &str,
    unit_cost: f64,
    moq: i64,
    product_url: &str,
    is_preferred: bool,
) -> Result<()> {
    if is_preferred {
        conn.execute(
            "UPDATE part_suppliers SET is_preferred = 0 WHERE part_id = ?1",
            params![part_id],
        )?;
    }
    conn.execute(
        "INSERT INTO part_suppliers (part_id, supplier_id, supplier_sku, mpn, unit_cost, moq, product_url, last_purchased_at, is_preferred)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, '', ?8)
         ON CONFLICT(part_id, supplier_id, supplier_sku) DO UPDATE SET
            mpn = excluded.mpn,
            unit_cost = excluded.unit_cost,
            moq = excluded.moq,
            product_url = excluded.product_url,
            is_preferred = excluded.is_preferred",
        params![
            part_id,
            supplier_id,
            supplier_sku,
            mpn,
            unit_cost,
            moq,
            product_url,
            if is_preferred { 1 } else { 0 }
        ],
    )?;
    Ok(())
}

fn map_location_with_stats(r: &rusqlite::Row<'_>) -> rusqlite::Result<LocationWithStats> {
    Ok(LocationWithStats {
        location: Location {
            id: r.get(0)?,
            code: r.get(1)?,
            name: r.get(2)?,
            area: r.get(3)?,
            rack: r.get(4)?,
            drawer: r.get(5)?,
            description: r.get(6)?,
            barcode_value: r.get(7)?,
            created_at: r.get(8)?,
            updated_at: r.get(9)?,
        },
        part_count: r.get(10)?,
        total_qty: r.get(11)?,
        total_value: round2(r.get::<_, f64>(12)?),
    })
}

fn map_supplier_with_stats(r: &rusqlite::Row<'_>) -> rusqlite::Result<SupplierWithStats> {
    Ok(SupplierWithStats {
        supplier: Supplier {
            id: r.get(0)?,
            name: r.get(1)?,
            website: r.get(2)?,
            account_url: r.get(3)?,
            currency: r.get(4)?,
            lead_time_days: r.get(5)?,
            shipping_notes: r.get(6)?,
            notes: r.get(7)?,
            created_at: r.get(8)?,
            updated_at: r.get(9)?,
        },
        part_count: r.get(10)?,
        total_value: round2(r.get::<_, f64>(11)?),
    })
}

fn map_part(r: &rusqlite::Row<'_>) -> rusqlite::Result<Part> {
    let qty: i64 = r.get(6)?;
    let min_qty: i64 = r.get(7)?;
    let unit_cost: f64 = r.get(8)?;
    Ok(Part {
        id: r.get(0)?,
        sku: r.get(1)?,
        category: r.get(2)?,
        name: r.get(3)?,
        spec: r.get(4)?,
        package: r.get(5)?,
        qty,
        min_qty,
        unit_cost: round2(unit_cost),
        currency: r.get(9)?,
        location_id: r.get(10)?,
        location_code: r.get(11)?,
        location_name: r.get(12)?,
        primary_supplier_id: r.get(13)?,
        primary_supplier_name: r.get(14)?,
        mpn: r.get(15)?,
        supplier_sku: r.get(16)?,
        product_url: r.get(17)?,
        datasheet_url: r.get(18)?,
        barcode_value: r.get(19)?,
        projects_tags: r.get(20)?,
        description: r.get(21)?,
        notes: r.get(22)?,
        image_path: r.get(23)?,
        pinout_image_path: r.get(24)?,
        created_at: r.get(25)?,
        updated_at: r.get(26)?,
        extended_value: round2(qty as f64 * unit_cost),
        low_stock: qty <= min_qty,
    })
}

fn map_part_supplier(r: &rusqlite::Row<'_>) -> rusqlite::Result<PartSupplier> {
    let pref: i64 = r.get(10)?;
    Ok(PartSupplier {
        id: r.get(0)?,
        part_id: r.get(1)?,
        supplier_id: r.get(2)?,
        supplier_name: r.get(3)?,
        supplier_sku: r.get(4)?,
        mpn: r.get(5)?,
        unit_cost: round2(r.get(6)?),
        moq: r.get(7)?,
        product_url: r.get(8)?,
        last_purchased_at: r.get(9)?,
        is_preferred: pref != 0,
    })
}

fn map_movement(r: &rusqlite::Row<'_>) -> rusqlite::Result<StockMovement> {
    Ok(StockMovement {
        id: r.get(0)?,
        part_id: r.get(1)?,
        sku: r.get(2)?,
        part_name: r.get(3)?,
        delta: r.get(4)?,
        old_qty: r.get(5)?,
        new_qty: r.get(6)?,
        unit_cost: r.get(7)?,
        supplier_id: r.get(8)?,
        supplier_name: r.get(9)?,
        reason: r.get(10)?,
        project_slug: r.get(11)?,
        note: r.get(12)?,
        created_at: r.get(13)?,
    })
}

pub fn normalize_location_code(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let upper = trimmed.to_uppercase();
    if let Some(rest) = upper.strip_prefix("DRAWER ") {
        return format!("DRW-{}", slugify_token(rest));
    }
    if let Some(rest) = upper.strip_prefix("BIN ") {
        return format!("BIN-{}", slugify_token(rest));
    }
    slugify_token(&upper)
}

fn slugify_token(s: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in s.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_uppercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_end_matches('-').to_string()
}

fn infer_area_drawer(raw: &str) -> (String, String) {
    let lower = raw.to_lowercase();
    if lower.starts_with("drawer") {
        let d = raw.split_whitespace().skip(1).collect::<Vec<_>>().join(" ");
        ("Bench Drawers".to_string(), d)
    } else if lower.starts_with("bin") {
        let d = raw.split_whitespace().skip(1).collect::<Vec<_>>().join(" ");
        ("Parts Rack".to_string(), d)
    } else {
        ("Workshop".to_string(), raw.to_string())
    }
}

pub fn normalize_category(raw: &str) -> String {
    let clean = raw
        .trim()
        .trim_end_matches(".md")
        .to_lowercase()
        .replace(' ', "-")
        .replace('_', "-");
    if clean.is_empty() {
        "hardware".to_string()
    } else {
        clean
    }
}

pub fn suggested_reorder_qty(qty: i64, min_qty: i64) -> i64 {
    let target = (min_qty * 2).max(min_qty + 1).max(1);
    (target - qty).max(1)
}

pub fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn parse_markdown_table(content: &str) -> (Vec<String>, Vec<Vec<String>>) {
    let mut headers = Vec::new();
    let mut rows = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') {
            continue;
        }
        let inner = trimmed.trim_matches('|');
        let cells: Vec<String> = inner.split('|').map(|c| c.trim().to_string()).collect();
        if headers.is_empty() {
            headers = cells;
        } else if cells
            .iter()
            .all(|c| !c.is_empty() && c.chars().all(|ch| matches!(ch, '-' | ':' | ' ')))
        {
            continue;
        } else {
            rows.push(cells);
        }
    }
    (headers, rows)
}

fn parse_i64(s: &str) -> i64 {
    let cleaned: String = s
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '-')
        .collect();
    cleaned.parse().unwrap_or(0)
}

fn parse_f64(s: &str) -> f64 {
    let cleaned: String = s
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    cleaned.parse().unwrap_or(0.0)
}

struct ParsedBag {
    mpn: Option<String>,
    supplier_sku: Option<String>,
    qty: Option<i64>,
}

fn parse_supplier_bag_payload(raw: &str) -> ParsedBag {
    // Handles DigiKey / Mouser ANSI MH10.8.2 / ISO 15434 2D DataMatrix/QR payloads (`[)>\x1e06\x1d1P...\x1dP...\x1dQ...`)
    // and LCSC JSON-like `{pbn:...,pc:C12345,pm:...,qty:10}` strings.
    let mut mpn = None;
    let mut supplier_sku = None;
    let mut qty = None;

    if raw.contains("[)>") || raw.contains('\x1d') || raw.contains('\x1e') {
        for field in raw.split(|c| c == '\x1d' || c == '\x1e' || c == '\r' || c == '\n') {
            let f = field.trim();
            if let Some(v) = f.strip_prefix("1P") {
                if !v.is_empty() {
                    mpn = Some(v.to_string());
                }
            } else if let Some(v) = f.strip_prefix("30P") {
                if supplier_sku.is_none() && !v.is_empty() {
                    supplier_sku = Some(v.to_string());
                }
            } else if let Some(v) = f.strip_prefix('P') {
                if !v.is_empty() && supplier_sku.is_none() {
                    supplier_sku = Some(v.to_string());
                }
            } else if let Some(v) = f.strip_prefix('Q') {
                if let Ok(q) = v.parse::<i64>() {
                    qty = Some(q);
                }
            }
        }
    } else if raw.starts_with('{') && raw.contains("pc:") {
        for part in raw.trim_matches(|c| c == '{' || c == '}').split(',') {
            if let Some((k, v)) = part.split_once(':') {
                let k = k.trim().trim_matches('"');
                let v = v.trim().trim_matches('"');
                match k {
                    "pc" if !v.is_empty() => supplier_sku = Some(v.to_string()),
                    "pm" if !v.is_empty() => mpn = Some(v.to_string()),
                    "qty" => {
                        if let Ok(q) = v.parse::<i64>() {
                            qty = Some(q);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    ParsedBag {
        mpn,
        supplier_sku,
        qty,
    }
}
