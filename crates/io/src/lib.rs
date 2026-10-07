//! File formats. Designs are saved as JSON (`.solvecraft`): the parametric document only, the
//! model is recomputed on load. Bodies export as STL (binary or ASCII), OBJ and STEP; STEP files
//! import as a base feature (see [`step_import_feature`]).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use solvecraft_doc::{Document, ModelState};
use solvecraft_geom::Mesh;

/// Design file extension.
pub const DESIGN_EXT: &str = "solvecraft";
/// Largest design file we read.
pub const MAX_DESIGN_BYTES: usize = 256 << 20;

#[derive(Debug, thiserror::Error)]
pub enum IoError {
    #[error("unsupported format `{0}` (use stl, obj, step or solvecraft)")]
    Format(String),
    #[error("nothing to export: the design has no bodies")]
    Empty,
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Kernel(#[from] solvecraft_kernel::KernelError),
    #[error(transparent)]
    Doc(#[from] solvecraft_doc::DocError),
}

pub type Result<T> = std::result::Result<T, IoError>;

/// Largest STEP file we read.
pub const MAX_STEP_BYTES: usize = 512 << 20;

/// Is this a STEP file name (`.step` / `.stp`, any case)?
pub fn is_step_path(path: &str) -> bool {
    std::path::Path::new(path).extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("step") || e.eq_ignore_ascii_case("stp"))
}

/// A STEP file read as an import feature.
pub struct StepFeature {
    /// Timeline name: the root product's name, else the file name.
    pub name: String,
    pub kind: solvecraft_doc::FeatureKind,
    /// Body names from the file (solid names, else product names).
    pub body_names: Vec<String>,
    pub warnings: Vec<String>,
}

/// Read STEP bytes (`file` is the source file name) into an Import base feature. The file is
/// read once here to fail early and collect names; the feature holds the STEP text.
pub fn step_import_feature(bytes: &[u8], file: &str) -> Result<StepFeature> {
    if bytes.len() > MAX_STEP_BYTES {
        return Err(IoError::Invalid(format!("STEP file too large ({} MB, limit {} MB)", bytes.len() >> 20, MAX_STEP_BYTES >> 20)));
    }
    // STEP is 7-bit text with escapes; tolerate stray 8-bit bytes.
    let text = String::from_utf8_lossy(bytes).into_owned();
    let imp = solvecraft_kernel::step_import_shared(&text)?;
    let stem = std::path::Path::new(file)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "Import".into());
    let generic = |n: &str| n.trim().is_empty() || n.starts_with('(') || n.eq_ignore_ascii_case("import") || n.eq_ignore_ascii_case("unnamed");
    // The root product's name; else the only body's name; else the file name.
    let name = match (imp.tree.as_slice(), imp.bodies.as_slice()) {
        ([root], _) if !generic(&root.name) => root.name.clone(),
        (_, [only]) if !generic(&only.name) => only.name.clone(),
        _ => stem,
    };
    let file_name = std::path::Path::new(file).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| file.to_string());
    Ok(StepFeature {
        name,
        body_names: imp.bodies.iter().map(|b| b.name.clone()).collect(),
        kind: solvecraft_doc::FeatureKind::Import { file: file_name, step: text, components: imp.tree.clone() },
        warnings: imp.warnings.clone(),
    })
}

/// Export formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    StlBinary,
    StlAscii,
    Obj,
    Step,
    Design,
}

impl Format {
    /// From a file name or extension (`part.stl`, `step`, `stp`…).
    pub fn from_name(name: &str) -> Result<Format> {
        let ext = name.rsplit('.').next().unwrap_or(name).to_ascii_lowercase();
        Ok(match ext.as_str() {
            "stl" => Format::StlBinary,
            "stla" | "stl-ascii" => Format::StlAscii,
            "obj" => Format::Obj,
            "step" | "stp" => Format::Step,
            "solvecraft" | "json" => Format::Design,
            _ => return Err(IoError::Format(ext)),
        })
    }
}

/// Fine export mesh for a body (chord tolerance 2e-4 of its size).
pub fn export_mesh(b: &solvecraft_kernel::Body) -> Result<Mesh> {
    Ok(b.tessellate((b.size() * 2e-4).max(1e-3))?)
}

