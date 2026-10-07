//! Documents and files: new, open (designs and STEP), insert STEP, save, export.

use serde_json::{Value, json};
use solvecraft_doc::Document;
use solvecraft_io::Format;

use super::CommandSpec;
use crate::params::{bad, bool_, str_, string_list};
use crate::{EngineError, Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("NewDocumentCommand", "New Design", new_doc).icon("new").key("Ctrl+N").noundo().params("name?"),
    CommandSpec::new("doc.open", "Open", open)
        .icon("open")
        .key("Ctrl+O")
        .noundo()
        .params("path: .solvecraft design, or a .step/.stp file (opens as a new design)"),
    CommandSpec::new("FusionImportCommandFromToolbar", "Insert STEP", insert_step)
        .at("SOLID", "INSERT")
        .icon("import")
        .params("path: .step/.stp file (its bodies join the design as an Import base feature); name?"),
    CommandSpec::new("ParaMeshInsertAlignCommand", "Insert Mesh", insert_mesh)
        .at("SOLID", "INSERT")
        .icon("import")
        .params("path: .3mf or .stl file (its meshes join the design as mesh bodies); name?"),
    CommandSpec::new("SaveDocumentCommand", "Save", save).icon("save").key("Ctrl+S").noundo().params("path? (default: current file)"),
    CommandSpec::new("SaveDocumentAsCommand", "Save As", save_as).icon("save").noundo().params("path"),
    CommandSpec::new("ExportCommand", "Export", export)
        .icon("export")
        .noundo()
        .params("path; format?: stl|stla|obj|step|3mf (default from extension); bodies?: [names]"),
    CommandSpec::new("FusionSaveAsSTLCommand", "Save As Mesh", save_stl).icon("export").noundo().params("path; bodies?: [names]; ascii?: bool"),
];

fn new_doc(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_(p, "name").unwrap_or("Untitled");
    *s = Session::new(Document::new(name));
    Ok(json!({"name": name}))
}

fn path_arg<'a>(p: &'a Value, cmd: &str) -> Result<&'a str> {
    str_(p, "path").filter(|x| !x.trim().is_empty() && x.len() < 4096).ok_or_else(|| bad(cmd, "`path` is required"))
}

/// Read a STEP file as an Import feature.
fn read_step(path: &str) -> Result<solvecraft_io::StepFeature> {
    let meta = std::fs::metadata(path).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    if meta.len() > solvecraft_io::MAX_STEP_BYTES as u64 {
        return Err(EngineError::Other(format!("{path}: file too large")));
    }
    let bytes = std::fs::read(path).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    solvecraft_io::step_import_feature(&bytes, path).map_err(|e| EngineError::Other(format!("{path}: {e}")))
}

/// Add an Import feature for a STEP file to the session's design.
fn add_step(s: &mut Session, path: &str, name: Option<&str>) -> Result<Value> {
    let sf = read_step(path)?;
    let fname = name.filter(|n| !n.trim().is_empty()).unwrap_or(&sf.name).to_string();
    let id = s.doc_mut().add_feature(sf.kind, Some(&fname))?;
    if let Some(f) = s.doc_mut().feature_mut(id) {
        f.body_names = sf.body_names;
    }
    s.active_sketch = None;
    s.refresh();
    if let Some(e) = s.model.result(id).and_then(|r| r.error.clone()) {
        return Err(EngineError::Other(e));
    }
    let st = s.model.state();
    let bodies: Vec<&str> = st.bodies.iter().filter(|b| b.feature == id).map(|b| b.name.as_str()).collect();
    let fname = s.doc.feature(id).map(|f| f.name.clone()).unwrap_or_default();
    Ok(json!({"path": path, "feature": id, "name": fname, "bodies": bodies, "warnings": sf.warnings}))
}

