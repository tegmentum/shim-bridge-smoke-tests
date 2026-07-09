//! Per-statement rewrites applied to the scraper output before the
//! `sql_inline` column lands in `test_cases`.
//!
//! Two rewrites live here today:
//!
//! * **`inject_geometry_casts`** — walks single-quoted string literals
//!   and appends `::GEOMETRY` when the literal is a bare WKT/EWKT.
//!   DuckDB's parser sees the literal as `VARCHAR` and, once the
//!   postgis extension is loaded, the shim exposes `st_*` as
//!   `(postgis.geometry, ...)` signatures rather than the built-in
//!   `GEOMETRY` type. Without the cast the binder rejects the call
//!   with `No function matches st_xxx(GEOMETRY, ...)`. The upstream
//!   PostgreSQL corpus relies on implicit `text -> geometry`
//!   coercion which DuckDB does not perform, so we inject it
//!   explicitly here.
//!
//! * **`load_function_leaf_map`** — reads a `postgis-catalog.toml` /
//!   `mobilitydb-catalog.toml` and returns a
//!   `function_name -> leaf_id` map so `main.rs` can stamp every
//!   emitted case with the canonical `leaf:<leaf>` tag. Functions
//!   not owned by any leaf are stamped `leaf:orphan` at the call
//!   site.
//!
//! Both rewrites are pure and cheap; they run once per case row.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

/// Function names that already take a WKT/WKB text or blob argument
/// and therefore must NOT receive a `::GEOMETRY` cast on their
/// string-literal argument. Passing the literal as `GEOMETRY` to
/// these parsers fails DuckDB's binder because the shim declares
/// them with `VARCHAR`/`BLOB` argument types, not `postgis.geometry`.
///
/// Match is case-insensitive on the enclosing call name. Names are
/// stored lowercase.
const WKT_PARSER_FNS: &[&str] = &[
    "st_geomfromtext",
    "st_geomfromewkt",
    "st_geomfromtwkb",
    "st_geomfromwkb",
    "st_geomfromewkb",
    "st_geomfromgeohash",
    "st_geomfromgeojson",
    "st_geomfromgml",
    "st_geomfromkml",
    "st_geomfrommarc21",
    "st_pointfromtext",
    "st_linefromtext",
    "st_polygonfromtext",
    "st_mpointfromtext",
    "st_mlinefromtext",
    "st_mpolyfromtext",
    "st_geomcollfromtext",
    "st_geographyfromtext",
    "st_geogfromtext",
    "st_geogfromwkb",
    "st_geog_from_text",
    "st_geog_from_wkb",
    // Underscore variants of the WKB parsers (some drivers spell
    // them this way).
    "st_geom_from_text",
    "st_geom_from_wkb",
    "st_geom_from_ewkt",
    "st_geom_from_ewkb",
    "st_geom_from_twkb",
    "st_geom_from_geojson",
    "st_geom_from_geohash",
    "st_geom_from_gml",
    "st_geom_from_kml",
    "st_geom_from_marc21",
    "st_point_from_text",
    "st_line_from_text",
    "st_polygon_from_text",
    "st_mpoint_from_text",
    "st_mline_from_text",
    "st_mpoly_from_text",
    "st_geomcoll_from_text",
    "st_geography_from_text",
];

/// True if `name` (case-insensitive) is a WKT/WKB parser that we
/// must not inject `::GEOMETRY` casts into.
fn is_wkt_parser(name: &str) -> bool {
    let lc = name.to_ascii_lowercase();
    WKT_PARSER_FNS.contains(&lc.as_str())
}

/// The WKT type keywords we recognise, longest first so that
/// `MULTIPOINT` is matched before `POINT`. Case-insensitive at match
/// time.
const WKT_KEYWORDS: &[&str] = &[
    "GEOMETRYCOLLECTION",
    "POLYHEDRALSURFACE",
    "MULTILINESTRING",
    "COMPOUNDCURVE",
    "CIRCULARSTRING",
    "MULTIPOLYGON",
    "MULTISURFACE",
    "CURVEPOLYGON",
    "MULTIPOINT",
    "MULTICURVE",
    "LINESTRING",
    "TRIANGLE",
    "POLYGON",
    "POINT",
    "TIN",
];

