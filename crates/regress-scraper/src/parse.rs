//! Regress-corpus SQL parser: strip comments, split on `;`,
//! extract top-level function calls.
//!
//! Regex-first per B2 §3. sqlparser-rs is not pulled in for the
//! MVP; PL/pgSQL blocks and psql meta-commands are skipped rather
//! than parsed.

use regex::Regex;

/// A single SQL statement extracted from a `.sql` file.
#[derive(Debug, Clone)]
pub struct Statement {
    pub start_line: usize,
    pub text: String,
    /// Original (pre-strip) text; the raw text keeps the string
    /// literals intact for downstream emission.
    pub raw: String,
}

/// One top-level function-of-interest call inside a SELECT.
#[derive(Debug, Clone)]
pub struct TopCall {
    pub function: String,
    /// Byte offset in the statement text where the call starts.
    pub start: usize,
}

/// Hand-rolled tokenizer that strips SQL comments while preserving
/// string literals. Handles `--` line comments and `/* ... */`
/// block comments. Skips comment detection inside `'...'` strings.
/// Dollar-quoted strings (`$$...$$`) are treated as opaque and
/// preserved verbatim -- statements containing them get skipped
/// downstream via the skip-list anyway.
pub fn strip_comments(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    let mut in_str = false;
    let mut in_dollar = false;
    let mut dollar_tag: Vec<u8> = Vec::new();
    while i < bytes.len() {
        let b = bytes[i];
        if in_dollar {
            // Look for closing $tag$
            if b == b'$' {
                // Try to match closing $tag$
                let close_len = 1 + dollar_tag.len() + 1;
                if i + close_len <= bytes.len()
                    && bytes[i + 1..i + 1 + dollar_tag.len()] == dollar_tag[..]
                    && bytes[i + close_len - 1] == b'$'
                {
                    out.push_str(std::str::from_utf8(&bytes[i..i + close_len]).unwrap_or(""));
                    i += close_len;
                    in_dollar = false;
                    dollar_tag.clear();
                    continue;
                }
            }
            out.push(b as char);
            i += 1;
            continue;
        }
        if in_str {
            // String state: only `'` ends it (with `''` escape).
            out.push(b as char);
            i += 1;
            if b == b'\'' {
                if i < bytes.len() && bytes[i] == b'\'' {
                    out.push('\'');
                    i += 1;
                } else {
                    in_str = false;
                }
            }
            continue;
        }
        // Not in a string. Check for dollar-quote start.
        if b == b'$' {
            // Try to lex a $tag$ opener. tag may be empty ($$).
            let mut j = i + 1;
            while j < bytes.len() {
                let cb = bytes[j];
                if cb == b'$' {
                    break;
                }
                if !cb.is_ascii_alphanumeric() && cb != b'_' {
                    break;
                }
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'$' {
                // Valid dollar-quote opener at i..=j.
                dollar_tag.clear();
                dollar_tag.extend_from_slice(&bytes[i + 1..j]);
                out.push_str(std::str::from_utf8(&bytes[i..=j]).unwrap_or(""));
                i = j + 1;
                in_dollar = true;
                continue;
            }
        }
        // Line comment
        if b == b'-' && i + 1 < bytes.len() && bytes[i + 1] == b'-' {
            // Skip to end of line, but keep the newline so line
            // numbers align.
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        // Block comment (non-nested)
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                // Preserve newlines for line-number alignment.
                if bytes[i] == b'\n' {
                    out.push('\n');
                }
                i += 1;
            }
            if i + 1 < bytes.len() {
                i += 2;
            }
            continue;
        }
        // Start of string
        if b == b'\'' {
            out.push('\'');
            in_str = true;
            i += 1;
            continue;
        }
        out.push(b as char);
        i += 1;
    }
    out
}

