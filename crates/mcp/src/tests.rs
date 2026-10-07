//! An agent builds parts through MCP only and verifies them through MCP only.

use std::f64::consts::PI;

use serde_json::{Value, json};

use crate::{Headless, PROTOCOL_VERSION, Server, tool_definitions};

fn server() -> Server {
    Server::new(Box::new(Headless::default()))
}

fn call(s: &mut Server, id: u64, method: &str, params: Value) -> Value {
    let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string();
    let r = s.handle_line(&line).expect("reply");
    serde_json::from_str(&r).unwrap()
}

fn raw_tool(s: &mut Server, name: &str, args: Value) -> Value {
    call(s, 99, "tools/call", json!({"name": name, "arguments": args}))
}

/// Call a tool that must succeed; returns its JSON text (or the first content block).
fn tool(s: &mut Server, name: &str, args: Value) -> Value {
    let r = raw_tool(s, name, args);
    let c = &r["result"]["content"][0];
    assert_ne!(r["result"]["isError"], json!(true), "{name} failed: {c}");
    assert!(r.get("error").is_none(), "{name}: {r}");
    if c["type"] == "text" { serde_json::from_str(c["text"].as_str().unwrap()).unwrap_or(Value::Null) } else { c.clone() }
}

fn volume(s: &mut Server) -> f64 {
    tool(s, "measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap()
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-12)
}

