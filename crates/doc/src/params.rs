//! Parameters: user parameters, model parameters of sketch dimensions, and feature inputs
//! (extrude distance, fillet radius…) which also appear as model parameters `dN` without being
//! stored twice: the feature keeps its expression and the parameter name refers to it. Every
//! parameter can be referenced by name from any expression; the dependency graph and cycles
//! come from the expression references.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::expr::{self, Kind, ParamDef, Value};
use crate::{DocError, Document, FeatureKind, HoleKind, MAX_PARAMS, PatternKind, PlaneRef, Result};

fn plane_inputs_mut<'a>(p: &'a mut PlaneRef, v: &mut Vec<(&'static str, &'a mut String, Kind)>) {
    match p {
        PlaneRef::Offset { base, distance } => {
            v.push(("Offset", distance, Kind::Length));
            plane_inputs_mut(base, v);
        }
        PlaneRef::AtAngle { base, angle, .. } => {
            v.push(("Angle", angle, Kind::Angle));
            plane_inputs_mut(base, v);
        }
        PlaneRef::AlongPath { t, .. } => v.push(("Distance (0 to 1)", t, Kind::Unitless)),
        _ => {}
    }
}

impl FeatureKind {
    /// The feature's numeric inputs: (label, expression, kind), in a fixed order.
    pub fn inputs_mut(&mut self) -> Vec<(&'static str, &mut String, Kind)> {
        use Kind::{Angle as A, Length as L, Unitless as U};
        let mut v: Vec<(&'static str, &mut String, Kind)> = Vec::new();
        match self {
            FeatureKind::Sketch { plane, .. }
            | FeatureKind::ConstructionPlane { plane }
            | FeatureKind::Split { plane, .. }
            | FeatureKind::SplitFace { plane, .. }
            | FeatureKind::Mirror { plane, .. } => plane_inputs_mut(plane, &mut v),
            FeatureKind::ConstructionAxis { def } => {
                for r in def.refs_mut() {
                    if let crate::construct::GeoRef::Plane { plane } = r {
                        plane_inputs_mut(plane, &mut v);
                    }
                }
            }
            FeatureKind::ConstructionPoint { def } => match def {
                crate::construct::PointDef::AlongPath { t, .. } => v.push(("Distance (0 to 1)", t, U)),
                other => {
                    for r in other.refs_mut() {
                        if let crate::construct::GeoRef::Plane { plane } = r {
                            plane_inputs_mut(plane, &mut v);
                        }
                    }
                }
            },
            FeatureKind::Extrude { extent, .. } => {
                v.push(("Distance", &mut extent.distance, L));
                if let Some(d) = &mut extent.distance2 {
                    v.push(("Distance 2", d, L));
                }
                if let Some(d) = &mut extent.start_offset {
                    v.push(("Start offset", d, L));
                }
                if let Some(d) = &mut extent.taper {
                    v.push(("Taper angle", d, A));
                }
            }
            FeatureKind::Revolve { angle, angle2, .. } => {
                v.push(("Angle", angle, A));
                if let Some(a) = angle2 {
                    v.push(("Angle 2", a, A));
                }
            }
            FeatureKind::Fillet { radius, style, .. } => match style {
                crate::FilletStyle::Constant => v.push(("Radius", radius, L)),
                crate::FilletStyle::Chord => v.push(("Chord length", radius, L)),
                crate::FilletStyle::Variable { radius2, .. } => {
                    v.push(("Start radius", radius, L));
                    v.push(("End radius", radius2, L));
                }
            },
            FeatureKind::Chamfer { distance, distance2, angle, .. } => {
                v.push(("Distance", distance, L));
                if let Some(d) = distance2 {
                    v.push(("Distance 2", d, L));
                }
                if let Some(a) = angle {
                    v.push(("Angle", a, A));
                }
            }
            FeatureKind::Box { length, width, height, .. } => {
                v.push(("Length", length, L));
                v.push(("Width", width, L));
                v.push(("Height", height, L));
            }
            FeatureKind::Cylinder { radius, height, .. } => {
                v.push(("Radius", radius, L));
                v.push(("Height", height, L));
            }
            FeatureKind::Sphere { radius, .. } => v.push(("Radius", radius, L)),
            FeatureKind::Torus { major, minor, .. } => {
                v.push(("Major radius", major, L));
                v.push(("Minor radius", minor, L));
            }
            FeatureKind::Pattern { pattern, .. } => match pattern {
                PatternKind::Rectangular { count1, spacing1, count2, spacing2, .. } => {
                    v.push(("Quantity", count1, U));
                    v.push(("Spacing", spacing1, L));
                    if let Some(c) = count2 {
                        v.push(("Quantity 2", c, U));
                    }
                    if let Some(sp) = spacing2 {
                        v.push(("Spacing 2", sp, L));
                    }
                }
                PatternKind::Path { count, spacing, .. } => {
                    v.push(("Quantity", count, U));
                    v.push(("Distance", spacing, L));
                }
                PatternKind::Circular { count, angle, .. } => {
                    v.push(("Quantity", count, U));
                    v.push(("Total angle", angle, A));
                }
            },
            FeatureKind::Shell { thickness, .. } => v.push(("Thickness", thickness, L)),
            FeatureKind::Draft { angle, neutral, .. } => {
                v.push(("Angle", angle, A));
                plane_inputs_mut(neutral, &mut v);
            }
            FeatureKind::Hole { diameter, depth, hole, .. } => {
                v.push(("Diameter", diameter, L));
                if let Some(d) = depth {
                    v.push(("Depth", d, L));
                }
                match hole {
                    HoleKind::Simple => {}
                    HoleKind::Drilled { tip_angle } => v.push(("Tip angle", tip_angle, A)),
                    HoleKind::Counterbore { cb_diameter, cb_depth } => {
                        v.push(("Counterbore diameter", cb_diameter, L));
                        v.push(("Counterbore depth", cb_depth, L));
                    }
                    HoleKind::Countersink { cs_diameter, cs_angle } => {
                        v.push(("Countersink diameter", cs_diameter, L));
                        v.push(("Countersink angle", cs_angle, A));
                    }
                }
            }
            FeatureKind::Thread { length, .. } => {
                if let Some(l) = length {
                    v.push(("Length", l, L));
                }
            }
            FeatureKind::Scale { factor, factors, .. } => {
                v.push(("Scale factor", factor, U));
                if let Some([x, y, z]) = factors {
                    v.push(("X scale", x, U));
                    v.push(("Y scale", y, U));
                    v.push(("Z scale", z, U));
                }
            }
            FeatureKind::OffsetFace { distance, .. } => v.push(("Distance", distance, L)),
            FeatureKind::BoundingSolid { margin, .. } => v.push(("Margin", margin, L)),
            FeatureKind::Pipe { diameter, wall, .. } => {
                v.push(("Diameter", diameter, L));
                if let Some(w) = wall {
                    v.push(("Wall", w, L));
                }
            }
            FeatureKind::Move { translate, angle, .. } => {
                let [x, y, z] = translate;
                v.push(("X distance", x, L));
                v.push(("Y distance", y, L));
                v.push(("Z distance", z, L));
                if let Some(a) = angle {
                    v.push(("Angle", a, A));
                }
            }
            FeatureKind::Emboss { depth, .. } => v.push(("Depth", depth, L)),
            FeatureKind::Boss { diameter, height, hole_diameter, hole_depth, draft, fillet, ribs, rib_thickness, rib_length, rib_offset, .. } => {
                v.push(("Diameter", diameter, L));
                v.push(("Height", height, L));
                for (l, o, k) in [
                    ("Hole diameter", hole_diameter, L),
                    ("Hole depth", hole_depth, L),
                    ("Draft angle", draft, A),
                    ("Root fillet", fillet, L),
                    ("Ribs", ribs, U),
                    ("Rib thickness", rib_thickness, L),
                    ("Rib length", rib_length, L),
                    ("Rib offset", rib_offset, L),
                ] {
                    if let Some(e) = o {
                        v.push((l, e, k));
                    }
                }
            }
            FeatureKind::Lip { width, height, gap, .. } => {
                v.push(("Width", width, L));
                v.push(("Height", height, L));
                if let Some(g) = gap {
                    v.push(("Gap", g, L));
                }
            }
            FeatureKind::Rest { width, length, height, draft, thickness, .. } => {
                v.push(("Width", width, L));
                v.push(("Height", height, L));
                for (l, o, k) in [("Length", length, L), ("Draft angle", draft, A), ("Wall thickness", thickness, L)] {
                    if let Some(e) = o {
                        v.push((l, e, k));
                    }
                }
            }
            FeatureKind::SnapFit { length, thickness, width, catch_depth, catch_length, .. } => {
                v.push(("Length", length, L));
                v.push(("Thickness", thickness, L));
                v.push(("Width", width, L));
                v.push(("Catch depth", catch_depth, L));
                v.push(("Catch length", catch_length, L));
            }
            FeatureKind::SheetContour { distance, .. } => v.push(("Distance", distance, L)),
            FeatureKind::SheetFlange { height, angle, radius, .. } => {
                v.push(("Height", height, L));
                v.push(("Angle", angle, A));
                if let Some(r) = radius {
                    v.push(("Bend radius", r, L));
                }
            }
            FeatureKind::SheetFold { angle, radius, .. } => {
                v.push(("Angle", angle, A));
                if let Some(r) = radius {
                    v.push(("Bend radius", r, L));
                }
            }
            FeatureKind::SheetHem { length, gap, .. } => {
                v.push(("Length", length, L));
                if let Some(g) = gap {
                    v.push(("Gap", g, L));
                }
            }
            FeatureKind::Coil { diameter, pitch, turns, section_size, start_angle, .. } => {
                v.push(("Diameter", diameter, L));
                v.push(("Pitch", pitch, L));
                v.push(("Revolutions", turns, U));
                v.push(("Section size", section_size, L));
                if let Some(a) = start_angle {
                    v.push(("Start angle", a, A));
                }
            }
            FeatureKind::Rib { thickness, depth, .. } => {
                v.push(("Thickness", thickness, L));
                if let Some(d) = depth {
                    v.push(("Depth", d, L));
                }
            }
            FeatureKind::ReplaceFace { target, .. } => plane_inputs_mut(target, &mut v),
            FeatureKind::Stitch { tolerance, .. } => v.push(("Tolerance", tolerance, L)),
            FeatureKind::Thicken { thickness, .. } => v.push(("Thickness", thickness, L)),
            FeatureKind::SurfaceTrim { plane, .. } => plane_inputs_mut(plane, &mut v),
            FeatureKind::SurfaceExtend { distance, .. } => v.push(("Distance", distance, L)),
            FeatureKind::Patch { .. } => {}
            FeatureKind::Combine { .. }
            | FeatureKind::SheetBase { .. }
            | FeatureKind::SheetUnfold { .. }
            | FeatureKind::SheetConvert { .. }
            | FeatureKind::Align { .. }
            | FeatureKind::Remove { .. }
            | FeatureKind::DeleteFace { .. }
            | FeatureKind::BoundaryFill { .. }
            | FeatureKind::Loft { .. }
            | FeatureKind::Sweep { .. }
            | FeatureKind::Import { .. }
            | FeatureKind::MeshImport { .. } => {}
        }
        v
    }

    /// Read-only view of [`FeatureKind::inputs_mut`]: (label, expression, kind).
    pub fn inputs(&self) -> Vec<(&'static str, String, Kind)> {
        // Imports have no inputs (and their STEP text is large: don't clone it).
        if matches!(self, FeatureKind::Import { .. } | FeatureKind::MeshImport { .. }) {
            return Vec::new();
        }
        let mut c = self.clone();
        c.inputs_mut().into_iter().map(|(l, e, k)| (l, e.clone(), k)).collect()
    }
}

/// One row of the parameters table.
#[derive(Clone, Debug, Serialize)]
pub struct ParamRow {
    pub name: String,
    pub expression: String,
    pub unit: String,
    pub value: Option<f64>,
    pub error: Option<String>,
    pub comment: String,
    /// "user", "sketch" (a dimension) or "feature".
    pub source: &'static str,
    /// For feature inputs: the feature and the input's label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<&'static str>,
    pub favorite: bool,
    /// Names this one's expression uses, and names whose expressions use this one.
    pub uses: Vec<String>,
    pub used_by: Vec<String>,
}

fn unit_of(k: Kind) -> &'static str {
    match k {
        Kind::Length => "mm",
        Kind::Angle => "deg",
        Kind::Unitless => "",
    }
}

