//! The design file format version and the migrations that bring older files up to date.
//!
//! A design file starts with `"format": "solvecraft/N"`. A file of an older format is read and
//! upgraded step by step (N → N+1 → … → current): JSON-level steps first, for changes in the
//! file's shape, then document-level steps. A file of a newer format than this build knows is
//! refused with a message saying so, never guessed at.
//!
//! History:
//! - 1: every design up to 2026-10-08 (format string since M0). Fields added over time all
//!   have defaults; designs from before feature input names and before occurrences are filled
//!   in on upgrade.
//! - 2: the same shape; files say 2 once they have been upgraded, so later readers can tell.
//! - 3: appearances per body, face and component with names and opacity
//!   (`"appearances": {"bodies": {name: {name, color, opacity}}, "faces": […], "components": {…}}`);
//!   before, `"appearances"` mapped body names to bare `[r, g, b]` colours.
//! - 4: joint origins mate by default (B's z against A's, as Fusion does); `flip` now means
//!   "don't mate". Older joints keep their placement: their `flip` is inverted.

use serde_json::Value;

use crate::{DocError, Document, Result};

/// The format this build writes.
pub const FORMAT: u32 = 4;

pub fn format_string(n: u32) -> String {
    format!("solvecraft/{n}")
}

/// The version in a format string (`solvecraft/N`).
pub fn version_of(s: &str) -> Option<u32> {
    s.strip_prefix("solvecraft/")?.parse().ok()
}

/// JSON-level upgrades: entry `i` turns format `i + 1` into `i + 2`.
type JsonStep = fn(&mut Value) -> Result<()>;
const JSON_STEPS: [JsonStep; 3] = [|_| Ok(()), v2_to_v3, v3_to_v4];

/// Joints placed with the old convention (aligned unless flipped) keep their placement.
fn v3_to_v4(v: &mut Value) -> Result<()> {
    if let Some(Value::Array(js)) = v.get_mut("assembly").and_then(|a| a.get_mut("joints")) {
        for j in js.iter_mut() {
            if let Some(o) = j.as_object_mut() {
                let f = o.get("flip").and_then(Value::as_bool).unwrap_or(false);
                o.insert("flip".into(), Value::Bool(!f));
            }
        }
    }
    Ok(())
}

/// Body colours `{name: [r, g, b]}` become named, opaque body appearances.
fn v2_to_v3(v: &mut Value) -> Result<()> {
    let Some(Value::Object(old)) = v.get("appearances") else { return Ok(()) };
    if !old.values().all(Value::is_array) {
        return Ok(());
    }
    let bodies: serde_json::Map<String, Value> =
        old.iter().map(|(k, c)| (k.clone(), serde_json::json!({"name": "Custom", "color": c, "opacity": 1.0}))).collect();
    if let Some(o) = v.as_object_mut() {
        o.insert("appearances".into(), serde_json::json!({ "bodies": bodies }));
    }
    Ok(())
}

/// Read a design file's JSON, upgrading older formats.
pub fn read(s: &str) -> Result<Document> {
    let mut v: Value = serde_json::from_str(s).map_err(|e| DocError::Invalid(format!("not a SolveCraft design: {e}")))?;
    let Some(obj) = v.as_object() else {
        return Err(DocError::Invalid("not a SolveCraft design (expected a JSON object)".into()));
    };
    let version = match obj.get("format") {
        None => 1,
        Some(Value::String(f)) => version_of(f).ok_or_else(|| DocError::Invalid(format!("not a SolveCraft design (format `{f}`)")))?,
        Some(_) => return Err(DocError::Invalid("not a SolveCraft design (format is not a string)".into())),
    };
    if version == 0 {
        return Err(DocError::Invalid("not a SolveCraft design (format 0)".into()));
    }
    if version > FORMAT {
        return Err(DocError::Invalid(format!(
            "this design was saved by a newer SolveCraft (file format {version}; this version reads formats up to {FORMAT}): update SolveCraft to open it"
        )));
    }
    check_numbers(&v, 0)?;
    for step in JSON_STEPS.iter().skip(version as usize - 1).take((FORMAT - version) as usize) {
        step(&mut v)?;
    }
    let mut d: Document = serde_json::from_value(v).map_err(|e| DocError::Invalid(format!("design file: {e}")))?;
    if d.features.len() > crate::document::MAX_FEATURES
        || d.params.len() > crate::document::MAX_PARAMS
        || d.components.len() > crate::document::MAX_FEATURES
    {
        return Err(DocError::Invalid("document too large".into()));
    }
    for o in &d.occurrences {
        if !rigid(&o.transform) {
            return Err(DocError::Invalid(format!("damaged design: occurrence `{}` has an invalid placement", o.name)));
        }
    }
    // Fill in what older designs (and hand-edited files) may lack; a no-op otherwise.
    normalise(&mut d)?;
    d.format = format_string(FORMAT);
    Ok(d)
}

/// Largest coordinate or size a design may hold (mm): a kilometre-scale part is already far
/// outside what the kernel's tolerances serve.
const MAX_REAL: f64 = 1e9;

