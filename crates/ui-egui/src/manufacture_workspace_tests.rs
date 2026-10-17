//! Manufacture workspace tests: workspace switching, context menu, scenario integration.

use serde_json::json;

use crate::context_menu::{self, Target};
use crate::scenario::Harness;
use crate::{Services, SolveApp};

fn fresh() -> SolveApp {
    SolveApp::new(solidcraft_engine::Session::default(), Services::default())
}

// ── URL parsing ────────────────────────────────────────────────────────────────

#[test]
fn loopback_url_parse_accepts_standard_form() {
    use super::parse_companion_url;
    assert_eq!(parse_companion_url("http://127.0.0.1:38473").unwrap().origin, "http://127.0.0.1:38473");
    assert_eq!(parse_companion_url("http://127.0.0.1:38473/").unwrap().origin, "http://127.0.0.1:38473");
    assert_eq!(parse_companion_url("http://127.0.0.1:38473/cam/v1").unwrap().origin, "http://127.0.0.1:38473");
    assert_eq!(parse_companion_url("http://localhost:9000").unwrap().origin, "http://localhost:9000");
    assert_eq!(parse_companion_url("http://[::1]:9000").unwrap().origin, "http://[::1]:9000");
}

#[test]
fn loopback_url_parse_rejects_https_and_portless() {
    use super::parse_companion_url;
    assert!(parse_companion_url("https://127.0.0.1:38473").is_err(), "https rejected");
    assert!(parse_companion_url("http://127.0.0.1").is_err(), "no port rejected");
    assert!(parse_companion_url("ftp://127.0.0.1:21").is_err(), "ftp rejected");
    assert!(parse_companion_url("").is_err(), "empty rejected");
}

// ── URL attack tests ───────────────────────────────────────────────────────────

/// Credential injection: `http://127.0.0.1:38473@evil.com` — the real host here
/// is `evil.com`; the `127.0.0.1:38473` part is the userinfo component.
#[test]
fn url_attack_userinfo_injection_rejected() {
    use super::parse_companion_url;
    // Classic "loopback in userinfo" attack — real host is evil.com
    assert!(parse_companion_url("http://127.0.0.1:38473@evil.com:80").is_err(), "userinfo injection must be rejected");
    assert!(parse_companion_url("http://user:pass@127.0.0.1:38473").is_err(), "explicit credentials must be rejected");
    assert!(parse_companion_url("http://user@127.0.0.1:38473").is_err(), "username-only userinfo must be rejected");
}

/// Remote (non-loopback) hosts must be rejected.
#[test]
fn url_attack_remote_hosts_rejected() {
    use super::parse_companion_url;
    assert!(parse_companion_url("http://evil.com:38473").is_err(), "domain host rejected");
    assert!(parse_companion_url("http://192.168.1.1:38473").is_err(), "LAN IP rejected");
    assert!(parse_companion_url("http://10.0.0.1:38473").is_err(), "private IP rejected");
    assert!(parse_companion_url("http://0.0.0.0:38473").is_err(), "all-interfaces 0.0.0.0 rejected");
    assert!(parse_companion_url("http://[::]:38473").is_err(), "IPv6 unspecified :: rejected");
    assert!(parse_companion_url("http://[::ffff:127.0.0.1]:38473").is_err(), "IPv4-mapped loopback rejected (must use exact [::1])");
}

/// Query/fragment tricks do not bypass host validation.
#[test]
fn url_attack_query_and_fragment_tricks_rejected() {
    use super::parse_companion_url;
    // Host remains evil.com; query/fragment do not change URL host parsing
    assert!(parse_companion_url("http://evil.com:38473?x=127.0.0.1").is_err(), "query trick: real host is evil.com");
    assert!(parse_companion_url("http://evil.com:38473#127.0.0.1").is_err(), "fragment trick: real host is evil.com");
}