/// Binary STL of several meshes (one solid).
pub fn stl_binary(meshes: &[Mesh], name: &str) -> Vec<u8> {
    let n: usize = meshes.iter().map(|m| m.triangles.len()).sum();
    let mut out = Vec::with_capacity(84 + n * 50);
    let mut header = format!("SolveCraft STL {name}").into_bytes();
    header.resize(80, b' ');
    out.extend_from_slice(&header);
    out.extend_from_slice(&u32::try_from(n).unwrap_or(u32::MAX).to_le_bytes());
    for m in meshes {
        for t in &m.triangles {
            let Some([a, b, c]) = m.tri(t) else { continue };
            let nrm = (b - a).cross(c - a).normalized().unwrap_or_default();
            for v in [nrm, a, b, c] {
                for x in v.to_f32() {
                    out.extend_from_slice(&x.to_le_bytes());
                }
            }
            out.extend_from_slice(&[0, 0]);
        }
    }
    out
}

/// ASCII STL.
pub fn stl_ascii(meshes: &[Mesh], name: &str) -> String {
    let name: String = name.chars().filter(|c| c.is_ascii_graphic()).collect();
    let mut s = format!("solid {name}\n");
    for m in meshes {
        for t in &m.triangles {
            let Some([a, b, c]) = m.tri(t) else { continue };
            let n = (b - a).cross(c - a).normalized().unwrap_or_default();
            s += &format!("  facet normal {:e} {:e} {:e}\n    outer loop\n", n.x, n.y, n.z);
            for v in [a, b, c] {
                s += &format!("      vertex {:e} {:e} {:e}\n", v.x, v.y, v.z);
            }
            s += "    endloop\n  endfacet\n";
        }
    }
    s += &format!("endsolid {name}\n");
    s
}

/// Wavefront OBJ, one object per body.
pub fn obj(meshes: &[(String, Mesh)]) -> String {
    let mut s = String::from("# SolveCraft OBJ export (mm)\n");
    let mut base = 1usize;
    for (name, m) in meshes {
        s += &format!("o {}\n", name.replace(char::is_whitespace, "_"));
        for p in &m.positions {
            s += &format!("v {} {} {}\n", p.x, p.y, p.z);
        }
        for n in &m.normals {
            s += &format!("vn {} {} {}\n", n.x, n.y, n.z);
        }
        for t in &m.triangles {
            let [a, b, c] = t.map(|i| i as usize + base);
            s += &format!("f {a}//{a} {b}//{b} {c}//{c}\n");
        }
        base += m.positions.len();
    }
    s
}

/// Export the given bodies of a model (all when `bodies` is empty) in a format.
pub fn export(state: &ModelState, bodies: &[String], format: Format, name: &str) -> Result<Vec<u8>> {
    let sel: Vec<&solvecraft_doc::ModelBody> = state.bodies.iter().filter(|b| bodies.is_empty() || bodies.contains(&b.name)).collect();
    if sel.is_empty() {
        return Err(IoError::Empty);
    }
    Ok(match format {
        Format::StlBinary | Format::StlAscii | Format::Obj => {
            let meshes: Vec<(String, Mesh)> = sel.iter().map(|b| Ok((b.name.clone(), export_mesh(&b.body)?))).collect::<Result<_>>()?;
            match format {
                Format::StlBinary => stl_binary(&meshes.into_iter().map(|(_, m)| m).collect::<Vec<_>>(), name),
                Format::StlAscii => stl_ascii(&meshes.into_iter().map(|(_, m)| m).collect::<Vec<_>>(), name).into_bytes(),
                _ => obj(&meshes).into_bytes(),
            }
        }
        Format::Step => {
            let bs: Vec<&solvecraft_kernel::Body> = sel.iter().map(|b| &b.body).collect();
            solvecraft_kernel::step_export(&bs, "SolveCraft")?.into_bytes()
        }
        Format::Design => return Err(IoError::Format("solvecraft (save the document instead)".into())),
    })
}

/// Read a design file's bytes.
pub fn read_design(bytes: &[u8]) -> Result<Document> {
    if bytes.len() > MAX_DESIGN_BYTES {
        return Err(IoError::Invalid("design file too large".into()));
    }
    let s = std::str::from_utf8(bytes).map_err(|_| IoError::Invalid("design file is not UTF-8 JSON".into()))?;
    Ok(Document::from_json(s)?)
}

/// Design file bytes.
pub fn write_design(doc: &Document) -> Vec<u8> {
    doc.to_json().into_bytes()
}

