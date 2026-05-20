//! SVG ERD renderer.
//!
//! Layout-rs / Graphviz produced unattractive output (huge whitespace,
//! flat record shapes, no visual hierarchy). This module replaces it with
//! a fully custom layout + SVG emitter:
//!
//! * Tables are laid out in topological ranks (parents → children).
//! * Each table card has a dark header, rounded corners, and per-column
//!   PK/FK badges.
//! * Foreign-key edges are drawn as cubic Bézier curves between the
//!   nearest sides of the connected tables.

// SVG layout is inherently a floating-point pipeline driven by integer counts
// (rows, ranks, badge counts). Cast lints add noise without catching real bugs.
// `unnecessary_wraps` is suppressed because the public API contract still
// returns `Result<String, String>` to leave room for future fallible paths
// (e.g. graph validation) without breaking the caller in `mod.rs`.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::range_plus_one,
    clippy::unnecessary_wraps,
    clippy::uninlined_format_args,
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::similar_names
)]

use std::collections::BTreeMap;
use std::fmt::Write as _;

use vespertide_core::{ColumnDef, TableDef};

use super::{
    ForeignKeyRelation, collect_foreign_key_relations, is_foreign_key_column, is_primary_key_column,
};

// ---------------------------------------------------------------------------
// Aesthetic constants
// ---------------------------------------------------------------------------

const FONT_FAMILY: &str = "Pretendard, 'Noto Sans KR', ui-sans-serif, system-ui, -apple-system, 'Segoe UI', \
    Roboto, 'Helvetica Neue', Arial, sans-serif";
const MONO_FAMILY: &str =
    "ui-monospace, SFMono-Regular, 'SF Mono', Menlo, Consolas, 'Courier New', monospace";

const HEADER_H: f64 = 34.0;
const ROW_H: f64 = 24.0;
const TABLE_PAD_X: f64 = 14.0;
const BADGE_W: f64 = 22.0;
const BADGE_H: f64 = 14.0;
const BADGE_GAP: f64 = 6.0;
const COL_GAP_TYPE: f64 = 18.0;
const TABLE_RADIUS: f64 = 14.0;

const TITLE_FS: f64 = 14.0;
const TITLE_CH: f64 = 7.9;
const NAME_FS: f64 = 12.0;
const NAME_CH: f64 = 6.7;
const TYPE_FS: f64 = 11.0;
const TYPE_CH: f64 = 5.8;
const BADGE_FS: f64 = 9.0;

const RANK_GAP: f64 = 80.0;
const NODE_GAP: f64 = 32.0;
const VIEW_PAD: f64 = 40.0;

// Palette (modern, light, neutral)
// DevFive (devfive.kr) brand palette.
// Signature purple #5b34f7, light bg #f7f8fb, accent yellow #ffe139.
const BG: &str = "#f7f8fb";
const CARD_BG: &str = "#ffffff";
const CARD_BORDER: &str = "#eaeaed";
const HEADER_FILL: &str = "url(#vespHeader)";
const HEADER_FG: &str = "#ffffff";
const HEADER_SUB: &str = "#e9defe";
const ROW_FG: &str = "#1a1a1a";
const ROW_FG_MUTED: &str = "#50505d";
const ROW_ALT_BG: &str = "#fafbfd";
const ROW_DIVIDER: &str = "#f0f0f4";
const PK_BG: &str = "#fff7d4";
const PK_FG: &str = "#8a6d04";
const FK_BG: &str = "#f0e9ff";
const FK_FG: &str = "#5b34f7";
const EDGE_STROKE: &str = "#b5a4f6";
const EDGE_END: &str = "#5b34f7";

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn render_svg(tables: &[TableDef]) -> Result<String, String> {
    if tables.is_empty() {
        return Ok(render_empty());
    }

    let mut boxes = build_boxes(tables);
    let relations = collect_foreign_key_relations(tables);
    let edges = build_edges(tables, &boxes, &relations);

    let ranks = compute_ranks(&boxes, &edges);
    layout_grid(&mut boxes, &ranks);

    let (vw, vh) = view_size(&boxes);
    Ok(render_doc(&boxes, &edges, vw, vh))
}

