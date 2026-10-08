//! Designs saved by older SolveCraft builds (tests/corpus, see its README) open and evaluate as
//! they did when they were saved; damaged and hostile design files never panic.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use solvecraft_engine::Session;

fn corpus() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus");
    let mut v: Vec<PathBuf> =
        std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "solvecraft")).collect();
    v.sort();
    assert!(v.len() >= 7, "corpus files in {}", dir.display());
    v
}

fn open(path: &Path) -> Session {
    let mut s = Session::default();
    s.execute("doc.open", &json!({"path": path.to_string_lossy()})).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    s
}

/// Differences that are deliberate behaviour changes made before the file format had versions
/// (no migration can tell those files apart): (file, check, why).
const KNOWN: [(&str, &str, &str); 2] = [
    (
        "m0-revolve-primitives",
        "Body1 bbox",
        "partial revolves turn toward the sketch normal since ac43365 (as Fusion's do); M0 turned them the other way",
    ),
    ("components-arm", "body_count", "Measure counted only the root component's bodies then; it measures every placed body now"),
];

fn known(file: &str, check: &str) -> bool {
    KNOWN.iter().any(|(f, c, _)| *f == file && *c == check)
}

/// Volumes and areas then came from coarser tessellation (and curved booleans and fillets have
/// become exact since): the same part within this relative tolerance (the oracle's).
const REL: f64 = 1e-3;

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs().max(1.0)
}

#[test]
fn old_designs_open_and_evaluate_as_saved() {
    let mut problems = Vec::new();
    for p in corpus() {
        let name = p.file_stem().unwrap_or_default().to_string_lossy().to_string();
        let want: Value = serde_json::from_slice(&std::fs::read(p.with_extension("expected.json")).unwrap()).unwrap();
        let mut s = open(&p);
        for r in &s.model.results {
            if let Some(e) = &r.error {
                problems.push(format!("{name}: feature {}: {e}", r.id));
            }
        }
        assert_eq!(s.doc.format, solvecraft_engine::doc::format::format_string(solvecraft_engine::doc::format::FORMAT), "{name}: upgraded");
        let got = s.execute("MeasureCommand", &json!({})).unwrap();
        if got["body_count"] != want["body_count"] && !known(&name, "body_count") {
            problems.push(format!("{name}: {} bodies, saved with {}", got["body_count"], want["body_count"]));
        }
        for w in want["bodies"].as_array().into_iter().flatten() {
            let bname = w["name"].as_str().unwrap_or_default();
            let Some(g) = got["bodies"].as_array().into_iter().flatten().find(|g| g["name"] == w["name"]) else {
                problems.push(format!("{name}: body {bname} missing"));
                continue;
            };
            for k in ["volume_mm3", "area_mm2"] {
                let (a, b) = (g[k].as_f64().unwrap_or(f64::NAN), w[k].as_f64().unwrap_or(f64::NAN));
                if !close(a, b, REL) {
                    problems.push(format!("{name}: {bname} {k} {a} (saved {b})"));
                }
            }
            if known(&name, &format!("{bname} bbox")) {
                continue;
            }
            for (end, i) in [("min", 0), ("min", 1), ("min", 2), ("max", 0), ("max", 1), ("max", 2)] {
                let (a, b) = (g["bbox"][end][i].as_f64().unwrap_or(f64::NAN), w["bbox"][end][i].as_f64().unwrap_or(f64::NAN));
                if (a - b).abs() > 1e-3 * b.abs().max(1.0) {
                    problems.push(format!("{name}: {bname} bbox {end}[{i}] {a} (saved {b})"));
                }
            }
        }
        // Saved again, it reads back the same.
        let again = solvecraft_engine::doc::Document::from_json(&s.doc.to_json()).unwrap();
        assert_eq!(again, *s.doc, "{name}: round trip");
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// A small deterministic generator (xorshift).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

const HOSTILE: [&str; 14] =
    ["0", "-1", "1e308", "-1e308", "1e9", "4294967297", "\"\"", "\"1/0\"", "\"((((1\"", "null", "true", "[]", "{}", "\"nope\""];

/// Every JSON value in a document, as a path of keys and indices.
fn paths(v: &Value, at: &mut Vec<Value>, out: &mut Vec<Vec<Value>>) {
    out.push(at.clone());
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                at.push(json!(k));
                paths(x, at, out);
                at.pop();
            }
        }
        Value::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                at.push(json!(i));
                paths(x, at, out);
                at.pop();
            }
        }
        _ => {}
    }
}

