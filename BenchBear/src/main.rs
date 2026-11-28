mod barcodes;
mod config;
mod db;
mod export;
mod mcp;
mod models;
mod scrape;
mod ui;
mod update;
mod web;

use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use crate::barcodes::decode::decode_image_bytes;
use crate::barcodes::labels::{
    build_bin_label, build_part_label, render_label_svg, BarcodeStyle, LabelPreset,
};
use crate::barcodes::pdf::render_labels_pdf;
use crate::config::AppConfig;
use crate::db::Database;
use crate::models::Snapshot;

#[derive(Parser)]
#[command(name = "benchbear", version, about = "BenchBear - BenchBear, Inventory, Locations, Suppliers & Barcode Engine")]
struct Cli {
    #[arg(long, global = true, help = "Path to wiki root directory")]
    wiki_root: Option<PathBuf>,

    #[arg(long, global = true, help = "Path to portable data directory (SQLite DB & snapshots)")]
    data_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the Inventory, Locations, Suppliers & Barcode Web Server
    Serve {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long)]
        port: Option<u16>,
    },
    /// Run the JSON-RPC 2.0 stdio MCP Server
    Mcp,
    /// Export snapshot.json and/or Markdown tables
    Export {
        #[arg(long, default_value = "all")]
        format: String,
    },
    /// Import/restore from a portable snapshot JSON file
    ImportSnapshot {
        path: PathBuf,
    },
    /// Import existing inventory/*.md and projects/*.md tables into SQLite
    MigrateMd,
    /// Generate a thermal SVG or vector PDF barcode label
    Label {
        #[arg(long, default_value = "part")]
        kind: String,
        #[arg(long)]
        id: String,
        #[arg(long, default_value = "combo")]
        barcode: String,
        #[arg(long, default_value = "2x1")]
        preset: String,
        #[arg(long, default_value = "svg")]
        format: String,
        #[arg(long)]
        out: PathBuf,
    },
    /// Decode 1D/2D barcodes from an image file and resolve part/bin
    Scan {
        image_path: PathBuf,
    },
    /// Fetch and extract part metadata from a product URL
    FetchUrl {
        url: String,
    },
    /// Check GitHub for newer BenchBear releases
    CheckUpdate {
        #[arg(long)]
        json: bool,
    },
}