/// Parse a binary or ASCII STL (used by tests and round-trip checks): triangles.
pub fn read_stl(bytes: &[u8]) -> Result<Vec<[[f32; 3]; 3]>> {
    let ascii = bytes.starts_with(b"solid") && std::str::from_utf8(bytes).is_ok_and(|s| s.contains("facet"));
    let mut tris = Vec::new();
    if ascii {
        let s = std::str::from_utf8(bytes).map_err(|_| IoError::Invalid("bad STL".into()))?;
        let mut cur: Vec<[f32; 3]> = Vec::new();
        for l in s.lines() {
            let mut it = l.split_whitespace();
            if it.next() == Some("vertex") {
                let v: Vec<f32> = it.filter_map(|x| x.parse().ok()).collect();
                if let [x, y, z] = v[..] {
                    cur.push([x, y, z]);
                }
                if let [a, b, c] = cur[..] {
                    tris.push([a, b, c]);
                    cur.clear();
                }
            }
        }
        return Ok(tris);
    }
    let n = bytes.get(80..84).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize).ok_or_else(|| IoError::Invalid("short STL".into()))?;
    if bytes.len() < 84 + n.saturating_mul(50) {
        return Err(IoError::Invalid("truncated STL".into()));
    }
    let f = |o: usize| bytes.get(o..o + 4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).unwrap_or(0.0);
    for i in 0..n {
        let o = 84 + i * 50 + 12;
        let v = |k: usize| [f(o + k * 12), f(o + k * 12 + 4), f(o + k * 12 + 8)];
        tris.push([v(0), v(1), v(2)]);
    }
    Ok(tris)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solvecraft_doc::{FeatureKind, Model, Operation};
    use solvecraft_geom::Vec3;

    fn model() -> (Document, Model) {
        let mut doc = Document::new("t");
        doc.add_feature(
            FeatureKind::Box { corner: Vec3::ZERO, length: "10".into(), width: "20".into(), height: "30".into(), operation: Operation::NewBody },
            None,
        )
        .unwrap();
        let mut m = Model::new();
        m.evaluate(&doc);
        (doc, m)
    }

    #[test]
    fn stl_round_trip_volume() {
        let (_, m) = model();
        let bytes = export(&m.state(), &[], Format::StlBinary, "t").unwrap();
        let tris = read_stl(&bytes).unwrap();
        let v: f64 = tris
            .iter()
            .map(|t| {
                let p = |k: usize| Vec3::new(t[k][0] as f64, t[k][1] as f64, t[k][2] as f64);
                p(0).dot(p(1).cross(p(2))) / 6.0
            })
            .sum();
        assert!((v - 6000.0).abs() < 1e-2, "{v}");
        let a = export(&m.state(), &[], Format::StlAscii, "t").unwrap();
        assert_eq!(read_stl(&a).unwrap().len(), tris.len());
        let o = String::from_utf8(export(&m.state(), &[], Format::Obj, "t").unwrap()).unwrap();
        assert!(o.contains("o Body1") && o.contains("\nf "));
    }

    #[test]
    fn step_and_design() {
        let (doc, m) = model();
        let s = String::from_utf8(export(&m.state(), &[], Format::Step, "t").unwrap()).unwrap();
        assert!(s.starts_with("ISO-10303-21;") && s.contains("CLOSED_SHELL"));
        let back = read_design(&write_design(&doc)).unwrap();
        assert_eq!(back, doc);
        assert!(read_design(b"\xff\xfe").is_err());
        assert!(matches!(export(&m.state(), &["nope".into()], Format::Step, "t"), Err(IoError::Empty)));
        assert_eq!(Format::from_name("a.STP").unwrap(), Format::Step);
        assert!(is_step_path("/x/Part.STEP") && is_step_path("a.stp") && !is_step_path("a.step.json") && !is_step_path("step"));
        let f = step_import_feature(s.as_bytes(), "/tmp/My Part.step").unwrap();
        assert_eq!(f.name, "My Part");
        assert_eq!(f.body_names.len(), 1);
        assert!(matches!(&f.kind, FeatureKind::Import { file, .. } if file == "My Part.step"));
        assert!(step_import_feature(b"ISO-10303-21;", "x.step").is_err());
        assert!(step_import_feature(&[0xff, 0xfe, 0x00], "x.step").is_err());
        assert!(Format::from_name("a.dwg").is_err());
        assert!(read_stl(&[0u8; 10]).is_err());
        let mut fake = vec![0u8; 84];
        fake[80..84].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(read_stl(&fake).is_err());
    }
}
