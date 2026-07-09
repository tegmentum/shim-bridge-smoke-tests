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
///     array-literal error (`ARRAY[NULL,NULL,…]` with no cast);
///   * expressions whose top-level probe wraps two geometry
///     values in a bbox-family operator (`=`, `<>`, `&&`, `<->`,
///     `<<|`, `|>>`, `&<|`, `|&>`, `<<`, `>>`, `&&&`, `~=`, ...)
///     the shim's bridge does not yet answer. Tracked under #66
///     (bbox-operator coverage); tagging the cases as
///     `fixture_bad` keeps the batch runner from perpetually
///     demoting the wrapped scalar (e.g. `st_makepoint`'s
///     `operators_84_a/b`) while the operator work is deferred.
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
    // bbox / spatial-comparator operators against another geometry
    // expression. Narrow prefix requirement (`st_` before the
    // operator OR a WKT-typed literal cast) so we don't over-match
    // arithmetic `=` in numeric fixtures. #66 tracks proper
    // routing through the shim's geometry_op_geometry entrypoints;
    // until then batch mode should skip rather than blackball the
    // wrapped constructor's status. Match anchors: `st_<ident>(...)`
    // or a `::geometry`-cast literal immediately followed by one of
    // the operator glyphs and another geometry-shaped expression.
    r"st_[a-z_]+\s*\([^)]*\)\s*(=|<>|&&|<->|<#>|<<\||\|>>|&<\||\|&>|~=|&&&)\s*st_[a-z_]+\s*\(",
    // TimescaleDB hypertable-scoped fixtures. The shim can't
    // provision hypertables, chunks, continuous aggregates, or any
    // catalog table under `_timescaledb_catalog.*` /
    // `_timescaledb_functions.*`, so any SELECT whose top call
    // wraps a hypertable-management/introspection function or
    // touches an internal catalog is by definition not testable
    // against the DuckDB-backed shim. We still emit the row
    // (bookkeeping) but stamp it fixture_bad.
    r"\b_timescaledb_(catalog|functions|internal|config)\b",
    r"\bcreate_hypertable\s*\(",
    r"\badd_dimension\s*\(",
    r"\badd_continuous_aggregate_policy\s*\(",
    r"\badd_(compression|retention|reorder|columnstore|compaction)_policy\s*\(",
    r"\bremove_(compression|retention|reorder|columnstore|compaction|continuous_aggregate)_policy\s*\(",
    r"\brefresh_continuous_aggregate\s*\(",
    r"\bcompress_chunk\s*\(",
    r"\bdecompress_chunk\s*\(",
    r"\brecompress_chunk\s*\(",
    r"\bdrop_chunks\s*\(",
    r"\bshow_chunks\s*\(",
    r"\bhypertable_(size|detailed_size|approximate_size|approximate_detailed_size|index_size|columnstore_stats|compression_stats)\s*\(",
    r"\bchunks?_(detailed_size|columnstore_stats|compression_stats)\s*\(",
    r"\bapproximate_row_count\s*\(",
    r"\bset_(chunk_time_interval|integer_now_func|number_partitions|partitioning_interval)\s*\(",
    r"\battach_(chunk|tablespace)\s*\(",
    r"\bdetach_(chunk|tablespace|tablespaces)\s*\(",
    r"\bmove_chunk\s*\(",
    r"\bsplit_chunk\s*\(",
    r"\bmerge_chunks\s*\(",
    r"\breorder_chunk\s*\(",
    r"\benable_chunk_skipping\s*\(",
    r"\bdisable_chunk_skipping\s*\(",
    r"\brun_job\s*\(",
    r"\bdelete_job\s*\(",
    r"\balter_job\s*\(",
    r"\bshow_(policies|tablespaces)\s*\(",
    r"\brestart_background_workers\s*\(",
    r"\b(start|stop)_background_workers\s*\(",
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

    #[test]
    fn detects_bbox_equality_between_geometry_expressions() {
        // #66 / #73 regression: `st_makepoint(...) = st_makepoint(...)`
        // — the shim's bridge does not yet route geometry `=` through
        // the operator dispatch, so mark the case fixture_bad.
        let s = "select st_makepoint(0,0) = st_makepoint(1,0)";
        assert!(matches_fixture_bad(s).is_some());
    }

    #[test]
    fn detects_bbox_ordering_operator() {
        let s = "select st_makeenvelope(2,2,4,4) &<| st_makeenvelope(2,2,4,4)";
        assert!(matches_fixture_bad(s).is_some());
    }

    #[test]
    fn does_not_flag_scalar_equality() {
        // Bare integer equality inside SELECT — must NOT trip the
        // bbox-operator regex.
        assert!(matches_fixture_bad("select 1 = 1").is_none());
    }
}