/// Split a stripped SQL blob into statements at top-level `;`.
/// Statement text has trailing whitespace trimmed. Line numbers
/// are 1-indexed into the original (pre-strip) file.
pub fn split_statements(stripped: &str) -> Vec<Statement> {
    let mut out = Vec::new();
    let bytes = stripped.as_bytes();
    let mut i = 0;
    let mut in_str = false;
    let mut in_dollar = false;
    let mut dollar_tag: Vec<u8> = Vec::new();
    let mut start = 0;
    let mut line = 1usize;
    let mut stmt_start_line = 1usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\n' {
            line += 1;
        }
        if in_dollar {
            if b == b'$' {
                let close_len = 1 + dollar_tag.len() + 1;
                if i + close_len <= bytes.len()
                    && bytes[i + 1..i + 1 + dollar_tag.len()] == dollar_tag[..]
                    && bytes[i + close_len - 1] == b'$'
                {
                    i += close_len;
                    in_dollar = false;
                    dollar_tag.clear();
                    continue;
                }
            }
            i += 1;
            continue;
        }
        if in_str {
            i += 1;
            if b == b'\'' {
                if i < bytes.len() && bytes[i] == b'\'' {
                    i += 1;
                } else {
                    in_str = false;
                }
            }
            continue;
        }
        if b == b'$' {
            let mut j = i + 1;
            while j < bytes.len() {
                let cb = bytes[j];
                if cb == b'$' {
                    break;
                }
                if !cb.is_ascii_alphanumeric() && cb != b'_' {
                    break;
                }
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'$' {
                dollar_tag.clear();
                dollar_tag.extend_from_slice(&bytes[i + 1..j]);
                i = j + 1;
                in_dollar = true;
                continue;
            }
        }
        if b == b'\'' {
            in_str = true;
            i += 1;
            continue;
        }
        if b == b';' {
            let text = stripped[start..i].trim();
            if !text.is_empty() {
                out.push(Statement {
                    start_line: stmt_start_line,
                    text: text.to_string(),
                    raw: text.to_string(),
                });
            }
            i += 1;
            start = i;
            // Advance stmt_start_line to the line of the next
            // non-whitespace char.
            let mut j = start;
            let mut ln = line;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                if bytes[j] == b'\n' {
                    ln += 1;
                }
                j += 1;
            }
            stmt_start_line = ln;
            continue;
        }
        i += 1;
    }
    if start < bytes.len() {
        let text = stripped[start..].trim();
        if !text.is_empty() {
            out.push(Statement {
                start_line: stmt_start_line,
                text: text.to_string(),
                raw: text.to_string(),
            });
        }
    }
    out
}

/// Extract top-level (paren-depth zero) function-of-interest calls
/// from a SELECT list. Returns calls in source order.
///
/// The `is_function_of_interest` predicate lets callers plug in
/// either a hardcoded prefix regex or a self-hosted inventory
/// lookup (design §8 self-hosting).
pub fn extract_top_calls(
    stmt: &str,
    is_function_of_interest: &dyn Fn(&str) -> bool,
) -> Vec<TopCall> {
    let bytes = stmt.as_bytes();
    // Find SELECT keyword (case-insensitive). If not present, no top calls.
    let lc = stmt.to_ascii_lowercase();
    let select_pos = find_keyword(&lc, "select");
    let Some(mut i) = select_pos else {
        return Vec::new();
    };
    i += "select".len();
    // Advance past DISTINCT if present.
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if lc[i..].starts_with("distinct") {
        i += "distinct".len();
    }
    let mut depth: i32 = 0;
    let mut out = Vec::new();
    let mut in_str = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_str {
            i += 1;
            if b == b'\'' {
                if i < bytes.len() && bytes[i] == b'\'' {
                    i += 1;
                } else {
                    in_str = false;
                }
            }
            continue;
        }
        if b == b'\'' {
            in_str = true;
            i += 1;
            continue;
        }
        if b == b'(' {
            depth += 1;
            i += 1;
            continue;
        }
        if b == b')' {
            depth -= 1;
            i += 1;
            continue;
        }
        // Stop at end of SELECT list -- FROM/WHERE/GROUP/ORDER/LIMIT/HAVING/etc.
        // at depth 0.
        if depth == 0 && is_word_boundary(bytes, i) {
            for kw in &["from ", "from\n", "from\t", "where ", "into ", ";"] {
                if lc[i..].starts_with(kw) {
                    return out;
                }
            }
        }
        // Try to lex an identifier at current position.
        if depth >= 0
            && (b.is_ascii_alphabetic() || b == b'_')
            && is_word_boundary(bytes, i)
        {
            let mut j = i + 1;
            while j < bytes.len()
                && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_')
            {
                j += 1;
            }
            let word = &lc[i..j];
            // Peek past whitespace for `(`.
            let mut k = j;
            while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                k += 1;
            }
            if k < bytes.len() && bytes[k] == b'(' && is_function_of_interest(word) {
                if depth == 0 {
                    out.push(TopCall {
                        function: word.to_string(),
                        start: i,
                    });
                }
                // Whether or not top-level, jump into the call --
                // the outer loop will do depth tracking from the
                // '(' onward. Just continue past the identifier.
            }
            i = j;
            continue;
        }
        i += 1;
    }
    out
}

