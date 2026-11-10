use otools_ai_switch_native::handle_json;
use serde_json::json;

#[test]
fn malformed_input_is_an_error_envelope() {
    let response = handle_json(b"{");
    assert_eq!(response["ok"], false);
    assert!(response["error"].is_string());
}

#[test]
fn event_poll_uses_host_object_envelope_before_initialization() {
    assert_eq!(
        handle_json(br#"{"method":"poll_events","payload":{}}"#),
        json!({"ok": true, "data": {"events": []}})
    );
}

#[test]
fn private_state_survives_shutdown_and_can_reinitialize() {
    let root =
        std::env::temp_dir().join(format!("otools-ai-switch-boundary-{}", std::process::id()));
    std::env::set_var("OTOOLS_AI_SWITCH_DATA_DIR", &root);
    for cycle in 0..2 {
        if cycle > 0 {
            let late = handle_json(br#"{"method":"get_settings"}"#);
            assert_eq!(
                late["ok"], false,
                "late calls must not resurrect an unloaded plugin"
            );
            assert_eq!(
                handle_json(br#"{"method":"__otools_initialize__"}"#)["ok"],
                true
            );
        }
        let settings = handle_json(br#"{"method":"get_settings"}"#);
        assert_eq!(settings["ok"], true, "{settings}");
        assert!(root.is_dir());
        assert_eq!(
            handle_json(br#"{"method":"list_route_credentials","payload":{"platform":"codex"}}"#)
                ["ok"],
            true
        );
        assert_eq!(
            handle_json(br#"{"method":"__otools_shutdown__"}"#)["ok"],
            true
        );
    }
    std::fs::remove_dir_all(root).unwrap();
    std::env::remove_var("OTOOLS_AI_SWITCH_DATA_DIR");
}

#[test]
fn shutdown_is_idempotent_without_initializing_a_runtime() {
    for _ in 0..2 {
        assert_eq!(
            handle_json(br#"{"method":"__otools_shutdown__"}"#)["ok"],
            true
        );
    }
}