/// Rewrite a SQL statement so every bare-WKT string literal is
/// followed by `::GEOMETRY`. Preserves literals that are already
/// cast (`'POINT(1 2)'::geometry`, `'POINT(1 2)'::geography`) and
/// preserves literals whose immediately-enclosing call is a
/// WKT/WKB parser function (`st_geomfromtext`, `st_geomfromewkt`,
/// `st_pointfromtext`, …) — those parsers expect a `VARCHAR`/`BLOB`
/// argument and reject `GEOMETRY`.
pub fn inject_geometry_casts(sql: &str) -> String {
    let bytes = sql.as_bytes();
    let mut out = String::with_capacity(bytes.len() + 32);
    let mut i = 0;
    // Stack of currently-open function-call names (lowercased).
    // Pushed on `(` after an identifier; empty-string is pushed
    // when the `(` is a bare grouping paren.
    let mut call_stack: Vec<String> = Vec::new();
    // The most recently completed identifier — set when we see a
    // run of `[A-Za-z0-9_]`, cleared on operators/commas/etc.
    // Preserved across whitespace so `foo (x)` still counts `foo`
    // as the callee.
    let mut last_ident = String::new();
    let mut ident_active = false;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\'' {
            // Scan a single-quoted literal, honouring `''` as an
            // in-string escaped quote.
            let start = i;
            let mut j = i + 1;
            while j < bytes.len() {
                if bytes[j] == b'\'' {
                    if j + 1 < bytes.len() && bytes[j + 1] == b'\'' {
                        j += 2;
                        continue;
                    }
                    break;
                }
                j += 1;
            }
            if j >= bytes.len() {
                // Unterminated literal — leave as-is.
                out.push_str(&sql[start..]);
                return out;
            }
            let lit_body = &sql[start + 1..j];
            out.push_str(&sql[start..=j]);
            i = j + 1;
            let inside_parser = call_stack
                .last()
                .map(|n| !n.is_empty() && is_wkt_parser(n))
                .unwrap_or(false);
            if looks_like_wkt(lit_body)
                && !already_cast(&sql[i..])
                && !inside_parser
            {
                out.push_str("::GEOMETRY");
            }
            // A literal is not an identifier; clear pending ident.
            last_ident.clear();
            ident_active = false;
            continue;
        }
        // Skip line/block comments so we don't accidentally rewrite
        // literals inside them — the scraper strips comments before
        // this runs, but the preprocess helper is also called on
        // migrated rows in-place, where comments may survive.
        if b == b'-' && i + 1 < bytes.len() && bytes[i + 1] == b'-' {
            while i < bytes.len() && bytes[i] != b'\n' {
                out.push(bytes[i] as char);
                i += 1;
            }
            last_ident.clear();
            ident_active = false;
            continue;
        }
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            out.push_str("/*");
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                out.push(bytes[i] as char);
                i += 1;
            }
            if i + 1 < bytes.len() {
                out.push_str("*/");
                i += 2;
            }
            last_ident.clear();
            ident_active = false;
            continue;
        }
        // Identifier + call-context tracking.
        if b.is_ascii_alphanumeric() || b == b'_' {
            if !ident_active {
                last_ident.clear();
                ident_active = true;
            }
            last_ident.push(b as char);
            out.push(b as char);
            i += 1;
            continue;
        }
        if b.is_ascii_whitespace() {
            // Whitespace pauses ident collection but keeps the
            // name available for the next `(`.
            ident_active = false;
            out.push(b as char);
            i += 1;
            continue;
        }
        if b == b'(' {
            // Push the pending identifier as the current callee
            // (or empty for a bare grouping paren).
            call_stack.push(last_ident.clone());
            last_ident.clear();
            ident_active = false;
            out.push('(');
            i += 1;
            continue;
        }
        if b == b')' {
            // Pop unconditionally — mismatched parens would only
            // happen on malformed SQL, and the emitted text is
            // unchanged either way.
            call_stack.pop();
            last_ident.clear();
            ident_active = false;
            out.push(')');
            i += 1;
            continue;
        }
        // Anything else (comma, arithmetic, `.`, `;`, quotes we
        // already handled): clear the pending identifier.
        last_ident.clear();
        ident_active = false;
        out.push(b as char);
        i += 1;
    }
    out
}

