-- Token kinds are open names (ADR 0028): the managed price table may
-- price kinds beyond the first four, such as cache_write_1h. The service
-- checks each kind name before it stores a row (TokenKind::new), so the
-- old list goes. Dropping a check only loosens it; the version before
-- keeps working.
ALTER TABLE managed_price_rows DROP CONSTRAINT IF EXISTS managed_price_rows_kind_check;