impl Document {
    /// Every (name, expression, kind) the expressions can refer to: stored parameters and the
    /// named feature inputs.
    pub fn all_param_exprs(&self) -> Vec<(String, String, Kind)> {
        let mut v: Vec<(String, String, Kind)> = self.params.iter().map(|p| (p.name.clone(), p.expr.clone(), p.kind())).collect();
        for f in &self.features {
            for (name, (_, e, k)) in f.param_names.iter().zip(f.kind.inputs()) {
                if !name.is_empty() {
                    v.push((name.clone(), e, k));
                }
            }
        }
        v
    }

    fn name_taken(&self, n: &str) -> bool {
        self.param(n).is_some() || self.features.iter().any(|f| f.param_names.iter().any(|x| x == n))
    }

    /// The next free `dN` name.
    pub fn next_d_name(&self) -> String {
        let mut n = 1;
        loop {
            let c = format!("d{n}");
            if !self.name_taken(&c) {
                return c;
            }
            n += 1;
        }
    }

    /// Give a feature's inputs parameter names (keeping existing names in place).
    pub fn name_feature_inputs(&mut self, id: u64) {
        let Some(idx) = self.feature_index(id) else { return };
        let n = self.features.get(idx).map(|f| f.kind.inputs().len()).unwrap_or(0);
        for k in 0..n {
            let have = self.features.get(idx).and_then(|f| f.param_names.get(k)).is_some_and(|x| !x.is_empty());
            if have {
                continue;
            }
            let name = self.next_d_name();
            if let Some(f) = self.features.get_mut(idx) {
                while f.param_names.len() <= k {
                    f.param_names.push(String::new());
                }
                if let Some(slot) = f.param_names.get_mut(k) {
                    *slot = name;
                }
            }
        }
        if let Some(f) = self.features.get_mut(idx) {
            f.param_names.truncate(n);
        }
    }