fn open_and_sync_db(cfg: &AppConfig) -> Result<Database> {
    let audit_path = cfg.data_dir.join("audit.jsonl");
    let db = Database::open(&cfg.db_path, Some(&audit_path))?;
    let (parts, _, _, _) = db.counts()?;
    if parts == 0 {
        let (imp_parts, imp_boms) = db.migrate_from_markdown(&cfg.inventory_dir, &cfg.projects_dir)?;
        if imp_parts > 0 || imp_boms > 0 {
            eprintln!(
                ":: [BenchBear] Imported {} parts and {} BOMs from Markdown into {}",
                imp_parts,
                imp_boms,
                cfg.db_path.display()
            );
        }
    }
    Ok(db)
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let port_override = match &cli.command {
        Commands::Serve { port, .. } => *port,
        _ => None,
    };
    let cfg = AppConfig::load(
        cli.wiki_root.as_deref(),
        cli.data_dir.as_deref(),
        port_override,
    );

    match cli.command {
        Commands::Serve { host, .. } => {
            let db = Arc::new(open_and_sync_db(&cfg)?);
            export::sync_all(&db, &cfg)?;
            web::run_server(host, cfg.port_inventory, cfg, db).await?;
        }
        Commands::Mcp => {
            let db = open_and_sync_db(&cfg)?;
            mcp::run_mcp_stdio(&cfg, &db)?;
        }
        Commands::Export { format } => {
            let db = open_and_sync_db(&cfg)?;
            match format.as_str() {
                "snapshot" => export::write_snapshot(&db, &cfg.data_dir)?,
                "markdown" => export::export_markdown(&db, &cfg)?,
                _ => export::sync_all(&db, &cfg)?,
            }
            println!(":: [BenchBear] Export ({}) complete.", format);
        }
        Commands::ImportSnapshot { path } => {
            let db = open_and_sync_db(&cfg)?;
            let raw = fs::read_to_string(&path)?;
            let snap: Snapshot = serde_json::from_str(&raw)?;
            let (locs, sups, parts, boms) = db.import_snapshot(&snap)?;
            export::sync_all(&db, &cfg)?;
            println!(
                ":: [BenchBear] Snapshot imported: {} locations, {} suppliers, {} parts, {} BOMs.",
                locs, sups, parts, boms
            );
        }
        Commands::MigrateMd => {
            let audit_path = cfg.data_dir.join("audit.jsonl");
            let db = Database::open(&cfg.db_path, Some(&audit_path))?;
            let (p, b) = db.migrate_from_markdown(&cfg.inventory_dir, &cfg.projects_dir)?;
            export::sync_all(&db, &cfg)?;
            println!(":: [BenchBear] Migrated {} parts and {} BOMs from Markdown.", p, b);
        }
        Commands::Label {
            kind,
            id,
            barcode,
            preset,
            format,
            out,
        } => {
            let db = open_and_sync_db(&cfg)?;
            let style = BarcodeStyle::from_str(&barcode);
            let pset = LabelPreset::from_str(&preset);
            let spec = if kind.eq_ignore_ascii_case("bin") || kind.eq_ignore_ascii_case("location") {
                let loc = db
                    .get_location(&id)?
                    .ok_or_else(|| anyhow!("Location not found: {}", id))?;
                let parts = db.list_parts(None, None, Some(&loc.location.code), None, false)?;
                build_bin_label(&loc, &parts, pset, style)?
            } else {
                let part = db
                    .get_part(&id)?
                    .ok_or_else(|| anyhow!("SKU not found: {}", id))?;
                build_part_label(&part, pset, style)?
            };
            if let Some(parent) = out.parent() {
                if !parent.as_os_str().is_empty() {
                    fs::create_dir_all(parent)?;
                }
            }
            if format.eq_ignore_ascii_case("pdf") {
                fs::write(&out, render_labels_pdf(&[spec])?)?;
            } else {
                fs::write(&out, render_label_svg(&spec))?;
            }
            println!(":: [BenchBear] Wrote label to {}", out.display());
        }
        Commands::Scan { image_path } => {
            let db = open_and_sync_db(&cfg)?;
            let bytes = fs::read(&image_path)?;
            let decoded = decode_image_bytes(&bytes, &db)?;
            println!("{}", serde_json::to_string_pretty(&decoded)?);
        }
        Commands::FetchUrl { url } => {
            let db = open_and_sync_db(&cfg)?;
            let draft = scrape::fetch_product_url(&url, &db).await?;
            println!("{}", serde_json::to_string_pretty(&draft)?);
        }
        Commands::CheckUpdate { json } => {
            let res = update::check_github_updates(&cfg.wiki_root).await;
            if json {
                println!("{}", serde_json::to_string_pretty(&res)?);
            } else {
                println!(
                    ":: [BenchBear] Repo: {} | Current: v{}{}",
                    res.repo,
                    res.current_version,
                    if res.local_commit.is_empty() {
                        String::new()
                    } else {
                        format!(" ({})", res.local_commit)
                    }
                );
                if let Some(err) = &res.error {
                    println!(":: [BenchBear] Update check failed: {}", err);
                    println!(":: [BenchBear] Releases: {}", res.release_url);
                } else if res.update_available {
                    println!(
                        ":: [BenchBear] Update available: {} (current v{})",
                        res.latest_tag, res.current_version
                    );
                    println!(":: [BenchBear] Release URL: {}", res.release_url);
                    println!(":: [BenchBear] Run `{}` to pull, rebuild, and restart.", res.update_command);
                } else {
                    let latest_lbl = if res.latest_tag.is_empty() {
                        format!("v{}", res.current_version)
                    } else {
                        res.latest_tag.clone()
                    };
                    println!(":: [BenchBear] Up to date (latest: {}).", latest_lbl);
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::*;

    #[test]
    fn db_crud_locations_suppliers_costs_and_boms() {
        let db = Database::open_in_memory().unwrap();

        let loc = db
            .upsert_location(&UpsertLocation {
                code: "BIN-A1".to_string(),
                name: "Bin A1".to_string(),
                area: "Rack 1".to_string(),
                rack: "R1".to_string(),
                drawer: "A1".to_string(),
                description: "ESP32 modules".to_string(),
                barcode_value: "".to_string(),
            })
            .unwrap();
        assert_eq!(loc.location.barcode_value, "BIN:BIN-A1");

        let part = db
            .upsert_part(&UpsertPart {
                sku: "MCU-001".to_string(),
                category: "microcontrollers".to_string(),
                name: "ESP32-S3-WROOM-1".to_string(),
                spec: "Dual-core 240MHz".to_string(),
                package: "SMD".to_string(),
                qty: 10,
                min_qty: 3,
                unit_cost: 4.50,
                currency: "USD".to_string(),
                location_code: Some("BIN-A1".to_string()),
                supplier_name: Some("DigiKey".to_string()),
                mpn: "ESP32-S3-WROOM-1-N8R8".to_string(),
                supplier_sku: "1965-ESP32S3-ND".to_string(),
                product_url: "https://www.digikey.com".to_string(),
                datasheet_url: String::new(),
                barcode_value: String::new(),
                projects_tags: "rover".to_string(),
                description: "Dual-core 240MHz WiFi BLE MCU with 8MB PSRAM and USB-OTG".to_string(),
                notes: String::new(),
                image_path: String::new(),
                pinout_image_path: String::new(),
                image_url: String::new(),
                pinout_image_url: String::new(),
            })
            .unwrap();
        assert_eq!(part.extended_value, 45.0);
        assert_eq!(part.location_code.as_deref(), Some("BIN-A1"));
        assert_eq!(part.primary_supplier_name.as_deref(), Some("DigiKey"));

        // Adjust stock down to trigger low stock
        let low = db
            .adjust_stock(
                "MCU-001",
                &AdjustStockReq {
                    delta: -8,
                    reason: "build".to_string(),
                    note: "Rover build".to_string(),
                    unit_cost: None,
                    supplier_id: None,
                    project_slug: Some("rover".to_string()),
                },
            )
            .unwrap();
        assert_eq!(low.qty, 2);
        assert!(low.low_stock);

        // Check valuation and reorder grouping
        let val = db.compute_valuation().unwrap();
        assert_eq!(val.total_skus, 1);
        assert_eq!(val.total_units, 2);
        assert_eq!(val.total_value, 9.0);
        assert_eq!(val.low_stock_skus, 1);

        let reorder = db.compute_reorder_list().unwrap();
        assert_eq!(reorder.len(), 1);
        assert_eq!(reorder[0].supplier_name, "DigiKey");
        assert_eq!(reorder[0].items[0].suggested_order_qty, 4); // target 2*3=6, have 2 -> order 4

        // Resolve barcodes (SKU, BIN, and DigiKey MH10.8.2 payload)
        let r1 = db.resolve_barcode("SKU:MCU-001").unwrap();
        assert_eq!(r1.match_kind, "part");
        let r2 = db.resolve_barcode("BIN:BIN-A1").unwrap();
        assert_eq!(r2.match_kind, "location");
        let r3 = db
            .resolve_barcode("[)>\x1e06\x1dP1965-ESP32S3-ND\x1d1PESP32-S3-WROOM-1-N8R8\x1dQ10\x1e\x04")
            .unwrap();
        assert_eq!(r3.match_kind, "part");
        assert_eq!(r3.parsed_qty, Some(10));

        // BOM create + deduct
        let bom = db
            .upsert_bom(&UpsertBom {
                slug: "rover".to_string(),
                title: "Rover".to_string(),
                status: "active".to_string(),
                notes: String::new(),
                items: vec![UpsertBomItem {
                    sku: "MCU-001".to_string(),
                    qty_needed: 2,
                    notes: String::new(),
                }],
            })
            .unwrap();
        assert!(bom.ready);
        let after = db
            .deduct_bom(
                "rover",
                &DeductBomReq {
                    multiplier: 1,
                    note: "Test".to_string(),
                },
            )
            .unwrap();
        assert!(!after.ready);
        assert_eq!(db.get_part("MCU-001").unwrap().unwrap().qty, 0);

        // Snapshot round-trip into fresh DB
        let snap = db.build_snapshot().unwrap();
        let db2 = Database::open_in_memory().unwrap();
        let (_, _, p_cnt, b_cnt) = db2.import_snapshot(&snap).unwrap();
        assert_eq!(p_cnt, 1);
        assert_eq!(b_cnt, 1);
        let p2 = db2.get_part("MCU-001").unwrap().unwrap();
        assert_eq!(p2.mpn, "ESP32-S3-WROOM-1-N8R8");
        assert!(p2.description.contains("USB-OTG"));

        // Description search (token-AND) and created/updated timestamp sorting
        let hits = db
            .list_parts_sorted(Some("psram usb-otg"), None, None, None, false, None)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].sku, "MCU-001");

        db.upsert_part(&UpsertPart {
            sku: "MCU-002".to_string(),
            category: "microcontrollers".to_string(),
            name: "RP2040 Pico".to_string(),
            spec: "Dual Cortex-M0+".to_string(),
            package: "DIP-40".to_string(),
            qty: 5,
            min_qty: 2,
            unit_cost: 4.00,
            currency: "USD".to_string(),
            location_code: Some("BIN-A1".to_string()),
            supplier_name: Some("DigiKey".to_string()),
            mpn: "SC0915".to_string(),
            supplier_sku: String::new(),
            product_url: String::new(),
            datasheet_url: String::new(),
            barcode_value: String::new(),
            projects_tags: "-".to_string(),
            description: "Raspberry Pi RP2040 dual-core board".to_string(),
            notes: String::new(),
            image_path: String::new(),
            pinout_image_path: String::new(),
            image_url: String::new(),
            pinout_image_url: String::new(),
        })
        .unwrap();

        let created_desc = db
            .list_parts_sorted(None, None, None, None, false, Some("created_desc"))
            .unwrap();
        assert_eq!(created_desc[0].sku, "MCU-002");
        assert_eq!(created_desc[1].sku, "MCU-001");

        let created_asc = db
            .list_parts_sorted(None, None, None, None, false, Some("created_asc"))
            .unwrap();
        assert_eq!(created_asc[0].sku, "MCU-001");

        // Update MCU-001 after 1.1s so updated_at strictly exceeds MCU-002's
        std::thread::sleep(std::time::Duration::from_millis(1100));
        db.adjust_stock(
            "MCU-001",
            &AdjustStockReq {
                delta: 3,
                reason: "restock".to_string(),
                note: String::new(),
                unit_cost: None,
                supplier_id: None,
                project_slug: None,
            },
        )
        .unwrap();

        let updated_desc = db
            .list_parts_sorted(None, None, None, None, false, Some("updated_desc"))
            .unwrap();
        assert_eq!(updated_desc[0].sku, "MCU-001");
        assert_eq!(updated_desc[1].sku, "MCU-002");

        let updated_asc = db
            .list_parts_sorted(None, None, None, None, false, Some("updated_asc"))
            .unwrap();
        assert_eq!(updated_asc[0].sku, "MCU-002");
    }
}
