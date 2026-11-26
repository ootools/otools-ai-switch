-- A mask never stores the original value: that would defeat the feature.
-- Each row records the deterministic placeholder and the *kind* of value it
-- stood for, so an audit view can say what left the machine without becoming a
-- second copy of the secrets.
CREATE TABLE IF NOT EXISTS privacy_mask_audit (
  id TEXT PRIMARY KEY,
  occurred_at TEXT NOT NULL,
  platform TEXT NOT NULL,
  rule_kind TEXT NOT NULL,
  placeholder TEXT NOT NULL,
  direction TEXT NOT NULL,
  char_length INTEGER NOT NULL,
  prefix TEXT NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS idx_privacy_mask_audit_occurred_at
  ON privacy_mask_audit (occurred_at DESC);