    /// Change a parameter: a stored one, or a feature input by its name.
    /// A typed length that is a bare number means the design's units: it gets that unit written
    /// in (feature inputs are stored in mm otherwise). Anything else is returned unchanged.
    pub fn with_design_unit(&self, e: &str, kind: Kind) -> String {
        let u = self.units.trim();
        let bare = expr::is_literal(e) && e.trim().trim_start_matches(['-', '+']).trim().parse::<f64>().is_ok();
        if kind == Kind::Length && bare && !u.is_empty() && u != "mm" && expr::unit_info(u).is_some_and(|(k, _)| k == Kind::Length) {
            format!("{} {u}", e.trim())
        } else {
            e.to_string()
        }
    }

    /// Every length input of every feature as the geometry reads it (mm; a unit-less result is
    /// in the design's units), for checking that an edit moves nothing.
    pub fn length_inputs(&self) -> Vec<Option<f64>> {
        let (vals, _) = self.param_values();
        let scale = expr::unit_info(self.units.trim()).filter(|(k, _)| *k == Kind::Length).map(|(_, s)| s).unwrap_or(1.0);
        let look = |n: &str| vals.get(n).copied().ok_or_else(|| DocError::Expr(format!("unknown parameter `{n}`")));
        self.features
            .iter()
            .flat_map(|f| f.kind.inputs())
            .filter(|(_, _, k)| *k == Kind::Length)
            .map(|(_, e, _)| {
                let d = if expr::is_literal(&e) { expr::Defaults::default() } else { expr::Defaults { len: scale, ..expr::Defaults::default() } };
                expr::eval_with(&e, &look).and_then(|v| v.to_kind_in(Kind::Length, d)).ok()
            })
            .collect()
    }