#[test]
fn handshake_and_tools_list() {
    let mut s = server();
    let init =
        call(&mut s, 1, "initialize", json!({"protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}}));
    assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
    assert_eq!(init["result"]["serverInfo"]["name"], "solvecraft");
    assert!(init["result"]["capabilities"]["tools"].is_object());
    assert!(init["result"]["instructions"].as_str().unwrap().contains("headless"));
    // An unknown version gets ours.
    let init = call(&mut s, 2, "initialize", json!({"protocolVersion": "1999-01-01"}));
    assert_eq!(init["result"]["protocolVersion"], PROTOCOL_VERSION);
    assert!(s.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
    assert!(s.is_initialized());
    assert_eq!(call(&mut s, 3, "ping", json!({}))["result"], json!({}));

    let tools = call(&mut s, 4, "tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    for want in [
        "list_commands",
        "execute",
        "batch",
        "inspect_design",
        "measure",
        "body_topology",
        "set_parameter",
        "screenshot",
        "export",
        "undo",
        "redo",
        "new_design",
        "open",
        "save",
    ] {
        assert!(names.contains(&want), "{want} missing from {names:?}");
    }
    for t in tools["result"]["tools"].as_array().unwrap() {
        assert_eq!(t["inputSchema"]["type"], "object", "{t}");
        assert!(t["description"].as_str().unwrap().len() > 20, "{t}");
    }

    let cmds = tool(&mut s, "list_commands", json!({"filter": "fillet"}));
    assert!(cmds.as_array().unwrap().iter().any(|c| c["id"] == "FusionFilletEdgesCommand"), "{cmds}");
    assert!(cmds.as_array().unwrap().len() < 5);

    let res = call(&mut s, 5, "resources/list", json!({}));
    assert_eq!(res["result"]["resources"].as_array().unwrap().len(), 2);
    let d = call(&mut s, 6, "resources/read", json!({"uri": "solvecraft://design"}));
    assert!(d["result"]["contents"][0]["text"].as_str().unwrap().contains("timeline"));
    assert_eq!(call(&mut s, 7, "resources/read", json!({"uri": "solvecraft://nope"}))["error"]["code"], -32002);
    assert_eq!(call(&mut s, 8, "nope/nope", json!({}))["error"]["code"], -32601);
}

/// Box → fillet → hole in one batch, then verify with measure / inspect_design and look at it.
#[test]
fn agent_builds_box_fillet_hole() {
    let mut s = server();
    let r = tool(
        &mut s,
        "batch",
        json!({"commands": [
            {"command": "SketchCreate", "params": {"plane": "XY"}},
            {"command": "ShapeRectangleTwoPoint", "params": {"p0": [0, 0], "p1": [40, 30]}},
            {"command": "SketchStop"},
            {"command": "Extrude", "params": {"distance": 20}},
            {"command": "FusionFilletEdgesCommand", "params": {"edges": [[0, 0, 10]], "radius": 3}},
            {"command": "SketchCreate", "params": {"plane": "XY"}},
            {"command": "CircleCenterRadius", "params": {"center": [20, 15], "radius": 5}},
            {"command": "Extrude", "params": {"distance": 20, "operation": "cut"}},
        ]}),
    );
    assert_eq!(r["completed"], 8, "{r}");
    let expect = 24000.0 - (9.0 - PI * 9.0 / 4.0) * 20.0 - PI * 25.0 * 20.0;
    let v = volume(&mut s);
    assert!(rel(v, expect) < 5e-4, "{v} vs {expect}");
    let m = tool(&mut s, "measure", json!({"body": "Body1"}));
    // 6 box faces + fillet + hole wall (the kernel may keep the wall as two halves).
    assert!((8..=9).contains(&m["bodies"][0]["faces"].as_u64().unwrap()), "{m}");
    assert!(rel(m["bodies"][0]["bbox"]["max"][0].as_f64().unwrap(), 40.0) < 1e-6, "{m}");

    let d = tool(&mut s, "inspect_design", json!({}));
    assert_eq!(d["timeline"].as_array().unwrap().len(), 5, "{d}");
    assert!(d["timeline"].as_array().unwrap().iter().all(|f| f["error"].is_null()), "{d}");
    assert_eq!(d["sketches"][0]["curves"].as_array().unwrap().len(), 4, "{d}");
    assert!(d["sketches"][0]["status"].is_string(), "{d}");
    assert!(d["bodies"][0]["volume_mm3"].as_f64().unwrap() > 0.0);

    let topo = tool(&mut s, "body_topology", json!({"body": "Body1"}));
    assert!(topo["edges"].as_array().unwrap().len() >= 12, "{topo}");
    assert!(topo["faces"].as_array().unwrap().len() >= 8, "{topo}");

    // Undo the hole, redo it.
    tool(&mut s, "undo", json!({}));
    assert!(rel(volume(&mut s), 24000.0 - (9.0 - PI * 9.0 / 4.0) * 20.0) < 5e-4);
    tool(&mut s, "redo", json!({}));
    assert!(rel(volume(&mut s), expect) < 5e-4);

    // Look at it from the top: a real PNG comes back as an image block.
    let img = tool(&mut s, "screenshot", json!({"view": "top", "width": 320, "height": 200}));
    assert_eq!(img["type"], "image");
    assert_eq!(img["mimeType"], "image/png");
    assert!(img["data"].as_str().unwrap().starts_with("iVBORw0KGgo"), "not a PNG");

    // Export, save, start over, open.
    let dir = std::env::temp_dir().join(format!("solvecraft-mcp-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let step = dir.join("part.step");
    tool(&mut s, "export", json!({"path": step.to_string_lossy()}));
    assert!(std::fs::read_to_string(&step).unwrap().contains("ISO-10303-21"));
    let stl = dir.join("part.stl");
    tool(&mut s, "export", json!({"path": stl.to_string_lossy(), "format": "stl"}));
    assert!(std::fs::metadata(&stl).unwrap().len() > 1000);
    let shot = dir.join("shot.png");
    tool(&mut s, "screenshot", json!({"path": shot.to_string_lossy(), "width": 64, "height": 64}));
    assert!(std::fs::metadata(&shot).unwrap().len() > 100);
    let design = dir.join("part.solvecraft");
    tool(&mut s, "save", json!({"path": design.to_string_lossy()}));
    tool(&mut s, "new_design", json!({"name": "Empty"}));
    assert_eq!(tool(&mut s, "measure", json!({}))["body_count"], 0);
    tool(&mut s, "open", json!({"path": design.to_string_lossy()}));
    assert!(rel(volume(&mut s), expect) < 5e-4);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn parameters_drive_the_design() {
    let mut s = server();
    tool(&mut s, "set_parameter", json!({"name": "len", "value": "40 mm"}));
    tool(&mut s, "execute", json!({"command": "PrimitiveBox", "params": {"length": "len", "width": 30, "height": "len / 2", "corner": [0, 0, 0]}}));
    assert!(rel(volume(&mut s), 40.0 * 30.0 * 20.0) < 1e-9);
    let r = tool(&mut s, "set_parameter", json!({"name": "len", "value": 50}));
    assert_eq!(r["value"], 50.0, "{r}");
    assert_eq!(r["errors"].as_array().unwrap().len(), 0, "{r}");
    assert!(rel(volume(&mut s), 50.0 * 30.0 * 25.0) < 1e-9);
    let d = tool(&mut s, "inspect_design", json!({"measure": false, "sketches": false}));
    assert!(d["params"].as_array().unwrap().iter().any(|p| p["name"] == "len" && p["value"] == 50.0), "{d}");
}

#[test]
fn batch_stops_at_first_error_and_rolls_back() {
    let mut s = server();
    tool(&mut s, "execute", json!({"command": "PrimitiveBox", "params": {"length": 10, "width": 10, "height": 10}}));
    let r = raw_tool(
        &mut s,
        "batch",
        json!({"commands": [
            {"command": "PrimitiveSphere", "params": {"radius": 3, "center": [50, 0, 0]}},
            {"command": "Extrude", "params": {"distance": "bogus +"}},
            {"command": "PrimitiveSphere", "params": {"radius": 3, "center": [80, 0, 0]}},
        ]}),
    );
    assert_eq!(r["result"]["isError"], true, "{r}");
    let body: Value = serde_json::from_str(r["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(body["failed"]["index"], 1, "{body}");
    assert_eq!(body["failed"]["command"], "Extrude");
    assert_eq!(body["undone"], 1, "{body}");
    assert_eq!(body["results"].as_array().unwrap().len(), 1);
    // Only the first box is left, and its undo step is intact.
    assert_eq!(tool(&mut s, "measure", json!({}))["body_count"], 1);
    assert_eq!(tool(&mut s, "inspect_design", json!({"measure": false}))["undo"], 1);

    // Without rollback the completed steps stay.
    let r = raw_tool(
        &mut s,
        "batch",
        json!({"rollback": false, "commands": [{"command": "PrimitiveSphere", "params": {"radius": 3, "center": [50, 0, 0]}}, {"command": "NoSuchCommand"}]}),
    );
    assert_eq!(r["result"]["isError"], true);
    assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("NoSuchCommand"));
    assert_eq!(tool(&mut s, "measure", json!({}))["body_count"], 2);
}

#[test]
fn unknown_tools_and_bad_arguments_are_clean_errors() {
    let mut s = server();
    let r = raw_tool(&mut s, "make_coffee", json!({}));
    assert_eq!(r["error"]["code"], -32602, "{r}");
    assert!(r["error"]["message"].as_str().unwrap().contains("make_coffee"));
    assert_eq!(raw_tool(&mut s, "execute", json!({}))["error"]["code"], -32602);
    assert_eq!(raw_tool(&mut s, "execute", json!({"command": 5}))["error"]["code"], -32602);
    assert_eq!(raw_tool(&mut s, "measure", json!({"bogus": 1}))["error"]["code"], -32602);
    assert_eq!(raw_tool(&mut s, "screenshot", json!({"view": "sideways"}))["error"]["code"], -32602);
    assert_eq!(call(&mut s, 1, "tools/call", json!({"arguments": {}}))["error"]["code"], -32602);
    // Engine errors are tool errors the model can read.
    let r = raw_tool(&mut s, "execute", json!({"command": "Extrude", "params": {"distance": 10}}));
    assert_eq!(r["result"]["isError"], true, "{r}");
    let r = raw_tool(&mut s, "execute", json!({"command": "NoSuchCommand"}));
    assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("unknown command"));
    // Framing errors.
    assert!(s.handle_line("{not json").unwrap().contains("-32700"));
    assert!(s.handle_line("[]").unwrap().contains("-32600"));
    assert!(s.handle_line("42").unwrap().contains("-32600"));
    assert!(s.handle_line("   ").is_none());
    assert!(s.handle_line(r#"{"jsonrpc":"2.0","id":1,"result":{}}"#).is_none());
    let both = s.handle_line(r#"[{"jsonrpc":"2.0","id":1,"method":"ping"},{"jsonrpc":"2.0","method":"notifications/x"}]"#).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&both).unwrap().as_array().unwrap().len(), 1);
}

fn hostile_values() -> Vec<Value> {
    vec![
        Value::Null,
        json!(true),
        json!(-1),
        json!(0),
        json!(1e308),
        json!(-1e308),
        json!(""),
        json!("nope"),
        json!("../../../../../../nonexistent/dir/x.step"),
        json!("1/0"),
        json!([]),
        json!([1e308, -1e308]),
        json!([{"command": "Extrude", "params": {"distance": 1e308}}]),
        json!([{"command": 7}, null, "x"]),
        json!({}),
        json!({"x": 1}),
    ]
}

/// Every tool, every argument it documents, every hostile value: replies are well-formed
/// JSON-RPC and nothing panics.
#[test]
fn hostile_arguments_never_panic() {
    let mut s = server();
    tool(&mut s, "execute", json!({"command": "PrimitiveBox", "params": {"length": 10, "width": 10, "height": 10}}));
    let defs = tool_definitions();
    let mut n = 0;
    for def in &defs {
        let name = def["name"].as_str().unwrap();
        // Hostile strings are relative paths: don't let the sweep write files into the crate.
        let writes = matches!(name, "save" | "screenshot" | "export");
        let keys: Vec<String> = def["inputSchema"]["properties"].as_object().unwrap().keys().filter(|k| !(writes && *k == "path")).cloned().collect();
        for k in &keys {
            for v in hostile_values() {
                let mut args = json!({});
                // Fill the other required arguments with something plausible.
                for r in def["inputSchema"]["required"].as_array().into_iter().flatten() {
                    args[r.as_str().unwrap()] = json!("Body1");
                }
                args[k.as_str()] = v;
                if name == "screenshot" && (k == "width" || k == "height") {
                    args["view"] = json!("front");
                }
                let r = raw_tool(&mut s, name, args.clone());
                assert!(r.get("result").is_some() || r["error"]["code"].is_i64(), "{name} {args}: {r}");
                n += 1;
            }
        }
    }
    assert!(n > 100);
    // Execute every command with hostile params through MCP too.
    let cmds = tool(&mut s, "list_commands", json!({}));
    for c in cmds.as_array().unwrap() {
        let id = c["id"].as_str().unwrap();
        if matches!(id, "doc.open" | "SaveDocumentCommand" | "SaveDocumentAsCommand" | "ExportCommand" | "FusionSaveAsSTLCommand") {
            continue;
        }
        for v in hostile_values() {
            let r = raw_tool(&mut s, "execute", json!({"command": id, "params": {"distance": v.clone(), "edges": v.clone(), "plane": v}}));
            assert!(r.get("result").is_some(), "{id}: {r}");
        }
    }
    // Arguments that are not an object.
    for v in hostile_values().into_iter().filter(|v| !v.is_object() && !v.is_null()) {
        let r = call(&mut s, 1, "tools/call", json!({"name": "measure", "arguments": v}));
        assert_eq!(r["error"]["code"], -32602, "{r}");
    }
}

#[test]
fn base64_matches_rfc4648() {
    use crate::tools::base64;
    for (i, o) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")] {
        assert_eq!(base64(i.as_bytes()), o);
    }
}
