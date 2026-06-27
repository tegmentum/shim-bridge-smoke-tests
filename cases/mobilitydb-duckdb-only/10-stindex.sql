-- Indexing: bucket 9 of the 1273-UDF expansion.
-- stindex_count_in_stbox takes a u32-prefixed blob of stindex
-- entries (4 + 65×N bytes) and an stbox; returns u32. We exercise
-- the empty-stindex shape end-to-end (sentinel-correctness, not
-- index correctness — that lives in upstream tests).

-- Empty stindex blob (u32 count = 0, no entries)
SELECT CASE WHEN stindex_count_in_stbox(
                  unhex('00000000'),
                  stbox_make(0.0, 0.0, 10.0, 10.0, 0, 1000000)
                ) = 0
            THEN 1 ELSE 0 END;