// ---------------------------------------------------------------------------
// Box / edge model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct TableBox {
    name: String,
    rows: Vec<RowSpec>,
    width: f64,
    height: f64,
    x: f64,
    y: f64,
    /// Column-name → row index, for fast FK row lookup.
    row_index: BTreeMap<String, usize>,
    /// First PK row index, used as anchor for incoming edges.
    pk_row: Option<usize>,
}

#[derive(Debug, Clone)]
struct RowSpec {
    name: String,
    type_str: String,
    is_pk: bool,
    is_fk: bool,
    nullable: bool,
}

#[derive(Debug, Clone)]
struct EdgeSpec {
    child_idx: usize,
    parent_idx: usize,
    child_row: usize,
    parent_row: usize,
    label: String,
}

fn build_boxes(tables: &[TableDef]) -> Vec<TableBox> {
    tables
        .iter()
        .map(|table| {
            let rows: Vec<RowSpec> = table
                .columns
                .iter()
                .map(|column| build_row(table, column))
                .collect();

            let mut row_index = BTreeMap::new();
            let mut pk_row = None;
            for (idx, row) in rows.iter().enumerate() {
                row_index.insert(row.name.clone(), idx);
                if row.is_pk && pk_row.is_none() {
                    pk_row = Some(idx);
                }
            }

            let width = measure_table_width(&table.name, &rows);
            let height = HEADER_H + ROW_H * rows.len() as f64;

            TableBox {
                name: table.name.clone(),
                rows,
                width,
                height,
                x: 0.0,
                y: 0.0,
                row_index,
                pk_row,
            }
        })
        .collect()
}

fn build_row(table: &TableDef, column: &ColumnDef) -> RowSpec {
    RowSpec {
        name: column.name.clone(),
        type_str: column.r#type.to_display_string(),
        is_pk: is_primary_key_column(table, &column.name),
        is_fk: is_foreign_key_column(table, &column.name),
        nullable: column.nullable,
    }
}

fn measure_table_width(name: &str, rows: &[RowSpec]) -> f64 {
    let title_w = name.chars().count() as f64 * TITLE_CH + TABLE_PAD_X * 2.0;

    let row_max = rows
        .iter()
        .map(|row| {
            let badges = badge_block_width(row);
            let name_w = row.name.chars().count() as f64 * NAME_CH;
            let type_w = row.type_str.chars().count() as f64 * TYPE_CH;
            TABLE_PAD_X * 2.0 + badges + name_w + COL_GAP_TYPE + type_w
        })
        .fold(0.0_f64, f64::max);

    let raw = title_w.max(row_max).max(180.0);
    // Round up to a nice 4-pixel grid for crispness.
    (raw / 4.0).ceil() * 4.0
}

fn badge_block_width(row: &RowSpec) -> f64 {
    let mut count = 0;
    if row.is_pk {
        count += 1;
    }
    if row.is_fk {
        count += 1;
    }
    if count == 0 {
        return 0.0;
    }
    count as f64 * BADGE_W + (count as f64 - 1.0).max(0.0) * 4.0 + BADGE_GAP
}

