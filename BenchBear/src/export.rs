use anyhow::Result;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::config::AppConfig;
use crate::db::{round2, Database};
use crate::models::Part;

pub fn sync_all(db: &Database, cfg: &AppConfig) -> Result<()> {
    write_snapshot(db, &cfg.data_dir)?;
    export_markdown_targets(db, cfg, true)?;
    Ok(())
}

pub fn sync_portable(db: &Database, cfg: &AppConfig) -> Result<()> {
    write_snapshot(db, &cfg.data_dir)?;
    export_markdown_targets(db, cfg, false)?;
    Ok(())
}

pub fn write_snapshot(db: &Database, data_dir: &Path) -> Result<()> {
    fs::create_dir_all(data_dir)?;
    let snap = db.build_snapshot()?;
    let json_bytes = serde_json::to_vec_pretty(&snap)?;
    let target = data_dir.join("snapshot.json");
    let tmp = data_dir.join(".snapshot.json.tmp");
    fs::write(&tmp, json_bytes)?;
    fs::rename(&tmp, &target)?;
    Ok(())
}

pub fn export_markdown(db: &Database, cfg: &AppConfig) -> Result<()> {
    export_markdown_targets(db, cfg, true)
}

fn export_markdown_targets(db: &Database, cfg: &AppConfig, include_docs: bool) -> Result<()> {
    let parts = db.list_parts(None, None, None, None, false)?;
    let locations = db.list_locations()?;
    let suppliers = db.list_suppliers()?;
    let valuation = db.compute_valuation()?;
    let reorder = db.compute_reorder_list()?;

    let docs_inv = cfg.docs_dir.join("inventory");
    let mut targets: Vec<&Path> = vec![cfg.inventory_dir.as_path()];
    if include_docs {
        targets.push(docs_inv.as_path());
    }
    for dir in &targets {
        fs::create_dir_all(dir)?;
    }

    // 1. Group parts by category
    let mut by_cat: BTreeMap<String, Vec<Part>> = BTreeMap::new();
    for p in parts {
        by_cat.entry(p.category.clone()).or_default().push(p);
    }

    for (cat, items) in &by_cat {
        let title = title_case_slug(cat);
        let mut md = format!(
            "---\ncategory: {cat}\nschema_version: 2\n---\n# {title}\n\n\
             | SKU | Part | Value/Spec | Package | Qty | Min | Unit Cost | Value | Location | Supplier | MPN | Projects | Link |\n\
             | --- | ---- | ---------- | ------- | --- | --- | --------- | ----- | -------- | -------- | --- | -------- | ---- |\n"
        );
        for p in items {
            let loc = p.location_code.as_deref().unwrap_or("-");
            let sup = p.primary_supplier_name.as_deref().unwrap_or("-");
            let mpn = if p.mpn.is_empty() { "-" } else { p.mpn.as_str() };
            let link = if p.product_url.is_empty() {
                "-".to_string()
            } else {
                format!("[Link]({})", p.product_url)
            };
            md.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | ${:.2} | ${:.2} | {} | {} | {} | {} | {} |\n",
                esc_cell(&p.sku),
                esc_cell(&p.name),
                esc_cell(&p.spec),
                esc_cell(&p.package),
                p.qty,
                p.min_qty,
                p.unit_cost,
                p.extended_value,
                esc_cell(loc),
                esc_cell(sup),
                esc_cell(mpn),
                esc_cell(&p.projects_tags),
                link
            ));
        }
        let detailed_items: Vec<&Part> = items
            .iter()
            .filter(|p| !p.description.trim().is_empty() || !p.notes.trim().is_empty())
            .collect();
        if !detailed_items.is_empty() {
            md.push_str("\n## Part Details & Descriptions\n\n");
            for p in detailed_items {
                md.push_str(&format!("### {} — {}\n\n", p.sku, p.name));
                if !p.description.trim().is_empty() {
                    md.push_str(p.description.trim());
                    md.push_str("\n\n");
                }
                if !p.notes.trim().is_empty() {
                    md.push_str(&format!("**Notes:** {}\n\n", p.notes.trim()));
                }
            }
        }
        let fname = format!("{}.md", cat);
        for dir in &targets {
            write_if_changed(&dir.join(&fname), &md)?;
        }
    }

    // 2. locations.md
    let mut loc_md = String::from(
        "---\ntitle: Locations & Bins\n---\n# Locations & Bins\n\n\
         | Code | Name | Area | Rack | Drawer | Barcode | SKUs | Units | Value |\n\
         | ---- | ---- | ---- | ---- | ------ | ------- | ---- | ----- | ----- |\n",
    );
    for l in &locations {
        loc_md.push_str(&format!(
            "| `{}` | {} | {} | {} | {} | `{}` | {} | {} | ${:.2} |\n",
            esc_cell(&l.location.code),
            esc_cell(&l.location.name),
            esc_cell(if l.location.area.is_empty() { "-" } else { &l.location.area }),
            esc_cell(if l.location.rack.is_empty() { "-" } else { &l.location.rack }),
            esc_cell(if l.location.drawer.is_empty() { "-" } else { &l.location.drawer }),
            esc_cell(&l.location.barcode_value),
            l.part_count,
            l.total_qty,
            l.total_value
        ));
    }
    for dir in &targets {
        write_if_changed(&dir.join("locations.md"), &loc_md)?;
    }

    // 3. suppliers.md
    let mut sup_md = String::from(
        "---\ntitle: Suppliers & Vendors\n---\n# Suppliers & Vendors\n\n\
         | Supplier | Currency | Lead Time | Linked SKUs | Stock Value | Website | Shipping Notes |\n\
         | -------- | -------- | --------- | ----------- | ----------- | ------- | -------------- |\n",
    );
    for s in &suppliers {
        let web = if s.supplier.website.is_empty() {
            "-".to_string()
        } else {
            format!("[Site]({})", s.supplier.website)
        };
        sup_md.push_str(&format!(
            "| {} | {} | {}d | {} | ${:.2} | {} | {} |\n",
            esc_cell(&s.supplier.name),
            esc_cell(&s.supplier.currency),
            s.supplier.lead_time_days,
            s.part_count,
            s.total_value,
            web,
            esc_cell(if s.supplier.shipping_notes.is_empty() { "-" } else { &s.supplier.shipping_notes }),
        ));
    }
    for dir in &targets {
        write_if_changed(&dir.join("suppliers.md"), &sup_md)?;
    }

    // 4. valuation.md
    let mut val_md = format!(
        "---\ntitle: Valuation & Reorder\n---\n# Inventory Valuation & Reorder Summary\n\n\
         - **Total SKUs:** {}\n\
         - **Total Units:** {}\n\
         - **Total Inventory Value:** ${:.2}\n\
         - **Low-Stock SKUs:** {}\n\
         - **Estimated Restock Cost:** ${:.2}\n\n\
         ## Value by Category\n\n\
         | Category | SKUs | Units | Value | Low Stock |\n\
         | -------- | ---- | ----- | ----- | --------- |\n",
        valuation.total_skus,
        valuation.total_units,
        valuation.total_value,
        valuation.low_stock_skus,
        valuation.estimated_restock_cost
    );
    for c in &valuation.by_category {
        val_md.push_str(&format!(
            "| {} | {} | {} | ${:.2} | {} |\n",
            esc_cell(&c.category),
            c.sku_count,
            c.total_units,
            round2(c.total_value),
            c.low_stock_count
        ));
    }
    val_md.push_str("\n## Low-Stock Reorder Sheet\n\n");
    if reorder.is_empty() {
        val_md.push_str("All tracked parts are above minimum stock levels.\n");
    } else {
        for g in &reorder {
            val_md.push_str(&format!(
                "### {} (Est. Order Total: ${:.2})\n\n\
                 | SKU | Part | Have | Min | Order Qty | Unit Cost | Est. Cost | Bin | Supplier SKU | MPN |\n\
                 | --- | ---- | ---- | --- | --------- | --------- | --------- | --- | ------------ | --- |\n",
                g.supplier_name, g.group_total_cost
            ));
            for item in &g.items {
                val_md.push_str(&format!(
                    "| `{}` | {} | {} | {} | {} | ${:.2} | ${:.2} | {} | {} | {} |\n",
                    esc_cell(&item.sku),
                    esc_cell(&item.part_name),
                    item.qty,
                    item.min_qty,
                    item.suggested_order_qty,
                    item.unit_cost,
                    item.est_order_cost,
                    esc_cell(&item.location_code),
                    esc_cell(if item.supplier_sku.is_empty() { "-" } else { &item.supplier_sku }),
                    esc_cell(if item.mpn.is_empty() { "-" } else { &item.mpn }),
                ));
            }
            val_md.push('\n');
        }
    }
    for dir in &targets {
        write_if_changed(&dir.join("valuation.md"), &val_md)?;
    }

    // 5. Mirror projects/*.md into docs/projects/ without symlinks
    if include_docs {
        let docs_proj = cfg.docs_dir.join("projects");
        if cfg.projects_dir.is_dir() {
            fs::create_dir_all(&docs_proj)?;
            for entry in fs::read_dir(&cfg.projects_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("md") {
                    if let Some(name) = path.file_name() {
                        let content = fs::read_to_string(&path)?;
                        write_if_changed(&docs_proj.join(name), &content)?;
                    }
                }
            }
        }
    }

    Ok(())
}

fn write_if_changed(path: &Path, content: &str) -> Result<()> {
    if let Ok(existing) = fs::read_to_string(path) {
        if existing == content {
            return Ok(());
        }
    }
    fs::write(path, content)?;
    Ok(())
}

fn esc_cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

fn title_case_slug(s: &str) -> String {
    s.split(['-', '_'])
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut chars = p.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
