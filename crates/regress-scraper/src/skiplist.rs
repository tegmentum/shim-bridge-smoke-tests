//! Skip-list for pg-only statements that don't correspond to
//! shim-testable probes.

use regex::Regex;
use std::sync::OnceLock;

const PG_ONLY_PATTERNS: &[&str] = &[
    r"\bpg_catalog\b",
    r"\binformation_schema\b",
    r"\bpg_extension\b",
    r"\bpg_stats\b",
    r"\bpg_class\b",
    r"\bpg_typeof\s*\(",
    r"\bcurrent_setting\s*\(",
    r"\bset\s+client_min_messages\b",
    r"\bset\s+standard_conforming_strings\b",
    r"\bset\s+parallel_",
    r"\\gexec\b",
    r"\\copy\b",
    r"\\i\b",
    r"\\o\b",
    r"\bcreate\s+or\s+replace\s+function\b",
    r"\bdo\s+\$\$",
    r"\braise\s+notice\b",
    r"\blanguage\s+plpgsql\b",
];

static COMPILED: OnceLock<Vec<Regex>> = OnceLock::new();

fn compiled() -> &'static [Regex] {
    COMPILED
        .get_or_init(|| {
            PG_ONLY_PATTERNS
                .iter()
                .map(|p| Regex::new(p).expect("valid regex"))
                .collect()
        })
        .as_slice()
}

/// Returns the first matching pattern index (and its source), if
/// the (lowercased) statement text matches any pg-only pattern.
pub fn matches_pg_only(stmt_lc: &str) -> Option<&'static str> {
    for (i, r) in compiled().iter().enumerate() {
        if r.is_match(stmt_lc) {
            return Some(PG_ONLY_PATTERNS[i]);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_pg_catalog() {
        assert!(matches_pg_only("select * from pg_catalog.pg_proc").is_some());
    }

    #[test]
    fn clean_stmt() {
        assert!(matches_pg_only("select st_area(g) from t").is_none());
    }
}