    pub fn change_param(&mut self, name: &str, expr_s: &str, unit: Option<&str>, comment: Option<&str>) -> Result<()> {
        let hit = self.features.iter().enumerate().find_map(|(i, f)| f.param_names.iter().position(|n| n == name).map(|k| (i, k)));
        let Some((i, k)) = hit else { return self.set_param(name, expr_s, unit, comment) };
        if expr_s.len() > 4096 {
            return Err(DocError::Expr("expression too long".into()));
        }
        let mut next = self.clone();
        let kind = self.features.get(i).and_then(|f| f.kind.inputs().get(k).map(|x| x.2)).unwrap_or(Kind::Unitless);
        let expr_s = self.with_design_unit(expr_s, kind);
        if let Some(f) = next.features.get_mut(i)
            && let Some((_, e, _)) = f.kind.inputs_mut().into_iter().nth(k)
        {
            *e = expr_s.clone();
        }
        if let Some(c) = comment {
            next.param_comments.insert(name.to_string(), c.to_string());
        }
        let (_, errors) = next.param_values();
        if let Some(e) = errors.get(name) {
            return Err(DocError::Expr(e.clone()));
        }
        *self = next;
        Ok(())
    }

    /// The parameters table (user, sketch dimensions, feature inputs).
    pub fn param_rows(&self) -> Vec<ParamRow> {
        let (vals, errs) = self.param_values();
        let all = self.all_param_exprs();
        let refs: BTreeMap<String, Vec<String>> = all.iter().map(|(n, e, _)| (n.clone(), expr::references(e))).collect();
        let used_by = |n: &str| -> Vec<String> { refs.iter().filter(|(_, r)| r.iter().any(|x| x == n)).map(|(k, _)| k.clone()).collect() };
        let value = |n: &str, k: Kind| vals.get(n).and_then(|v| v.to_kind(k).ok()).map(|x| if k == Kind::Angle { x.to_degrees() } else { x });
        let mut rows = Vec::new();
        for p in &self.params {
            rows.push(ParamRow {
                name: p.name.clone(),
                expression: p.expr.clone(),
                unit: p.unit.clone(),
                value: self.param_display_value(&vals, &p.name),
                error: errs.get(&p.name).cloned(),
                comment: p.comment.clone(),
                source: if p.model { "sketch" } else { "user" },
                feature: None,
                input: None,
                favorite: self.favorites.contains(&p.name),
                uses: refs.get(&p.name).cloned().unwrap_or_default(),
                used_by: used_by(&p.name),
            });
        }
        for f in &self.features {
            for (name, (label, e, k)) in f.param_names.iter().zip(f.kind.inputs()) {
                if name.is_empty() {
                    continue;
                }
                rows.push(ParamRow {
                    name: name.clone(),
                    expression: e,
                    unit: unit_of(k).into(),
                    value: value(name, k),
                    error: errs.get(name).cloned(),
                    comment: self.param_comments.get(name).cloned().unwrap_or_default(),
                    source: "feature",
                    feature: Some(f.name.clone()),
                    input: Some(label),
                    favorite: self.favorites.contains(name),
                    uses: refs.get(name).cloned().unwrap_or_default(),
                    used_by: used_by(name),
                });
            }
        }
        rows
    }