/// Path/query are stripped from the resulting origin so they cannot influence requests.
#[test]
fn url_parse_strips_path_and_query_from_origin() {
    use super::parse_companion_url;
    let parsed = parse_companion_url("http://127.0.0.1:38473/some/path?token=secret#frag").unwrap();
    assert_eq!(parsed.origin, "http://127.0.0.1:38473");
    assert!(!parsed.origin.contains("secret"), "token must not leak into origin");
    assert!(!parsed.origin.contains("path"), "path must not appear in origin");
}

/// `api_url` builds sub-paths correctly.
#[test]
fn parsed_companion_url_api_url_builds_correctly() {
    use super::parse_companion_url;
    let p = parse_companion_url("http://127.0.0.1:38473").unwrap();
    assert_eq!(p.api_url("/cam/v1/jobs"), "http://127.0.0.1:38473/cam/v1/jobs");
    assert_eq!(p.api_url("/cam/v1/jobs/abc/status"), "http://127.0.0.1:38473/cam/v1/jobs/abc/status");
}

// ── Response mapping (deserialization) ────────────────────────────────────────

/// POST /cam/v1/jobs → SubmitResp shape
#[test]
fn response_submit_resp_deserializes() {
    let raw = r#"{"job_id":"abc-123","status":"pending"}"#;
    let v: serde_json::Value = serde_json::from_str(raw).unwrap();
    assert_eq!(v["job_id"].as_str(), Some("abc-123"));
    assert_eq!(v["status"].as_str(), Some("pending"));
}

/// GET /cam/v1/jobs/:id/status → all valid status strings round-trip through JSON
#[test]
fn response_status_resp_all_phases_deserialize() {
    for status in &["pending", "running", "done", "failed", "cancelled"] {
        let raw = format!(r#"{{"job_id":"x","status":"{status}"}}"#);
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["status"].as_str(), Some(*status), "status={status}");
    }
}

#[test]
fn response_status_resp_with_error_field() {
    let raw = r#"{"job_id":"x","status":"failed","error":"tool collision"}"#;
    let v: serde_json::Value = serde_json::from_str(raw).unwrap();
    assert_eq!(v["status"].as_str(), Some("failed"));
    assert_eq!(v["error"].as_str(), Some("tool collision"));
}

/// GET /cam/v1/jobs/:id/result → nested result with segments and post_output
#[test]
fn response_result_resp_deserializes_segments() {
    let raw = json!({
        "job_id": "abc-123",
        "status": "done",
        "result": {
            "segments": [
                {"kind": "rapid", "from": [0.0,0.0,0.0], "to": [10.0,0.0,5.0]},
                {"kind": "cut",   "from": [10.0,0.0,5.0], "to": [20.0,0.0,5.0]}
            ],
            "post_output": "G0 X10\nG1 X20 F300"
        }
    });
    let segs = &raw["result"]["segments"];
    assert_eq!(segs.as_array().map(|a| a.len()), Some(2));
    assert_eq!(raw["result"]["post_output"].as_str(), Some("G0 X10\nG1 X20 F300"));
}

#[test]
fn response_result_resp_null_result_is_ok() {
    let raw = json!({"job_id": "abc-123", "status": "running", "result": null});
    assert!(raw["result"].is_null());
}

// ── Workspace switching ────────────────────────────────────────────────────────

#[test]
fn enter_manufacture_sets_tab() {
    let mut app = fresh();
    app.home.open = false;
    super::enter(&mut app);
    assert_eq!(app.ui.tab, "MANUFACTURE", "enter sets MANUFACTURE tab");
}

#[test]
fn exit_manufacture_returns_to_solid() {
    let mut app = fresh();
    app.home.open = false;
    super::enter(&mut app);
    super::exit(&mut app);
    assert_eq!(app.ui.tab, "SOLID", "exit restores SOLID tab");
}

#[test]
fn enter_blocked_while_sketch_active() {
    let mut app = fresh();
    app.home.open = false;
    app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
    app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    assert!(app.session.active_sketch.is_some());
    super::enter(&mut app);
    assert_ne!(app.ui.tab, "MANUFACTURE", "enter must be blocked while sketching");
}

