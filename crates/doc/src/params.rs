//! Parameters: user parameters, model parameters of sketch dimensions, and feature inputs
//! (extrude distance, fillet radius…) which also appear as model parameters `dN` without being
//! stored twice: the feature keeps its expression and the parameter name refers to it. Every
//! parameter can be referenced by name from any expression; the dependency graph and cycles
//! come from the expression references.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::expr::{self, Kind};
use crate::{DocError, Document, FeatureKind, HoleKind, PatternKind, PlaneRef, Result};

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
            | FeatureKind::Mirror { plane, .. } => plane_inputs_mut(plane, &mut v),
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
            FeatureKind::Revolve { angle, .. } => v.push(("Angle", angle, A)),
            FeatureKind::Fillet { radius, .. } => v.push(("Radius", radius, L)),
            FeatureKind::Chamfer { distance, .. } => v.push(("Distance", distance, L)),
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
            FeatureKind::Combine { .. }
            | FeatureKind::Loft { .. }
            | FeatureKind::Sweep { .. }
            | FeatureKind::Import { .. }
            | FeatureKind::MeshImport { .. } => {}
        }
        v
    }

    /// Read-only view of [`FeatureKind::inputs_mut`]: (label, expression, kind).
    pub fn inputs(&self) -> Vec<(&'static str, String, Kind)> {
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
    pub fn change_param(&mut self, name: &str, expr_s: &str, unit: Option<&str>, comment: Option<&str>) -> Result<()> {
        let hit = self.features.iter().enumerate().find_map(|(i, f)| f.param_names.iter().position(|n| n == name).map(|k| (i, k)));
        let Some((i, k)) = hit else { return self.set_param(name, expr_s, unit, comment) };
        if expr_s.len() > 4096 {
            return Err(DocError::Expr("expression too long".into()));
        }
        let mut next = self.clone();
        if let Some(f) = next.features.get_mut(i)
            && let Some((_, e, _)) = f.kind.inputs_mut().into_iter().nth(k)
        {
            *e = expr_s.to_string();
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
                value: value(&p.name, p.kind()),
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