    pub fn set_favorite(&mut self, name: &str, on: bool) -> Result<()> {
        if !self.name_taken(name) {
            return Err(DocError::Unknown(format!("parameter `{name}`")));
        }
        if on {
            self.favorites.insert(name.to_string());
        } else {
            self.favorites.remove(name);
        }
        Ok(())
    }

    /// User parameters as CSV (`name,unit,expression,value,comment`).
    pub fn params_csv(&self) -> String {
        let q = |s: &str| if s.contains([',', '"', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() };
        let mut out = String::from("name,unit,expression,value,comment,favorite\n");
        for r in self.param_rows().into_iter().filter(|r| r.source == "user") {
            out += &format!(
                "{},{},{},{},{},{}\n",
                q(&r.name),
                q(&r.unit),
                q(&r.expression),
                r.value.map(|v| format!("{v}")).unwrap_or_default(),
                q(&r.comment),
                r.favorite
            );
        }
        out
    }

    /// Import user parameters (CSV with a header, or JSON `[{name, expression, unit?, comment?}]`):
    /// existing ones are updated, new ones added, all checked before anything changes.
    pub fn import_params(&mut self, text: &str) -> Result<Vec<String>> {
        let rows: Vec<(String, String, Option<String>, Option<String>)> = if text.trim_start().starts_with('[') {
            let v: serde_json::Value = serde_json::from_str(text).map_err(|e| DocError::Invalid(format!("parameters JSON: {e}")))?;
            v.as_array()
                .ok_or_else(|| DocError::Invalid("parameters JSON must be a list".into()))?
                .iter()
                .map(|r| {
                    let s = |k: &str| r.get(k).and_then(|x| x.as_str().map(str::to_string).or_else(|| x.as_f64().map(|n| n.to_string())));
                    Ok((
                        s("name").ok_or_else(|| DocError::Invalid("a parameter without `name`".into()))?,
                        s("expression").or_else(|| s("value")).unwrap_or_default(),
                        s("unit"),
                        s("comment"),
                    ))
                })
                .collect::<Result<_>>()?
        } else {
            let mut lines = text.lines();
            let header: Vec<String> = parse_csv_line(lines.next().unwrap_or_default()).into_iter().map(|h| h.trim().to_ascii_lowercase()).collect();
            let col = |n: &str| header.iter().position(|h| h == n);
            let (Some(ni), Some(ei)) = (col("name"), col("expression").or(col("value"))) else {
                return Err(DocError::Invalid("the CSV needs `name` and `expression` columns".into()));
            };
            let (ui, ci) = (col("unit"), col("comment"));
            lines
                .filter(|l| !l.trim().is_empty())
                .map(|l| {
                    let c = parse_csv_line(l);
                    let get = |i: Option<usize>| i.and_then(|i| c.get(i)).map(|s| s.trim().to_string());
                    (get(Some(ni)).unwrap_or_default(), get(Some(ei)).unwrap_or_default(), get(ui), get(ci))
                })
                .collect()
        };
        if rows.len() > 10_000 {
            return Err(DocError::Invalid("too many parameters".into()));
        }
        let mut next = self.clone();
        let mut names = Vec::new();
        for (n, e, u, c) in rows {
            next.change_param(&n, &e, u.as_deref(), c.as_deref())?;
            names.push(n);
        }
        *self = next;
        Ok(names)
    }
}

/// Something that uses a parameter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ParamUser {
    /// Another parameter's expression (a stored one or a feature input).
    Parameter { name: String },
    /// A feature input (`input` is its label, e.g. `Distance`).
    Feature { feature: u64, name: String, input: String },
    /// A sketch dimension (`constraint` is its id).
    Dimension { feature: u64, name: String, constraint: String },
}