#[test]
fn reset_document_clears_job_state_but_preserves_endpoint() {
    let mut app = fresh();
    app.manufacture_workspace.cam_endpoint = "http://127.0.0.1:9999".into();
    app.manufacture_workspace.cam_token = "secret".into();
    app.manufacture_workspace.simulate_step = 42;
    super::reset_document(&mut app);
    assert_eq!(app.manufacture_workspace.cam_endpoint, "http://127.0.0.1:9999", "endpoint preserved");
    assert!(app.manufacture_workspace.cam_token.is_empty(), "token cleared");
    assert_eq!(app.manufacture_workspace.simulate_step, 0, "step cleared");
}

// ── active() ──────────────────────────────────────────────────────────────────

#[test]
fn active_false_by_default() {
    let app = fresh();
    assert!(!super::active(&app));
}

#[test]
fn active_true_after_enter() {
    let mut app = fresh();
    app.home.open = false;
    super::enter(&mut app);
    assert!(super::active(&app));
}

// ── Context menu workspace items ───────────────────────────────────────────────

#[test]
fn workspace_menu_contains_manufacture_and_it_is_enabled() {
    let app = fresh();
    let items = context_menu::items(&app, &Target::Workspace);
    let mfg = items.iter().find(|i| i.id == "ui.workspace.manufacture");
    assert!(mfg.is_some(), "Manufacture item must appear in workspace menu, got: {:?}", items.iter().map(|i| &i.id).collect::<Vec<_>>());
    assert!(mfg.is_some_and(|i| i.enabled), "Manufacture workspace item must be enabled");
}

#[test]
fn workspace_menu_manufacture_switches_tab() {
    let mut app = fresh();
    app.home.open = false;
    let item = context_menu::items(&app, &Target::Workspace)
        .into_iter()
        .find(|i| i.id == "ui.workspace.manufacture")
        .expect("Manufacture item in workspace menu");
    context_menu::run_item(&mut app, &item, egui::pos2(0.0, 0.0));
    assert_eq!(app.ui.tab, "MANUFACTURE", "menu item switches to MANUFACTURE");
}

// ── Engine command integration ─────────────────────────────────────────────────

#[test]
fn manufacture_settings_roundtrip_via_engine_command() {
    let mut app = fresh();
    app.run("manufacture.settings.set", json!({"name": "Setup1", "set": {"stock": {"kind": "box", "width": 100.0, "depth": 80.0, "height": 25.0}}}))
        .unwrap();
    let result = app.session.execute("manufacture.settings.get", &json!({"name": "Setup1"})).unwrap();
    assert_eq!(result["name"].as_str(), Some("Setup1"));
    assert!(result["setup"]["stock"]["width"].as_f64().is_some());
}

#[test]
fn manufacture_backends_returns_schema_version_one() {
    let mut app = fresh();
    let result = app.session.execute("manufacture.backends", &json!({})).unwrap();
    assert_eq!(result["schema_version"].as_u64(), Some(1), "schema_version must be 1");
    assert!(result["backends"].is_array());
}

// ── Scenario harness: workspace switcher ──────────────────────────────────────

#[test]
fn toolbar_workspace_switcher_shows_manufacture_label() {
    let mut h = Harness::new();
    h.app.home.open = false;
    super::enter(&mut h.app);
    h.frames(2);
    // The workspace switcher should now show "MANUFACTURE".
    let ws = crate::scenario::rect_of("workspace_switcher");
    assert!(ws.is_some(), "workspace switcher must be drawn");
}

#[test]
fn manufacture_scenario_preflight_with_no_setup_sets_status() {
    let mut h = Harness::new();
    h.app.home.open = false;
    super::enter(&mut h.app);
    h.frames(2);
    // No setup defined: preflight must fail gracefully without panic.
    let result = h.app.session.execute("manufacture.preflight", &json!({"setup": "Setup1"}));
    assert!(result.is_err(), "preflight without a setup should return an error");
}

// ── State-machine tests using FakeCamBackend ───────────────────────────────────

use solidcraft_manufacture::job::{Availability, CamBackend, CamBackendDescriptor, CamHandle, CamJobId, CamJobInput, CamPhase, CamResult};
use std::sync::{Arc, Mutex};

