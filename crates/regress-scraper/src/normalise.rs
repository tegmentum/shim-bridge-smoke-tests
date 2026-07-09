//! psql `-tA` -> duckdb `.mode csv` value transforms.
//!
//! The comparator in `test-fn` uses `expected.trim() == actual.trim()`
//! after byte-level comparison. To keep expected values from psql
//! runnable against the ducklink-composed shim, per-type massage
//! is applied here.

pub fn normalise_psql_to_duckdb(v: &str) -> String {
    let t = v.trim();
    // Booleans.
    if t == "t" {
        return "true".to_string();
    }
    if t == "f" {
        return "false".to_string();
    }
    // NULL (blank cell).
    if t.is_empty() {
        return String::new();
    }
    // WKT tightening: `POINT(1 2)` -> `POINT (1 2)` (space after
    // type keyword) to match ducklink shim output.
    let wkt_types = [
        "POINT",
        "MULTIPOINT",
        "LINESTRING",
        "MULTILINESTRING",
        "POLYGON",
        "MULTIPOLYGON",
        "GEOMETRYCOLLECTION",
        "CIRCULARSTRING",
        "COMPOUNDCURVE",
        "CURVEPOLYGON",
        "MULTISURFACE",
        "MULTICURVE",
        "TRIANGLE",
        "POLYHEDRALSURFACE",
        "TIN",
    ];
    for kw in &wkt_types {
        let with_open = format!("{}(", kw);
        if t.starts_with(&with_open) {
            let mut out = String::from(*kw);
            out.push(' ');
            out.push_str(&t[kw.len()..]);
            return out;
        }
        // Case-insensitive: PostGIS emits upper, but be generous.
        let lower_open = format!("{}(", kw.to_ascii_lowercase());
        if t.to_ascii_lowercase().starts_with(&lower_open) {
            let mut out = String::from(*kw);
            out.push(' ');
            out.push_str(&t[kw.len()..]);
            return out;
        }
    }
    t.to_string()
}

/// Heuristic: does a value look like binary/WKB hex output?
/// Long hex-only string starting with 01/00.
pub fn looks_like_binary(v: &str) -> bool {
    let t = v.trim();
    if t.len() < 40 {
        return false;
    }
    // hex has to be strictly [0-9a-fA-F] and even length.
    t.len() % 2 == 0 && t.chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bool_conv() {
        assert_eq!(normalise_psql_to_duckdb("t"), "true");
        assert_eq!(normalise_psql_to_duckdb("f"), "false");
    }

    #[test]
    fn wkt_space() {
        assert_eq!(
            normalise_psql_to_duckdb("POINT(1 2)"),
            "POINT (1 2)"
        );
    }

    #[test]
    fn detect_hex_wkb() {
        assert!(looks_like_binary(
            "0101000000000000000000F03F0000000000000040"
        ));
        assert!(!looks_like_binary("POINT(1 2)"));
    }
}