fn build_edges(
    tables: &[TableDef],
    boxes: &[TableBox],
    relations: &std::collections::BTreeSet<ForeignKeyRelation>,
) -> Vec<EdgeSpec> {
    let name_idx: BTreeMap<&str, usize> = tables
        .iter()
        .enumerate()
        .map(|(i, t)| (t.name.as_str(), i))
        .collect();

    let mut edges = Vec::new();
    for rel in relations {
        let Some(&child_idx) = name_idx.get(rel.child_table.as_str()) else {
            continue;
        };
        let Some(&parent_idx) = name_idx.get(rel.parent_table.as_str()) else {
            continue;
        };
        if child_idx == parent_idx {
            // Self-reference: skip drawing (rare and hard to route nicely).
            continue;
        }

        let child_row = rel
            .child_columns
            .first()
            .and_then(|c| boxes[child_idx].row_index.get(c).copied())
            .unwrap_or(0);
        let parent_row = rel
            .parent_columns
            .first()
            .and_then(|c| boxes[parent_idx].row_index.get(c).copied())
            .or(boxes[parent_idx].pk_row)
            .unwrap_or(0);

        let label = format!(
            "{} → {}",
            rel.child_columns.join(", "),
            rel.parent_columns.join(", ")
        );

        edges.push(EdgeSpec {
            child_idx,
            parent_idx,
            child_row,
            parent_row,
            label,
        });
    }
    edges
}

// ---------------------------------------------------------------------------
// Rank assignment + grid layout
// ---------------------------------------------------------------------------