impl ParamUser {
    pub fn describe(&self) -> String {
        match self {
            ParamUser::Parameter { name } => format!("parameter {name}"),
            ParamUser::Feature { name, input, .. } => format!("{name} ({input})"),
            ParamUser::Dimension { name, constraint, .. } => format!("{name} dimension {constraint}"),
        }
    }
}

/// Length unit of the document in mm (bare numbers of unit-less parameters next to lengths).
fn doc_len(units: &str) -> f64 {
    match expr::unit_info(units) {
        Some((Kind::Length, s)) => s,
        _ => 1.0,
    }
}

impl Document {
    /// Every parameter with its unit, for evaluation: stored parameters and named feature
    /// inputs (mm / deg / none by their kind).
    pub fn param_defs(&self) -> Vec<ParamDef> {
        let mut v: Vec<ParamDef> =
            self.params.iter().map(|p| ParamDef { name: p.name.clone(), expr: p.expr.clone(), unit: p.unit.clone() }).collect();
        for f in &self.features {
            for (name, (_, e, k)) in f.param_names.iter().zip(f.kind.inputs()) {
                if !name.is_empty() {
                    // A bare number stored in an input is mm (the design's units are written in
                    // when it is typed); anything else that comes out unit-less (a unit-less
                    // parameter used as a length) is in the design's units.
                    let unit = match k {
                        Kind::Length if !expr::is_literal(&e) && expr::unit_info(self.units.trim()).is_some_and(|(k, _)| k == Kind::Length) => {
                            self.units.trim().to_string()
                        }
                        _ => unit_of(k).to_string(),
                    };
                    v.push(ParamDef { name: name.clone(), expr: e, unit });
                }
            }
        }
        v
    }

    /// Evaluate all parameters: values (mm / rad / unit-less) and errors by name. Errors name
    /// the problem (`circular reference: a → b → a`, `cannot add a length and an angle`).
    pub fn param_values(&self) -> (BTreeMap<String, Value>, BTreeMap<String, String>) {
        expr::eval_params(&self.param_defs(), doc_len(&self.units))
    }

    /// A parameter's value in its own unit (e.g. 2 for a 50.8 mm parameter in inches).
    pub fn param_display_value(&self, vals: &BTreeMap<String, Value>, name: &str) -> Option<f64> {
        let v = vals.get(name)?;
        let unit = match self.param(name) {
            Some(p) => p.unit.clone(),
            None => v.kind().map(|k| unit_of(k).to_string()).unwrap_or_default(),
        };
        Some(expr::value_in_unit(*v, &unit))
    }

    /// The unit for a new parameter with this expression: that of its value; a bare number is
    /// a length (mm).
    pub(crate) fn infer_unit(&self, e: &str) -> String {
        let (vals, _) = self.param_values();
        let v = expr::eval_with(e, &|n| vals.get(n).copied().ok_or_else(|| DocError::Expr(format!("unknown parameter `{n}`"))));
        match v.ok().and_then(|v| v.kind()) {
            Some(Kind::Unitless) if !expr::references(e).is_empty() => String::new(),
            Some(Kind::Angle) => "deg".into(),
            _ => "mm".into(),
        }
    }