fn is_word_boundary(bytes: &[u8], i: usize) -> bool {
    if i == 0 {
        return true;
    }
    let prev = bytes[i - 1];
    !(prev.is_ascii_alphanumeric() || prev == b'_')
}

fn find_keyword(lc: &str, kw: &str) -> Option<usize> {
    let mut i = 0;
    let bytes = lc.as_bytes();
    while let Some(pos) = lc[i..].find(kw) {
        let abs = i + pos;
        let ok_before = abs == 0
            || !(bytes[abs - 1].is_ascii_alphanumeric() || bytes[abs - 1] == b'_');
        let end = abs + kw.len();
        let ok_after = end >= bytes.len()
            || !(bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_');
        if ok_before && ok_after {
            return Some(abs);
        }
        i = abs + 1;
    }
    None
}

/// Attempt to parse a leading label literal in a SELECT list. Many
/// PostGIS regress SELECTs open with `SELECT '<label>', expr`; the
/// label is a hand-authored id used to align expected rows.
///
/// Returns `Some(label)` when the first item in the SELECT list is
/// a bare string literal.
pub fn extract_label(stmt: &str) -> Option<String> {
    let lc = stmt.to_ascii_lowercase();
    let mut i = find_keyword(&lc, "select")? + "select".len();
    let bytes = stmt.as_bytes();
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if i >= bytes.len() || bytes[i] != b'\'' {
        return None;
    }
    let mut j = i + 1;
    let mut buf = String::new();
    while j < bytes.len() {
        if bytes[j] == b'\'' {
            if j + 1 < bytes.len() && bytes[j + 1] == b'\'' {
                buf.push('\'');
                j += 2;
                continue;
            }
            // End of label. It must be immediately followed by `,`.
            let mut k = j + 1;
            while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                k += 1;
            }
            if k < bytes.len() && bytes[k] == b',' {
                return Some(buf);
            }
            return None;
        }
        buf.push(bytes[j] as char);
        j += 1;
    }
    None
}

/// If the SELECT list starts with a label literal (`SELECT 'foo',
/// expr`), rewrite the statement to drop the label -- otherwise
/// the label lands in the DuckDB CSV output as an extra column
/// that the shim-side comparator cannot skip. Returns the
/// rewritten statement text; falls back to the original if we
/// cannot confidently splice it.
pub fn strip_leading_label(stmt: &str) -> String {
    let lc = stmt.to_ascii_lowercase();
    let Some(sel) = find_keyword(&lc, "select") else {
        return stmt.to_string();
    };
    let after_sel = sel + "select".len();
    let bytes = stmt.as_bytes();
    let mut i = after_sel;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if i >= bytes.len() || bytes[i] != b'\'' {
        return stmt.to_string();
    }
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
        return stmt.to_string();
    }
    // Now expect optional whitespace, then a `,`.
    let mut k = j + 1;
    while k < bytes.len() && bytes[k].is_ascii_whitespace() {
        k += 1;
    }
    if k >= bytes.len() || bytes[k] != b',' {
        return stmt.to_string();
    }
    // Splice: `SELECT ` + tail-after-comma.
    let head = &stmt[..after_sel];
    let tail = &stmt[k + 1..];
    format!("{} {}", head.trim_end(), tail.trim_start())
}