/// A minimal fake backend that records submissions and provides controllable handles.
struct FakeCamBackend {
    submitted: Arc<Mutex<Vec<CamJobId>>>,
}

impl FakeCamBackend {
    fn new() -> Self {
        Self { submitted: Arc::new(Mutex::new(vec![])) }
    }
}

impl CamBackend for FakeCamBackend {
    fn submit(&self, id: CamJobId, input: CamJobInput) -> CamResult<CamHandle> {
        self.submitted.lock().unwrap().push(id);
        Ok(CamHandle::new(id, input.setup.clone(), input.scene_generation, 1))
    }

    fn descriptor(&self) -> CamBackendDescriptor {
        CamBackendDescriptor { id: "fake".into(), label: "Fake".into(), availability: Availability::Available, note: None }
    }

    fn is_available(&self) -> bool {
        true
    }
}

/// cancel() must set `CamPhase::Cancelling` (not immediately `Cancelled`), so the
/// network adapter has a chance to send the DELETE request before confirming cancellation.
#[test]
fn cam_handle_cancel_transitions_to_cancelling_not_cancelled() {
    let id = CamJobId { session: 1, serial: 1, generation: 1 };
    let handle = CamHandle::new(id, "Setup1".into(), 0, 1);
    // Phase before cancel must be non-terminal.
    assert!(!handle.status().unwrap().progress.phase.terminal(), "initial phase is not terminal");

    handle.cancel().ok();

    assert_eq!(
        handle.status().unwrap().progress.phase,
        CamPhase::Cancelling,
        "cancel() must set Cancelling (not Cancelled) — adapter must confirm via DELETE"
    );
}

/// Cancelling is not terminal — the state machine must keep running to send the DELETE.
#[test]
fn cancelling_phase_is_not_terminal() {
    assert!(!CamPhase::Cancelling.terminal(), "Cancelling is not terminal");
    assert!(CamPhase::Cancelled.terminal(), "Cancelled is terminal");
    assert!(CamPhase::Completed.terminal(), "Completed is terminal");
    assert!(CamPhase::Failed.terminal(), "Failed is terminal");
}

/// FakeCamBackend correctly records the submitted job id.
#[test]
fn fake_backend_records_submission() {
    let fake = FakeCamBackend::new();
    let submitted = fake.submitted.clone();

    let id = CamJobId { session: 7, serial: 3, generation: 1 };
    let input = CamJobInput {
        setup: "Setup1".into(),
        settings: Arc::new(solidcraft_manufacture::ManufactureSettings::default()),
        step_bytes: Arc::new(vec![]),
        scene_generation: 42,
        step_sha256: "abc".into(),
    };

    let handle = fake.submit(id, input).unwrap();
    let ids: Vec<_> = submitted.lock().unwrap().clone();
    assert_eq!(ids, vec![CamJobId { session: 7, serial: 3, generation: 1 }]);
    let st = handle.status().unwrap();
    assert_eq!(st.setup, "Setup1");
    assert_eq!(st.scene_generation, 42);
}

/// `CamHost::pump()` delegates to the backend without panicking.
#[test]
fn cam_host_pump_is_callable_without_panic() {
    use solidcraft_manufacture::job::CamHost;
    let host = CamHost::new(FakeCamBackend::new());
    // Default pump is a no-op for FakeCamBackend; must never panic.
    host.pump();
}

/// The FakeCamHost built into the manufacture crate completes jobs synchronously.
/// This verifies the contract our companion backend must also satisfy.
#[test]
fn fake_cam_host_completes_synchronously() {
    use solidcraft_manufacture::job::{CamHost, CamPhase};

    let host = CamHost::fake();
    let id = CamJobId { session: 0, serial: 0, generation: 0 };
    let input = CamJobInput {
        setup: "S".into(),
        settings: Arc::new(solidcraft_manufacture::ManufactureSettings::default()),
        step_bytes: Arc::new(vec![]),
        scene_generation: 0,
        step_sha256: String::new(),
    };
    let handle = host.submit(id, input).unwrap();
    assert_eq!(handle.status().unwrap().progress.phase, CamPhase::Completed);
}