    /// Everything that uses a parameter: other parameters (and feature inputs) whose
    /// expressions refer to it, and the sketch dimensions it drives.
    pub fn param_users(&self, name: &str) -> Vec<ParamUser> {
        let mut out = Vec::new();
        for p in &self.params {
            if p.name != name && expr::references(&p.expr).iter().any(|r| r == name) {
                out.push(ParamUser::Parameter { name: p.name.clone() });
            }
        }
        for f in &self.features {
            for (k, (label, e, _)) in f.kind.inputs().into_iter().enumerate() {
                if expr::references(&e).iter().any(|r| r == name) {
                    match f.param_names.get(k).filter(|n| !n.is_empty()) {
                        Some(n) => out.push(ParamUser::Parameter { name: n.clone() }),
                        None => out.push(ParamUser::Feature { feature: f.id, name: f.name.clone(), input: label.to_string() }),
                    }
                }
            }
            if let FeatureKind::Sketch { sketch, .. } = &f.kind {
                for c in &sketch.constraints {
                    if c.param.as_deref() == Some(name) {
                        out.push(ParamUser::Dimension { feature: f.id, name: f.name.clone(), constraint: c.id.clone() });
                    }
                }
            }
        }
        out
    }

    /// Remove a stored parameter; refused while something uses it (the error lists the users).
    pub fn remove_param(&mut self, name: &str) -> Result<()> {
        if self.param(name).is_none() {
            if self.features.iter().any(|f| f.param_names.iter().any(|n| n == name)) {
                return Err(DocError::Invalid(format!("`{name}` is a feature's input: delete or edit the feature instead")));
            }
            return Err(DocError::Unknown(format!("parameter `{name}`")));
        }
        let users = self.param_users(name);
        if !users.is_empty() {
            let list: Vec<String> = users.iter().map(ParamUser::describe).collect();
            return Err(DocError::Invalid(format!("`{name}` is used by {}", list.join(", "))));
        }
        self.params.retain(|p| p.name != name);
        self.favorites.remove(name);
        Ok(())
    }

    /// Rename a parameter (stored or a feature input); every expression, feature input,
    /// dimension, favourite and comment that refers to it follows. Returns how many
    /// references were updated.
    pub fn rename_param(&mut self, old: &str, new: &str) -> Result<usize> {
        let new = new.trim();
        if !self.name_taken(old) {
            return Err(DocError::Unknown(format!("parameter `{old}`")));
        }
        if old == new {
            return Ok(0);
        }
        if let Some(p) = Self::param_name_problem(new) {
            return Err(DocError::Invalid(p));
        }
        if self.name_taken(new) {
            return Err(DocError::Invalid(format!("there is already a parameter `{new}`")));
        }
        let mut n = 0;
        for p in &mut self.params {
            if p.name == old {
                p.name = new.to_string();
            }
            let r = expr::rename_reference(&p.expr, old, new);
            if r != p.expr {
                p.expr = r;
                n += 1;
            }
        }
        for f in &mut self.features {
            for x in &mut f.param_names {
                if x == old {
                    *x = new.to_string();
                }
            }
            for (_, e, _) in f.kind.inputs_mut() {
                let r = expr::rename_reference(e, old, new);
                if r != *e {
                    *e = r;
                    n += 1;
                }
            }
            if let FeatureKind::Sketch { sketch, .. } = &mut f.kind {
                for c in &mut sketch.constraints {
                    if c.param.as_deref() == Some(old) {
                        c.param = Some(new.to_string());
                        n += 1;
                    }
                }
            }
        }
        if self.favorites.remove(old) {
            self.favorites.insert(new.to_string());
        }
        for c in &mut self.configs.columns {
            if *c == crate::config::Column::Param(old.to_string()) {
                *c = crate::config::Column::Param(new.to_string());
            }
        }
        // Configuration cells refer to parameters too.
        for r in &mut self.configs.rows {
            for cell in &mut r.cells {
                *cell = expr::rename_reference(cell, old, new);
            }
        }
        if let Some(c) = self.param_comments.remove(old) {
            self.param_comments.insert(new.to_string(), c);
        }
        Ok(n)
    }

