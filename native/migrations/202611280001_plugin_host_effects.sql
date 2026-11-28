CREATE TABLE plugin_effect_receipts (
    account_id TEXT NOT NULL REFERENCES route_credentials(id) ON DELETE CASCADE,
    plugin_id TEXT NOT NULL,
    event_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (account_id, plugin_id, event_id)
);
CREATE TABLE plugin_scoped_state (
    account_id TEXT NOT NULL REFERENCES route_credentials(id) ON DELETE CASCADE,
    plugin_id TEXT NOT NULL,
    state_key TEXT NOT NULL,
    value_json TEXT NOT NULL,
    PRIMARY KEY (account_id, plugin_id, state_key)
);