fn at_mut<'a>(v: &'a mut Value, path: &[Value]) -> Option<&'a mut Value> {
    let mut cur = v;
    for k in path {
        cur = match k {
            Value::String(s) => cur.get_mut(s.as_str())?,
            Value::Number(n) => cur.get_mut(n.as_u64()? as usize)?,
            _ => return None,
        };
    }
    Some(cur)
}

/// Open a file's bytes as File → Open does and evaluate them; any panic is a failure.
fn try_bytes(bytes: &[u8]) {
    if let Ok(doc) = solvecraft_engine::io::read_design(bytes) {
        let mut s = Session::new(doc);
        let _ = s.execute("MeasureCommand", &json!({}));
        let _ = s.execute("document.inspect", &json!({}));
    }
}

#[test]
fn damaged_and_hostile_design_files_never_panic() {
    let mut rng = Rng(0x5eed_cafe_f00d_d00d);
    let mut failures = Vec::new();
    for p in corpus() {
        let name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
        let bytes = std::fs::read(&p).unwrap();
        let doc: Value = serde_json::from_slice(&bytes).unwrap();
        let mut all = Vec::new();
        paths(&doc, &mut Vec::new(), &mut all);
        let mut cases: Vec<(String, Vec<u8>)> = Vec::new();
        // Truncated and bit-flipped files.
        for _ in 0..8 {
            let cut = rng.below(bytes.len());
            cases.push((format!("truncated at {cut}"), bytes[..cut].to_vec()));
            let mut b = bytes.clone();
            let i = rng.below(b.len());
            b[i] ^= 1 << rng.below(8);
            cases.push((format!("bit flip at {i}"), b));
        }
        // Values replaced by hostile ones, and keys removed.
        for _ in 0..25 {
            let path = &all[rng.below(all.len())];
            let mut v = doc.clone();
            let h = HOSTILE[rng.below(HOSTILE.len())];
            if let Some(slot) = at_mut(&mut v, path) {
                *slot = serde_json::from_str(h).unwrap();
            }
            cases.push((format!("{path:?} = {h}"), v.to_string().into_bytes()));
            let mut v = doc.clone();
            if let Some((last, parent)) = path.split_last()
                && let (Some(Value::Object(m)), Value::String(k)) = (at_mut(&mut v, parent), last)
            {
                m.remove(k);
            }
            cases.push((format!("{path:?} removed"), v.to_string().into_bytes()));
        }
        // Features repeated many times; deep nesting; not JSON at all.
        let mut v = doc.clone();
        if let Some(Value::Array(fs)) = v.get_mut("features") {
            let copy = fs.clone();
            for _ in 0..2 {
                fs.extend(copy.iter().cloned());
            }
        }
        cases.push(("features repeated".into(), v.to_string().into_bytes()));
        cases.push(("deep nesting".into(), format!("{}{}", "[".repeat(100_000), "]".repeat(100_000)).into_bytes()));
        cases.push(("binary".into(), (0..4096u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8).collect()));
        for (what, b) in cases {
            if std::env::var_os("CORPUS_TRACE").is_some() {
                eprintln!("{name}: {what}");
            }
            if std::panic::catch_unwind(|| try_bytes(&b)).is_err() {
                failures.push(format!("{name}: {what}"));
            }
        }
    }
    assert!(failures.is_empty(), "panics:\n{}", failures.join("\n"));
}
