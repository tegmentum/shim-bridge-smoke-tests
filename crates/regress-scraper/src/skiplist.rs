//! Skip-list for pg-only statements that don't correspond to
//! shim-testable probes, and a companion list of known-broken
//! upstream fixture patterns we tag as `fixture_bad` so coverage
//! reports can see the row was considered but can't be executed.

use regex::Regex;
use std::sync::OnceLock;

/// Known-broken upstream fixture patterns. Matches on the
/// lowercased statement body. When a statement matches, the
/// scraper still emits the row (so bookkeeping is honest) but
/// stamps it with `fixture_bad` + `fixture_bad_pattern:<pat>` tags
/// and the runner's batch mode will surface the same DuckDB error
/// every time (which is the point — we know the fixture is bad).
///
/// Patterns land here when they are:
///   * references to test-only tables the shim doesn't provision
///     (e.g. `asmarc21_rt` — only exists inside the postgis
///     regress harness);
///   * postgres-specific syntax DuckDB's parser rejects outright
///     (`(ST_Dump(geom)).*` composite-star expansion);
///   * expressions that trigger the DuckDB binder's untypeable
///     array-literal error (`ARRAY[NULL,NULL,…]` with no cast).
const FIXTURE_BAD_PATTERNS: &[&str] = &[
    // Regress-only helper table populated by `constructors.sql`;
    // absent in a fresh DuckDB `:memory:` session.
    r"\basmarc21_rt\b",
    // Composite `.*` expansion — `(ST_Dump(geom)).*` — DuckDB
    // parser rejects `).* ` outside a column list.
    r"\)\s*\.\s*\*",
    // Null-typed array literal. DuckDB refuses `ARRAY[NULL,NULL]`
    // without an element-type annotation. Common in the postgis
    // regress corpus's `ST_MakeLine(ARRAY[NULL,NULL,NULL,NULL])`.
    r"array\s*\[\s*null\s*,\s*null",
];

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
static COMPILED_FIXTURE_BAD: OnceLock<Vec<Regex>> = OnceLock::new();

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

fn compiled_fixture_bad() -> &'static [Regex] {
    COMPILED_FIXTURE_BAD
        .get_or_init(|| {
            FIXTURE_BAD_PATTERNS
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

/// Returns the first matching `fixture_bad` pattern source, if the
/// (lowercased) statement text matches any known-bad-fixture
/// pattern. Callers still emit the row but stamp it with
/// `fixture_bad` + `fixture_bad_pattern:<pat>` tags.
pub fn matches_fixture_bad(stmt_lc: &str) -> Option<&'static str> {
    for (i, r) in compiled_fixture_bad().iter().enumerate() {
        if r.is_match(stmt_lc) {
            return Some(FIXTURE_BAD_PATTERNS[i]);
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

    #[test]
    fn detects_asmarc21_rt_fixture() {
        let s = "select st_area(geom) from asmarc21_rt where gid = 1";
        assert!(matches_fixture_bad(s).is_some());
    }

    #[test]
    fn detects_composite_star_expansion() {
        let s = "select (st_dump(geom)).* from t";
        assert!(matches_fixture_bad(s).is_some());
    }

    #[test]
    fn detects_null_typed_array_literal() {
        let s = "select st_makeline(array[null,null,null,null])";
        assert!(matches_fixture_bad(s).is_some());
    }

    #[test]
    fn does_not_flag_clean_stmt_as_fixture_bad() {
        assert!(matches_fixture_bad("select st_area('point(1 2)'::geometry)").is_none());
    }
}
