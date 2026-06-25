-- ttext-ops: first bucket of the 1273-UDF expansion.
-- Exercises CSV construct, accessors, predicates, and string
-- transforms end-to-end through the bridge. Uses multi-line
-- string literals (both DuckDB and SQLite accept embedded
-- newlines inside single-quoted strings) so the case is
-- dialect-portable.

-- Constructor — CSV input parses every row (header + 2 instants).
SELECT CASE WHEN ttext_num_instants(
  ttext_from_csv('timestamp,value
0,Hello
1000000,World')
) = 2 THEN 1 ELSE 0 END;

-- Accessors
WITH seq AS (
  SELECT ttext_from_csv('timestamp,value
0,Hello
1000000,World') AS s
)
SELECT CASE WHEN ttext_num_instants(s) = 2
              AND ttext_start_value(s) = 'Hello'
              AND ttext_end_value(s) = 'World'
              AND ttext_value_at(s, 1000000) = 'World'
            THEN 1 ELSE 0 END FROM seq;

-- Predicates (ever / always)
WITH seq AS (
  SELECT ttext_from_csv('timestamp,value
0,Hello
1000000,Hello') AS s
)
SELECT CASE WHEN ttext_ever_eq(s, 'Hello') = true
              AND ttext_always_eq(s, 'Hello') = true
              AND ttext_ever_eq(s, 'World') = false
            THEN 1 ELSE 0 END FROM seq;

-- String transforms — verify upper / left / concat_str / prepend_str
WITH seq AS (
  SELECT ttext_from_csv('timestamp,value
0,Hello
1000000,World') AS s
)
SELECT CASE WHEN ttext_value_at(ttext_upper(s), 0) = 'HELLO'
              AND ttext_value_at(ttext_lower(s), 0) = 'hello'
              AND ttext_value_at(ttext_left(s, 3), 1000000) = 'Wor'
              AND ttext_value_at(ttext_concat_str(s, '!'), 0) = 'Hello!'
              AND ttext_value_at(ttext_prepend_str('>>', s), 0) = '>>Hello'
            THEN 1 ELSE 0 END FROM seq;
