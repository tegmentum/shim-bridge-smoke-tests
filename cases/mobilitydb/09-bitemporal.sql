-- Bitemporal types: bucket 7 of the 1273-UDF expansion.
-- Wire format is `[value@[vs, ve)@[ts, te)]` — value followed by
-- valid-time period and transaction-time period.

-- bitemp_bool round-trip
SELECT CASE WHEN length(bitemporal_bool_to_text(
                  bitemporal_bool_from_text('[true@[0, 100]@[200, 300]]'))) > 10
            THEN 1 ELSE 0 END;

-- bitemp_int round-trip
SELECT CASE WHEN length(bitemporal_int_to_text(
                  bitemporal_int_from_text('[42@[0, 100]@[200, 300]]'))) > 10
            THEN 1 ELSE 0 END;

-- bitemp_float round-trip
SELECT CASE WHEN length(bitemporal_float_to_text(
                  bitemporal_float_from_text('[1.5@[0, 100]@[200, 300]]'))) > 10
            THEN 1 ELSE 0 END;

-- bitemp_text round-trip — quoted string value
SELECT CASE WHEN length(bitemporal_text_to_text(
                  bitemporal_text_from_text('["hello"@[0, 100]@[200, 300]]'))) > 10
            THEN 1 ELSE 0 END;