fn compute_ranks(boxes: &[TableBox], edges: &[EdgeSpec]) -> Vec<usize> {
    let n = boxes.len();
    let mut parents: Vec<Vec<usize>> = vec![Vec::new(); n];
    for edge in edges {
        parents[edge.child_idx].push(edge.parent_idx);
    }

    let mut ranks = vec![0_usize; n];
    // Iterative fixed-point; cap iterations to avoid cycles spiralling.
    for _ in 0..(n + 1) {
        let mut changed = false;
        for i in 0..n {
            let candidate = parents[i]
                .iter()
                .map(|&p| ranks[p].saturating_add(1))
                .max()
                .unwrap_or(0);
            if candidate > ranks[i] {
                ranks[i] = candidate;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    ranks
}

fn layout_grid(boxes: &mut [TableBox], ranks: &[usize]) {
    let max_rank = ranks.iter().copied().max().unwrap_or(0);
    let num_ranks = max_rank + 1;

    // Bucket by rank.
    let mut groups: Vec<Vec<usize>> = vec![Vec::new(); num_ranks];
    for (i, &r) in ranks.iter().enumerate() {
        groups[r].push(i);
    }

    // Stable order inside each rank: by name.
    for group in &mut groups {
        group.sort_by(|&a, &b| boxes[a].name.cmp(&boxes[b].name));
    }

    // If the layout is very lopsided (one rank stuffed full while another is sparse),
    // rebalance by splitting the largest rank.
    rebalance_groups(&mut groups, boxes.len());

    // Compute per-rank column width as max box width.
    let col_widths: Vec<f64> = groups
        .iter()
        .map(|group| {
            group
                .iter()
                .map(|&i| boxes[i].width)
                .fold(180.0_f64, f64::max)
        })
        .collect();

    // X positions per rank (left edge of column).
    let mut col_x = Vec::with_capacity(groups.len());
    let mut cursor = VIEW_PAD;
    for w in &col_widths {
        col_x.push(cursor);
        cursor += *w + RANK_GAP;
    }

    // Place inside each column, centered horizontally on the column's width.
    for (rank_idx, group) in groups.iter().enumerate() {
        let mut y = VIEW_PAD;
        let column_x = col_x[rank_idx];
        let column_w = col_widths[rank_idx];
        for &i in group {
            let bx = &mut boxes[i];
            bx.x = column_x + (column_w - bx.width) / 2.0;
            bx.y = y;
            y += bx.height + NODE_GAP;
        }
    }
}

fn rebalance_groups(groups: &mut Vec<Vec<usize>>, total: usize) {
    if groups.is_empty() {
        return;
    }
    let target_max = ((total as f64).sqrt().ceil() as usize).max(3);

    let mut i = 0;
    while i < groups.len() {
        if groups[i].len() > target_max {
            let overflow: Vec<usize> = groups[i].split_off(target_max);
            groups.insert(i + 1, overflow);
        }
        i += 1;
    }
}

fn view_size(boxes: &[TableBox]) -> (f64, f64) {
    let mut w = 0.0_f64;
    let mut h = 0.0_f64;
    for bx in boxes {
        w = w.max(bx.x + bx.width);
        h = h.max(bx.y + bx.height);
    }
    (w + VIEW_PAD, h + VIEW_PAD)
}

// ---------------------------------------------------------------------------
// SVG emission
// ---------------------------------------------------------------------------

fn render_doc(boxes: &[TableBox], edges: &[EdgeSpec], vw: f64, vh: f64) -> String {
    let mut out = String::with_capacity(4096);

    let _ = writeln!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {w:.0} {h:.0}\" \
         width=\"{w:.0}\" height=\"{h:.0}\" font-family=\"{ff}\" \
         style=\"letter-spacing:-0.25px\">",
        w = vw,
        h = vh,
        ff = FONT_FAMILY,
    );

    render_defs(&mut out);

    let _ = writeln!(
        out,
        "  <rect x=\"0\" y=\"0\" width=\"{w:.0}\" height=\"{h:.0}\" fill=\"{bg}\"/>",
        w = vw,
        h = vh,
        bg = BG,
    );

    // Edges first, so tables render above.
    out.push_str("  <g class=\"edges\" fill=\"none\">\n");
    for edge in edges {
        render_edge(
            &mut out,
            &boxes[edge.child_idx],
            &boxes[edge.parent_idx],
            edge,
        );
    }
    out.push_str("  </g>\n");

    // Tables.
    out.push_str("  <g class=\"tables\">\n");
    for bx in boxes {
        render_table(&mut out, bx);
    }
    out.push_str("  </g>\n");

    out.push_str("</svg>\n");
    out
}

fn render_defs(out: &mut String) {
    out.push_str("  <defs>\n");
    out.push_str(
        "    <linearGradient id=\"vespHeader\" x1=\"0\" y1=\"0\" x2=\"0\" y2=\"1\">\n\
             \x20     <stop offset=\"0\" stop-color=\"#5b34f7\"/>\n\
             \x20     <stop offset=\"1\" stop-color=\"#7e5cff\"/>\n\
             \x20   </linearGradient>\n",
    );
    out.push_str(
        "    <filter id=\"vespShadow\" x=\"-20%\" y=\"-20%\" width=\"140%\" height=\"140%\">\n\
             \x20     <feDropShadow dx=\"0\" dy=\"2\" stdDeviation=\"3\" \
             flood-color=\"#5b34f7\" flood-opacity=\"0.10\"/>\n\
             \x20   </filter>\n",
    );
    out.push_str(
        "    <marker id=\"vespArrow\" viewBox=\"0 0 10 10\" refX=\"9\" refY=\"5\" \
         markerWidth=\"7\" markerHeight=\"7\" orient=\"auto-start-reverse\">\n\
             \x20     <path d=\"M0 0 L10 5 L0 10 z\" fill=\"#5b34f7\"/>\n\
             \x20   </marker>\n",
    );
    out.push_str(
        "    <marker id=\"vespCircle\" viewBox=\"0 0 10 10\" refX=\"5\" refY=\"5\" \
         markerWidth=\"6\" markerHeight=\"6\" orient=\"auto\">\n\
             \x20     <circle cx=\"5\" cy=\"5\" r=\"3\" fill=\"#ffffff\" \
             stroke=\"#5b34f7\" stroke-width=\"1.6\"/>\n\
             \x20   </marker>\n",
    );
    out.push_str("  </defs>\n");
}

fn render_table(out: &mut String, bx: &TableBox) {
    let _ = writeln!(
        out,
        "    <g class=\"table\" transform=\"translate({x:.1} {y:.1})\">",
        x = bx.x,
        y = bx.y,
    );

    // Card background with shadow.
    let _ = writeln!(
        out,
        "      <rect class=\"card\" x=\"0\" y=\"0\" width=\"{w:.0}\" height=\"{h:.0}\" \
         rx=\"{r}\" ry=\"{r}\" fill=\"{cbg}\" stroke=\"{cb}\" stroke-width=\"1\" \
         filter=\"url(#vespShadow)\"/>",
        w = bx.width,
        h = bx.height,
        r = TABLE_RADIUS,
        cbg = CARD_BG,
        cb = CARD_BORDER,
    );

    // Header band — use a path so only the top corners are rounded.
    let header_path = rounded_top_path(bx.width, HEADER_H, TABLE_RADIUS);
    let _ = writeln!(
        out,
        "      <path d=\"{path}\" fill=\"{fill}\"/>",
        path = header_path,
        fill = HEADER_FILL,
    );

    // Title.
    let _ = writeln!(
        out,
        "      <text x=\"{tx:.1}\" y=\"{ty:.1}\" fill=\"{fg}\" font-size=\"{fs}\" \
         font-weight=\"600\" letter-spacing=\"0.2\">{name}</text>",
        tx = TABLE_PAD_X,
        ty = HEADER_H / 2.0 + TITLE_FS / 2.0 - 2.0,
        fg = HEADER_FG,
        fs = TITLE_FS,
        name = escape_xml(&bx.name),
    );

    // Column count hint, right-aligned in header.
    let count_str = format!("{} cols", bx.rows.len());
    let _ = writeln!(
        out,
        "      <text x=\"{cx:.1}\" y=\"{cy:.1}\" fill=\"{sub}\" font-size=\"10\" \
         font-weight=\"500\" text-anchor=\"end\">{count}</text>",
        cx = bx.width - TABLE_PAD_X,
        cy = HEADER_H / 2.0 + 4.0,
        sub = HEADER_SUB,
        count = escape_xml(&count_str),
    );

    // Rows.
    for (idx, row) in bx.rows.iter().enumerate() {
        render_row(out, bx, idx, row);
    }

    out.push_str("    </g>\n");
}

fn render_row(out: &mut String, bx: &TableBox, idx: usize, row: &RowSpec) {
    let y = HEADER_H + idx as f64 * ROW_H;
    let is_last = idx == bx.rows.len() - 1;

    // Alt background for zebra striping. Skip the very last row's stripe to keep
    // the rounded bottom corners clean (the card border handles the visual).
    if idx % 2 == 1 {
        if is_last {
            let path = rounded_bottom_path(bx.width, y, ROW_H, TABLE_RADIUS);
            let _ = writeln!(out, "      <path d=\"{path}\" fill=\"{ROW_ALT_BG}\"/>");
        } else {
            let _ = writeln!(
                out,
                "      <rect x=\"0\" y=\"{y:.1}\" width=\"{w:.0}\" height=\"{h:.1}\" \
                 fill=\"{bg}\"/>",
                y = y,
                w = bx.width,
                h = ROW_H,
                bg = ROW_ALT_BG,
            );
        }
    }

    // Top divider (skip on the first row — header bottom acts as divider).
    if idx > 0 {
        let _ = writeln!(
            out,
            "      <line x1=\"{x1:.0}\" y1=\"{y:.1}\" x2=\"{x2:.0}\" y2=\"{y:.1}\" \
             stroke=\"{c}\" stroke-width=\"1\"/>",
            x1 = 1.0,
            x2 = bx.width - 1.0,
            y = y,
            c = ROW_DIVIDER,
        );
    }

    // Badges.
    let mut badge_x = TABLE_PAD_X;
    if row.is_pk {
        render_badge(
            out,
            badge_x,
            y + (ROW_H - BADGE_H) / 2.0,
            "PK",
            PK_BG,
            PK_FG,
        );
        badge_x += BADGE_W + 4.0;
    }
    if row.is_fk {
        render_badge(
            out,
            badge_x,
            y + (ROW_H - BADGE_H) / 2.0,
            "FK",
            FK_BG,
            FK_FG,
        );
        badge_x += BADGE_W + 4.0;
    }

    let name_x = if row.is_pk || row.is_fk {
        badge_x + BADGE_GAP - 4.0
    } else {
        TABLE_PAD_X
    };

    // Column name.
    let name_weight = if row.is_pk { "600" } else { "500" };
    let _ = writeln!(
        out,
        "      <text x=\"{nx:.1}\" y=\"{ty:.1}\" fill=\"{fg}\" font-size=\"{fs}\" \
         font-weight=\"{w}\">{name}</text>",
        nx = name_x,
        ty = y + ROW_H / 2.0 + NAME_FS / 2.0 - 2.0,
        fg = ROW_FG,
        fs = NAME_FS,
        w = name_weight,
        name = escape_xml(&row.name),
    );

    // Type, right-aligned in monospace.
    let type_display = if row.nullable {
        format!("{}?", row.type_str)
    } else {
        row.type_str.clone()
    };
    let _ = writeln!(
        out,
        "      <text x=\"{tx:.1}\" y=\"{ty:.1}\" fill=\"{fg}\" font-size=\"{fs}\" \
         font-family=\"{ff}\" font-style=\"italic\" text-anchor=\"end\">{t}</text>",
        tx = bx.width - TABLE_PAD_X,
        ty = y + ROW_H / 2.0 + TYPE_FS / 2.0 - 2.0,
        fg = ROW_FG_MUTED,
        fs = TYPE_FS,
        ff = MONO_FAMILY,
        t = escape_xml(&type_display),
    );
}

fn render_badge(out: &mut String, x: f64, y: f64, label: &str, bg: &str, fg: &str) {
    let _ = writeln!(
        out,
        "      <g class=\"badge\"><rect x=\"{x:.1}\" y=\"{y:.1}\" \
         width=\"{w}\" height=\"{h}\" rx=\"3\" ry=\"3\" fill=\"{bg}\"/>\
         <text x=\"{tx:.1}\" y=\"{ty:.1}\" fill=\"{fg}\" font-size=\"{fs}\" \
         font-weight=\"700\" text-anchor=\"middle\" letter-spacing=\"0.4\">{label}</text></g>",
        x = x,
        y = y,
        w = BADGE_W,
        h = BADGE_H,
        bg = bg,
        tx = x + BADGE_W / 2.0,
        ty = y + BADGE_H / 2.0 + BADGE_FS / 2.0 - 1.5,
        fg = fg,
        fs = BADGE_FS,
        label = label,
    );
}

fn rounded_top_path(w: f64, h: f64, r: f64) -> String {
    format!(
        "M 0 {h:.1} L 0 {r:.1} Q 0 0 {r:.1} 0 L {wr:.1} 0 Q {w:.1} 0 {w:.1} {r:.1} \
         L {w:.1} {h:.1} Z",
        w = w,
        h = h,
        r = r,
        wr = w - r,
    )
}

fn rounded_bottom_path(w: f64, top_y: f64, h: f64, r: f64) -> String {
    let bot = top_y + h;
    format!(
        "M 0 {top:.1} L {w:.1} {top:.1} L {w:.1} {br:.1} Q {w:.1} {bot:.1} {wr:.1} {bot:.1} \
         L {r:.1} {bot:.1} Q 0 {bot:.1} 0 {br:.1} Z",
        top = top_y,
        w = w,
        bot = bot,
        br = bot - r,
        wr = w - r,
        r = r,
    )
}

// ---------------------------------------------------------------------------
// Edge routing
// ---------------------------------------------------------------------------

fn render_edge(out: &mut String, child: &TableBox, parent: &TableBox, edge: &EdgeSpec) {
    let child_y = child.y + HEADER_H + edge.child_row as f64 * ROW_H + ROW_H / 2.0;
    let parent_y = parent.y + HEADER_H + edge.parent_row as f64 * ROW_H + ROW_H / 2.0;

    // Pick the sides closest to each other.
    let (sx, sy, ex, ey, sdir, edir) = pick_anchors(child, parent, child_y, parent_y);

    let path = bezier_path(sx, sy, ex, ey, sdir, edir);

    // Two-layer stroke: subtle wide halo + crisp narrow stroke for a soft look.
    let _ = writeln!(
        out,
        "    <path d=\"{path}\" stroke=\"#ffffff\" stroke-width=\"4\" opacity=\"0.7\"/>",
    );
    let _ = writeln!(
        out,
        "    <path d=\"{path}\" stroke=\"{stroke}\" stroke-width=\"1.6\" \
         marker-start=\"url(#vespCircle)\" marker-end=\"url(#vespArrow)\">\
         <title>{title}</title></path>",
        stroke = EDGE_STROKE,
        title = escape_xml(&format!("{} {} → {}", child.name, edge.label, parent.name)),
    );

    // Suppress unused-variable warning for EDGE_END when not referenced elsewhere.
    let _ = EDGE_END;
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

fn pick_anchors(
    child: &TableBox,
    parent: &TableBox,
    child_y: f64,
    parent_y: f64,
) -> (f64, f64, f64, f64, Side, Side) {
    let child_left = child.x;
    let child_right = child.x + child.width;
    let parent_left = parent.x;
    let parent_right = parent.x + parent.width;

    // Prefer horizontal connections — they read cleaner for ERDs.
    let horizontal_separation = parent_left > child_right || child_left > parent_right;
    if horizontal_separation {
        if parent_left >= child_right {
            // Parent is to the right of the child.
            return (
                child_right,
                child_y,
                parent_left,
                parent_y,
                Side::Right,
                Side::Left,
            );
        }
        // Parent is to the left of the child.
        return (
            child_left,
            child_y,
            parent_right,
            parent_y,
            Side::Left,
            Side::Right,
        );
    }

    // Otherwise route top/bottom.
    if parent.y + parent.height <= child.y {
        let sx = child.x + child.width / 2.0;
        let ex = parent.x + parent.width / 2.0;
        return (
            sx,
            child.y,
            ex,
            parent.y + parent.height,
            Side::Top,
            Side::Bottom,
        );
    }
    let sx = child.x + child.width / 2.0;
    let ex = parent.x + parent.width / 2.0;
    (
        sx,
        child.y + child.height,
        ex,
        parent.y,
        Side::Bottom,
        Side::Top,
    )
}

fn bezier_path(sx: f64, sy: f64, ex: f64, ey: f64, s_side: Side, e_side: Side) -> String {
    let dx = (ex - sx).abs();
    let dy = (ey - sy).abs();
    let pull = dx.max(dy).max(40.0) * 0.5;

    let (cs_x, cs_y) = control_point(sx, sy, s_side, pull);
    let (ce_x, ce_y) = control_point(ex, ey, e_side, pull);

    format!(
        "M {sx:.1} {sy:.1} C {csx:.1} {csy:.1} {cex:.1} {cey:.1} {ex:.1} {ey:.1}",
        sx = sx,
        sy = sy,
        csx = cs_x,
        csy = cs_y,
        cex = ce_x,
        cey = ce_y,
        ex = ex,
        ey = ey,
    )
}

fn control_point(x: f64, y: f64, side: Side, pull: f64) -> (f64, f64) {
    match side {
        Side::Left => (x - pull, y),
        Side::Right => (x + pull, y),
        Side::Top => (x, y - pull),
        Side::Bottom => (x, y + pull),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn render_empty() -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 320 120\" \
         width=\"320\" height=\"120\" font-family=\"{ff}\">\n\
         \x20 <rect x=\"0\" y=\"0\" width=\"320\" height=\"120\" fill=\"{bg}\"/>\n\
         \x20 <text x=\"160\" y=\"65\" fill=\"#50505d\" font-size=\"14\" \
         text-anchor=\"middle\">No tables to render</text>\n\
         </svg>\n",
        ff = FONT_FAMILY,
        bg = BG,
    )
}

fn escape_xml(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}