/// A literal body is treated as bare WKT if — after stripping an
/// optional `SRID=<n>;` prefix and leading whitespace — the first
/// token matches one of `WKT_KEYWORDS` case-insensitively and is
/// followed by whitespace, `(`, or the token `EMPTY` / `Z` / `M` /
/// `ZM`.
fn looks_like_wkt(lit_body: &str) -> bool {
    let mut t = lit_body.trim();
    // Optional SRID prefix: `SRID=4326;<wkt>`.
    if let Some(rest) = strip_srid_prefix(t) {
        t = rest.trim_start();
    }
    let up = t.to_ascii_uppercase();
    for kw in WKT_KEYWORDS {
        if let Some(after) = up.strip_prefix(kw) {
            // The keyword must be followed by an opener that
            // distinguishes a WKT tag from an accidental prefix
            // match (e.g. a chunk of freeform text starting with
            // "POINT..." would otherwise look like WKT).
            let a = after.trim_start();
            if a.is_empty() {
                return false;
            }
            if a.starts_with('(') || a.starts_with("EMPTY") {
                return true;
            }
            // WKT dimensionality tags: `POINT Z (1 2 3)`,
            // `POINT ZM (1 2 3 4)`, `POINT M EMPTY`, etc.
            for dim in &["Z ", "M ", "ZM ", "Z(", "M(", "ZM("] {
                if a.starts_with(dim) {
                    return true;
                }
            }
        }
    }
    false
}

fn strip_srid_prefix(s: &str) -> Option<&str> {
    let up = s.to_ascii_uppercase();
    if !up.starts_with("SRID=") {
        return None;
    }
    let after_eq = &s[5..];
    // Find the `;`
    let semi = after_eq.find(';')?;
    // Everything left of `;` should be digits (with optional sign).
    let n = &after_eq[..semi];
    let n_ok = !n.is_empty() && n.chars().all(|c| c.is_ascii_digit() || c == '-');
    if !n_ok {
        return None;
    }
    Some(&after_eq[semi + 1..])
}

/// After the closing `'` of a literal, does the tail already carry a
/// `::<type>` cast that would render our injection redundant or
/// wrong (e.g. `::geography`, `::text`, `::bytea`)?
fn already_cast(after_close: &str) -> bool {
    let t = after_close.trim_start();
    t.starts_with("::")
}

/// Root shape of a shim-interface catalog TOML. We only need the
/// leaves array with each leaf's function lists.
#[derive(Debug, Deserialize)]
struct CatalogRoot {
    #[serde(default)]
    leaves: Vec<CatalogLeaf>,
}

#[derive(Debug, Deserialize)]
struct CatalogLeaf {
    id: String,
    #[serde(default)]
    scalars: Vec<String>,
    #[serde(default)]
    aggregates: Vec<String>,
    #[serde(default)]
    table_functions: Vec<String>,
    #[serde(default)]
    window_functions: Vec<String>,
}

