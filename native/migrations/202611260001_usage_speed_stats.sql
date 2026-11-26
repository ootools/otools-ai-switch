-- The account list promotes `success` and `duration_ms` out of `metadata_json`
-- into real columns (see 202609270001). Speed statistics need one more scalar:
-- the instant the first real output token appeared. Without it, a request only
-- has a wall-clock duration, which cannot be split into time-to-first-token and
-- generation time, so output tokens/sec is not computable at all.
--
-- Additive and backfilling-only: legacy rows keep a NULL `ttft_ms`, which the
-- speed aggregates skip. `metadata_json` stays the source of truth and is left
-- untouched, so every other reader keeps working unchanged.
ALTER TABLE usage_events ADD COLUMN ttft_ms INTEGER;

-- `json_extract` yields a number when the proxy wrote it and NULL when it did
-- not (every row before this field existed). The LIKE guard keeps the rewrite
-- off rows that never carried the key, leaving them NULL exactly as a
-- pre-migration row is.
UPDATE usage_events
   SET ttft_ms = json_extract(metadata_json, '$.ttft_ms')
 WHERE metadata_json LIKE '%"ttft_ms"%';