    /// Set a parameter's comment (stored parameters keep it, feature inputs in `param_comments`).
    pub fn set_param_comment(&mut self, name: &str, comment: &str) -> Result<()> {
        if comment.len() > 4096 {
            return Err(DocError::Invalid("comment too long".into()));
        }
        if let Some(p) = self.params.iter_mut().find(|p| p.name == name) {
            p.comment = comment.to_string();
            return Ok(());
        }
        if !self.name_taken(name) {
            return Err(DocError::Unknown(format!("parameter `{name}`")));
        }
        if comment.is_empty() {
            self.param_comments.remove(name);
        } else {
            self.param_comments.insert(name.to_string(), comment.to_string());
        }
        Ok(())
    }

    /// Dependency edges: (user, parameter used). Users are parameter names, or
    /// `Feature:<name>` for a feature whose inputs or dimensions use the parameter.
    pub fn param_graph(&self) -> Vec<(String, String)> {
        let mut out = expr::param_edges(&self.param_defs());
        for f in &self.features {
            let mut names: BTreeSet<String> = f.param_names.iter().filter(|n| !n.is_empty()).cloned().collect();
            if let FeatureKind::Sketch { sketch, .. } = &f.kind {
                names.extend(sketch.constraints.iter().filter_map(|c| c.param.clone()));
            }
            out.extend(names.into_iter().map(|n| (format!("Feature:{}", f.name), n)));
        }
        out
    }

    /// Features whose result depends on a parameter (directly or through other parameters).
    pub fn features_using_param(&self, name: &str) -> Vec<u64> {
        let defs = self.param_defs();
        let mut affected: BTreeSet<String> = BTreeSet::from([name.to_string()]);
        for _ in 0..defs.len().min(MAX_PARAMS * 2) {
            let before = affected.len();
            for d in &defs {
                if expr::references(&d.expr).iter().any(|r| affected.contains(r)) {
                    affected.insert(d.name.clone());
                }
            }
            if affected.len() == before {
                break;
            }
        }
        let mut out = Vec::new();
        for f in &self.features {
            let mut hit = f.param_names.iter().any(|n| affected.contains(n));
            hit |= f.kind.expressions().iter().any(|e| expr::references(e).iter().any(|r| affected.contains(r)));
            if let FeatureKind::Sketch { sketch, .. } = &f.kind {
                hit |= sketch.constraints.iter().any(|c| c.param.as_ref().is_some_and(|p| affected.contains(p)));
            }
            if hit {
                out.push(f.id);
            }
        }
        out
    }

    /// Add a user parameter (refused when the name exists).
    pub fn add_user_param(&mut self, name: &str, expr_s: &str, unit: Option<&str>, comment: Option<&str>) -> Result<()> {
        if self.name_taken(name.trim()) {
            return Err(DocError::Invalid(format!("there is already a parameter `{}`", name.trim())));
        }
        if self.params.len() >= MAX_PARAMS {
            return Err(DocError::Invalid("too many parameters".into()));
        }
        self.set_param(name.trim(), expr_s, unit, comment)
    }
}

/// One CSV line with quoted fields.
fn parse_csv_line(l: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = l.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            ('"', _) => quoted = !quoted,
            (',', false) => out.push(std::mem::take(&mut cur)),
            (c, _) => cur.push(c),
        }
    }
    out.push(cur);
    out
}

/// Parameters whose definitions depend on each other in a loop.
pub fn cycles(list: &[(String, String, Kind)]) -> BTreeSet<String> {
    let refs: BTreeMap<&str, Vec<String>> = list.iter().map(|(n, e, _)| (n.as_str(), expr::references(e))).collect();
    let mut bad = BTreeSet::new();
    for (start, _, _) in list {
        // Depth-first from `start`; reaching it again is a cycle.
        let mut stack: Vec<(String, usize)> = refs.get(start.as_str()).map(|r| r.iter().map(|x| (x.clone(), 1)).collect()).unwrap_or_default();
        let mut seen = BTreeSet::new();
        while let Some((n, d)) = stack.pop() {
            if &n == start {
                bad.insert(start.clone());
                break;
            }
            if d > 10_000 || !seen.insert(n.clone()) {
                continue;
            }
            if let Some(r) = refs.get(n.as_str()) {
                stack.extend(r.iter().map(|x| (x.clone(), d + 1)));
            }
        }
    }
    bad
}
