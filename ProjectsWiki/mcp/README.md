# BenchBear MCP Server

The built-in stdio MCP server runs directly from the compiled Rust engine (`target/release/benchbear mcp`), or via the Python wrapper (`mcp/server.py`), which automatically delegates to the Rust binary when present.

## Available Tools

- `search_parts` — Filter parts by query, category, location, supplier, or low-stock status
- `get_part` — Inspect full part details, alternate suppliers, and stock movement history
- `add_part` / `update_part` — Create or update parts with location, supplier, MPN, supplier SKU, and unit cost
- `adjust_stock` — Increment/decrement part quantities with optional restock unit cost and movement notes
- `move_part` — Move a part to a different bin/location
- `list_locations` / `upsert_location` — Manage storage bins, racks, drawers, and `BIN:` barcodes
- `list_suppliers` / `upsert_supplier` / `add_part_supplier` — Manage vendors, lead times, and multi-supplier quotes
- `get_valuation` — Get total inventory value and breakdown by category, location, and supplier
- `get_reorder_list` — Get low-stock parts grouped by supplier with estimated order totals
- `read_project` / `check_bom` / `deduct_bom` — Inspect project BOMs, check shortages/costs, and deduct builds
- `generate_label` — Generate thermal SVG or vector PDF labels (`2x1`, `2.25x1.25`, `ql62x29`, `ql62x42`)
- `decode_barcode` — Decode 1D/2D barcodes from an image file and resolve matching parts/bins

## Client Configuration

See [configs/mcp_config.json](../configs/mcp_config.json) for a drop-in `mcpServers` block.