/// Every non-integer number in the file is finite and within ±`MAX_REAL` (a damaged or hostile
/// file is refused instead of overflowing the geometry later).
fn check_numbers(v: &Value, depth: usize) -> Result<()> {
    if depth > 64 {
        return Err(DocError::Invalid("damaged design: nested too deeply".into()));
    }
    match v {
        Value::Number(n) if !(n.is_u64() || n.is_i64()) => {
            let x = n.as_f64().unwrap_or(f64::NAN);
            if !(x.is_finite() && x.abs() <= MAX_REAL) {
                return Err(DocError::Invalid(format!("damaged design: the number {n} is out of range")));
            }
        }
        Value::Array(a) => a.iter().try_for_each(|x| check_numbers(x, depth + 1))?,
        Value::Object(m) => m.values().try_for_each(|x| check_numbers(x, depth + 1))?,
        _ => {}
    }
    Ok(())
}

/// A placement: rotation (orthonormal, not mirrored) and a translation in range.
fn rigid(m: &crate::Mat) -> bool {
    let row = |i: usize| m.get(i).copied().unwrap_or_default();
    let (x, y, z, t) = (row(0), row(1), row(2), row(3));
    let dot = |a: [f64; 4], b: [f64; 4]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let unit = |a: [f64; 4]| (dot(a, a) - 1.0).abs() < 1e-6;
    let det = x[0] * (y[1] * z[2] - y[2] * z[1]) - x[1] * (y[0] * z[2] - y[2] * z[0]) + x[2] * (y[0] * z[1] - y[1] * z[0]);
    m.iter().flatten().all(|v| v.is_finite())
        && unit(x)
        && unit(y)
        && unit(z)
        && dot(x, y).abs() < 1e-6
        && dot(y, z).abs() < 1e-6
        && dot(x, z).abs() < 1e-6
        && det > 0.0
        && [x[3], y[3], z[3]].iter().all(|v| v.abs() < 1e-9)
        && (t[3] - 1.0).abs() < 1e-9
        && t[..3].iter().all(|v| v.abs() <= MAX_REAL)
}

/// Feature input names and component occurrences, which format 1 files may predate.
fn normalise(d: &mut Document) -> Result<()> {
    let ids: Vec<u64> = d.features.iter().map(|f| f.id).collect();
    for id in ids {
        d.name_feature_inputs(id);
    }
    let missing: Vec<(u64, u64)> =
        d.components.iter().filter(|c| !d.occurrences.iter().any(|o| o.component == c.id)).map(|c| (c.id, c.parent)).collect();
    for (c, p) in missing {
        d.add_occurrence(c, p, crate::IDENTITY)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(version_of("solvecraft/1"), Some(1));
        assert_eq!(version_of("solvecraft/x"), None);
        let d = Document::new("A");
        assert_eq!(d.format, format_string(FORMAT));
        let back = read(&d.to_json()).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn older_upgraded_newer_refused_others_rejected() {
        let mut v: Value = serde_json::from_str(&Document::new("A").to_json()).unwrap();
        v["format"] = Value::from("solvecraft/1");
        assert_eq!(read(&v.to_string()).unwrap().format, format_string(FORMAT));
        v.as_object_mut().unwrap().remove("format");
        assert!(read(&v.to_string()).is_ok(), "a file without a format is format 1");
        v["format"] = Value::from(format_string(FORMAT + 1));
        let e = read(&v.to_string()).unwrap_err().to_string();
        assert!(e.contains("newer SolveCraft"), "{e}");
        for bad in ["solvecraft/0", "acad/3", "solvecraft/-1", "solvecraft/99999999999999999999"] {
            v["format"] = Value::from(bad);
            assert!(read(&v.to_string()).is_err(), "{bad}");
        }
        assert!(read("[]").is_err());
        // Format 2 body colours become body appearances.
        let mut old: Value = serde_json::from_str(&Document::new("A").to_json()).unwrap();
        old["format"] = Value::from("solvecraft/2");
        old["appearances"] = serde_json::json!({"Body1": [200, 10, 20]});
        let d = read(&old.to_string()).unwrap();
        let l = d.appearances.bodies.get("Body1").unwrap();
        // Format 3 joints (aligned unless flipped) keep their placement.
        let mut j: Value = serde_json::from_str(&Document::new("A").to_json()).unwrap();
        j["format"] = Value::from("solvecraft/3");
        j["assembly"] = serde_json::json!({"joints": [{"id": 1, "name": "J", "kind": "rigid", "a": {"occurrence": 0, "snap": {"type": "point", "point": [0, 0, 0]}}, "b": {"occurrence": 0, "snap": {"type": "point", "point": [0, 0, 0]}}}]});
        let dj = read(&j.to_string()).unwrap();
        assert!(dj.assembly.joints[0].flip, "aligned before = not mated now");
        assert_eq!((l.color, l.opacity, l.name.as_str()), ([200, 10, 20], 1.0, "Custom"));
        let mut v: Value = serde_json::from_str(&Document::new("A").to_json()).unwrap();
        v["huge"] = Value::from(1e300);
        assert!(read(&v.to_string()).is_err(), "out-of-range numbers anywhere");
        assert!(read("{\"format\": 2}").is_err());
    }
}