/// Regex-based prefix predicate for functions of interest.
/// Fallback when the shim-interface DB has no `scalars.name`
/// inventory to self-host against.
pub fn default_prefix_predicate(extension: &str) -> Box<dyn Fn(&str) -> bool + Send + Sync> {
    // Blocklist -- SQL keywords / bare column-selector idents that
    // would otherwise match a prefix. Kept lowercased.
    let block: &[&'static str] =
        &["as", "and", "or", "not", "in", "on", "at", "asc", "desc", "any", "all", "avg"];
    let prefixes: Vec<&'static str> = match extension {
        "postgis" => vec!["st_", "postgis_", "geometry", "geography"],
        "mobilitydb" => vec![
            // temporal type prefixes
            "st_",
            "tgeom",
            "tpoint",
            "tint",
            "tfloat",
            "ttext",
            "tbool",
            "tgeog",
            "tnpoint",
            "tcbuffer",
            "tpose",
            // temporal / lifted arithmetic accessors
            "temporal",
            "duration",
            "num",
            "start",
            "end",
            "min",
            "max",
            "shift",
            "scale",
            // WKT/EWKT/text/binary encoders
            "astext",
            "aswkt",
            "asewkt",
            "asewkb",
            "asbinary",
            "asmfjson",
            "asgeojson",
            "asmvtgeom",
            "ashexewkb",
            // set/span/spanset constructors
            "set",
            "span",
            "spanset",
            "intset",
            "bigintset",
            "floatset",
            "textset",
            "dateset",
            "tstzset",
            "intspan",
            "floatspan",
            "datespan",
            "tstzspan",
            "intspanset",
            "floatspanset",
            "period",
            // range/at/minus accessors
            "at",
            "minus",
            "always",
            "ever",
            "value",
            "timestamp",
            "atgeometry",
        ],
        _ => vec![],
    };
    let block: Vec<&'static str> = block.to_vec();
    Box::new(move |ident: &str| {
        if block.contains(&ident) {
            return false;
        }
        prefixes.iter().any(|p| ident.starts_with(p))
    })
}

/// Predicate backed by an in-memory set of names (self-hosted).
pub fn inventory_predicate(names: std::collections::HashSet<String>) -> Box<dyn Fn(&str) -> bool + Send + Sync> {
    Box::new(move |ident: &str| names.contains(ident))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_line_comment() {
        let s = "SELECT 1; -- hello\nSELECT 2;";
        let out = strip_comments(s);
        assert!(out.contains("SELECT 1"));
        assert!(out.contains("SELECT 2"));
        assert!(!out.contains("hello"));
    }

    #[test]
    fn strip_block_comment() {
        let s = "SELECT /* block */ 1;";
        let out = strip_comments(s);
        assert!(!out.contains("block"));
    }

    #[test]
    fn preserve_str_literal() {
        let s = "SELECT '--not comment' AS x;";
        let out = strip_comments(s);
        assert!(out.contains("--not comment"));
    }

    #[test]
    fn split_two_stmts() {
        let s = "SELECT 1; SELECT 2;";
        let out = split_statements(s);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn top_call_basic() {
        let pred = default_prefix_predicate("postgis");
        let calls = extract_top_calls(
            "SELECT ST_MakePoint(1, 2), ST_Area(g) FROM t",
            &*pred,
        );
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].function, "st_makepoint");
        assert_eq!(calls[1].function, "st_area");
    }

    #[test]
    fn nested_no_top() {
        let pred = default_prefix_predicate("postgis");
        let calls = extract_top_calls(
            "SELECT ST_AsText(ST_MakePoint(1, 2))",
            &*pred,
        );
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function, "st_astext");
    }

    #[test]
    fn label_extract() {
        let l = extract_label("SELECT '113', ST_Area(g) FROM t");
        assert_eq!(l.as_deref(), Some("113"));
    }

    #[test]
    fn label_missing() {
        let l = extract_label("SELECT ST_Area(g) FROM t");
        assert_eq!(l, None);
    }
}