/// Add a MeshImport feature for a 3MF or STL file.
fn add_mesh(s: &mut Session, path: &str, name: Option<&str>) -> Result<Value> {
    let meta = std::fs::metadata(path).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    if meta.len() > solvecraft_io::MAX_3MF_BYTES as u64 {
        return Err(EngineError::Other(format!("{path}: file too large")));
    }
    let bytes = std::fs::read(path).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    let mf = solvecraft_io::mesh_import_feature(&bytes, path).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    let fname = name.filter(|n| !n.trim().is_empty()).unwrap_or(&mf.name).to_string();
    let id = s.doc_mut().add_feature(mf.kind, Some(&fname))?;
    if let Some(f) = s.doc_mut().feature_mut(id) {
        f.body_names = mf.body_names;
    }
    s.active_sketch = None;
    s.refresh();
    if let Some(e) = s.model.result(id).and_then(|r| r.error.clone()) {
        return Err(EngineError::Other(e));
    }
    let st = s.model.state();
    let bodies: Vec<&str> = st.bodies.iter().filter(|b| b.feature == id).map(|b| b.name.as_str()).collect();
    let fname = s.doc.feature(id).map(|f| f.name.clone()).unwrap_or_default();
    Ok(json!({"path": path, "feature": id, "name": fname, "bodies": bodies, "warnings": mf.warnings}))
}

fn insert_mesh(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ParaMeshInsertAlignCommand";
    let path = path_arg(p, cmd)?;
    if !solvecraft_io::is_mesh_path(path) {
        return Err(bad(cmd, "only 3MF and STL files (.3mf, .stl) can be inserted as meshes"));
    }
    add_mesh(s, path, str_(p, "name"))
}

fn insert_step(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "FusionImportCommandFromToolbar";
    let path = path_arg(p, cmd)?;
    if !solvecraft_io::is_step_path(path) {
        return Err(bad(cmd, "only STEP files (.step, .stp) can be inserted"));
    }
    add_step(s, path, str_(p, "name"))
}

fn open(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_arg(p, "doc.open")?;
    if solvecraft_io::is_step_path(path) || solvecraft_io::is_mesh_path(path) {
        // A new, unsaved design holding the file's bodies.
        let stem = std::path::Path::new(path).file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_else(|| "Untitled".into());
        let mut fresh = Session::new(Document::new(&stem));
        let r = if solvecraft_io::is_step_path(path) { add_step(&mut fresh, path, None)? } else { add_mesh(&mut fresh, path, None)? };
        fresh.undo.clear();
        *s = fresh;
        let errors = s.model.results.iter().filter(|r| r.error.is_some()).count();
        return Ok(json!({"path": path, "features": s.doc.features.len(), "errors": errors, "bodies": r["bodies"], "warnings": r["warnings"]}));
    }
    let meta = std::fs::metadata(path).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    if meta.len() as usize > solvecraft_io::MAX_DESIGN_BYTES {
        return Err(EngineError::Other(format!("{path}: file too large")));
    }
    let bytes = std::fs::read(path).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    let doc = solvecraft_io::read_design(&bytes)?;
    *s = Session::new(doc);
    s.path = Some(path.to_string());
    let errors = s.model.results.iter().filter(|r| r.error.is_some()).count();
    Ok(json!({"path": path, "features": s.doc.features.len(), "errors": errors}))
}

fn write(s: &mut Session, path: &str) -> Result<Value> {
    let bytes = solvecraft_io::write_design(&s.doc);
    std::fs::write(path, &bytes).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    s.path = Some(path.to_string());
    s.mark_saved();
    Ok(json!({"path": path, "bytes": bytes.len()}))
}

fn save(s: &mut Session, p: &Value) -> Result<Value> {
    let path = match str_(p, "path") {
        Some(x) if !x.trim().is_empty() => x.to_string(),
        _ => s.path.clone().ok_or_else(|| bad("SaveDocumentCommand", "the design has no file yet: give a `path`"))?,
    };
    write(s, &path)
}

fn save_as(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_arg(p, "SaveDocumentAsCommand")?.to_string();
    write(s, &path)
}

fn export_to(s: &Session, path: &str, format: Format, bodies: &[String]) -> Result<Value> {
    let name = std::path::Path::new(path).file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_else(|| s.doc.name.clone());
    let bytes = solvecraft_io::export(&s.world_state(), bodies, format, &name)?;
    std::fs::write(path, &bytes).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    Ok(json!({"path": path, "bytes": bytes.len(), "format": format!("{format:?}")}))
}

fn export(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_arg(p, "ExportCommand")?;
    let format = Format::from_name(str_(p, "format").unwrap_or(path))?;
    export_to(s, path, format, &string_list(p, "bodies"))
}

fn save_stl(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_arg(p, "FusionSaveAsSTLCommand")?;
    let f = if bool_(p, "ascii").unwrap_or(false) { Format::StlAscii } else { Format::StlBinary };
    export_to(s, path, f, &string_list(p, "bodies"))
}