/// Build a `function_name -> leaf_id` map from the given catalog
/// TOML. Function names are lowercased for a case-insensitive
/// lookup against the scraper's canonical form. When a function
/// appears in multiple leaves, first-wins by leaf declaration order
/// (matches the catalog's own tie-break rule).
pub fn load_function_leaf_map(path: &Path) -> Result<HashMap<String, String>> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("read {}", path.display()))?;
    let root: CatalogRoot = toml::from_str(&raw)
        .with_context(|| format!("parse {}", path.display()))?;
    let mut out: HashMap<String, String> = HashMap::new();
    for leaf in &root.leaves {
        for f in leaf
            .scalars
            .iter()
            .chain(leaf.aggregates.iter())
            .chain(leaf.table_functions.iter())
            .chain(leaf.window_functions.iter())
        {
            out.entry(f.to_ascii_lowercase())
                .or_insert_with(|| leaf.id.clone());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_cast_to_bare_point() {
        let s = "SELECT st_area('POINT(1 2)')";
        let out = inject_geometry_casts(s);
        assert_eq!(out, "SELECT st_area('POINT(1 2)'::GEOMETRY)");
    }

    #[test]
    fn adds_cast_to_polygon_and_multipoint() {
        let s = "SELECT ST_Intersects('MULTIPOINT(0 0, 1 1)', 'POLYGON((0 0,1 0,1 1,0 1,0 0))')";
        let out = inject_geometry_casts(s);
        assert_eq!(
            out,
            "SELECT ST_Intersects('MULTIPOINT(0 0, 1 1)'::GEOMETRY, 'POLYGON((0 0,1 0,1 1,0 1,0 0))'::GEOMETRY)"
        );
    }

    #[test]
    fn respects_existing_cast() {
        let s = "SELECT st_area('POINT(1 2)'::geography)";
        let out = inject_geometry_casts(s);
        assert_eq!(out, s);
    }

    #[test]
    fn honours_srid_prefix() {
        let s = "SELECT 'SRID=4326;POINT(1 2)'";
        let out = inject_geometry_casts(s);
        assert_eq!(out, "SELECT 'SRID=4326;POINT(1 2)'::GEOMETRY");
    }

    #[test]
    fn ignores_non_wkt_literal() {
        let s = "SELECT 'hello world' AS x";
        let out = inject_geometry_casts(s);
        assert_eq!(out, s);
    }

    #[test]
    fn handles_wkt_with_dim_tag() {
        let s = "SELECT st_area('POINT Z (1 2 3)')";
        let out = inject_geometry_casts(s);
        assert_eq!(out, "SELECT st_area('POINT Z (1 2 3)'::GEOMETRY)");
    }

    #[test]
    fn handles_empty_wkt() {
        let s = "SELECT 'POLYGON EMPTY'";
        let out = inject_geometry_casts(s);
        assert_eq!(out, "SELECT 'POLYGON EMPTY'::GEOMETRY");
    }

    #[test]
    fn preserves_embedded_apostrophes() {
        let s = "SELECT 'it''s not wkt' AS x";
        let out = inject_geometry_casts(s);
        assert_eq!(out, s);
    }

    #[test]
    fn skips_cast_inside_st_geomfromtext() {
        let s = "SELECT ST_GeomFromText('POINT(1 2)')";
        let out = inject_geometry_casts(s);
        assert_eq!(out, s);
    }

    #[test]
    fn skips_cast_inside_st_geomfromewkt_case_insensitive() {
        let s = "SELECT st_GeomFromEWKT('SRID=4326;POINT(1 2)')";
        let out = inject_geometry_casts(s);
        assert_eq!(out, s);
    }

    #[test]
    fn skips_cast_inside_st_pointfromtext() {
        let s = "SELECT ST_PointFromText('POINT(1 2)', 4326)";
        let out = inject_geometry_casts(s);
        assert_eq!(out, s);
    }

    #[test]
    fn skips_cast_inside_st_geog_from_text_underscore() {
        let s = "SELECT ST_Geog_From_Text('POINT(1 2)')";
        let out = inject_geometry_casts(s);
        assert_eq!(out, s);
    }

    #[test]
    fn skips_cast_with_whitespace_between_ident_and_paren() {
        let s = "SELECT ST_GeomFromText ('POINT(1 2)')";
        let out = inject_geometry_casts(s);
        assert_eq!(out, s);
    }

    #[test]
    fn casts_outer_wkt_when_inner_is_parser() {
        // Outer call is st_area, inner is st_geomfromtext; the
        // outer WKT literal (there is none here) would be cast but
        // the inner literal (inside the parser) must not be.
        let s = "SELECT st_area(ST_GeomFromText('POINT(1 2)'))";
        let out = inject_geometry_casts(s);
        assert_eq!(out, s);
    }

    #[test]
    fn casts_when_outer_call_is_not_a_parser() {
        // ST_Buffer takes GEOMETRY, so the bare WKT literal must
        // be cast even though it's nested inside another call.
        let s = "SELECT st_astext(st_buffer('POINT(1 2)', 1))";
        let out = inject_geometry_casts(s);
        assert_eq!(
            out,
            "SELECT st_astext(st_buffer('POINT(1 2)'::GEOMETRY, 1))"
        );
    }

    #[test]
    fn call_stack_pops_on_close_paren() {
        // The first literal is inside st_geomfromtext (skip cast);
        // the second sits outside any parser call (cast).
        let s =
            "SELECT ST_GeomFromText('POINT(1 2)'), st_area('POLYGON((0 0,1 0,1 1,0 0))')";
        let out = inject_geometry_casts(s);
        assert_eq!(
            out,
            "SELECT ST_GeomFromText('POINT(1 2)'), st_area('POLYGON((0 0,1 0,1 1,0 0))'::GEOMETRY)"
        );
    }
}
