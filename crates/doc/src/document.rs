use serde::{Deserialize, Serialize};
use solvecraft_geom::{Plane, Vec2, Vec3};
use solvecraft_sketch::Sketch;

use crate::expr::{self, Kind};
use crate::{DocError, Result};

pub const MAX_FEATURES: usize = 10_000;
pub const MAX_PARAMS: usize = 10_000;

/// A named parameter. User parameters are created by the user; model parameters (`d1`, `d2`…)
/// are created by dimensions and features.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    pub expr: String,
    /// `mm`, `deg` or empty (unit-less).
    pub unit: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
    /// Created by a dimension or feature (not by the user).
    #[serde(default)]
    pub model: bool,
}

impl Param {
    pub fn kind(&self) -> Kind {
        Kind::from_unit(&self.unit)
    }
}

/// Where a sketch lies.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PlaneRef {
    /// `XY`, `XZ` or `YZ`.
    Origin { name: String },
    /// An explicit plane (e.g. picked from a planar face).
    Custom { plane: Plane },
    /// Another plane offset along its normal by an expression.
    Offset { base: Box<PlaneRef>, distance: String },
    /// Another plane rotated about a line (in world coordinates) by an angle expression.
    AtAngle { base: Box<PlaneRef>, axis_origin: Vec3, axis_dir: Vec3, angle: String },
    /// A construction plane feature (by name).
    Construction { name: String },
}

/// Which closed profiles of a sketch a feature uses.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProfileSel {
    /// Every profile.
    All,
    /// Profiles by index in the solved sketch's profile list.
    Indices { indices: Vec<usize> },
    /// Profiles whose outer loop is made of exactly these curves (order-insensitive); stable
    /// across parameter changes.
    Curves { loops: Vec<Vec<String>> },
    /// The profile containing each sketch point.
    Points { points: Vec<Vec2> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    #[default]
    NewBody,
    Join,
    Cut,
    Intersect,
}

impl Operation {
    pub fn parse(s: &str) -> Option<Operation> {
        Some(match s.to_ascii_lowercase().replace([' ', '_', '-'], "").as_str() {
            "new" | "newbody" | "newcomponent" => Operation::NewBody,
            "join" | "union" | "add" => Operation::Join,
            "cut" | "subtract" | "difference" => Operation::Cut,
            "intersect" | "intersection" => Operation::Intersect,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Along the sketch normal.
    #[default]
    Positive,
    Negative,
    /// Both sides; `distance` per side.
    Symmetric,
}

/// Extent of an extrude.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Extent {
    pub distance: String,
    #[serde(default)]
    pub direction: Direction,
    /// Optional second side distance (two-sided extrude), along the negative normal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance2: Option<String>,
    /// Start offset from the sketch plane (expression).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_offset: Option<String>,
    /// Through all bodies in the direction(s) instead of `distance`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub through_all: bool,
    /// Taper angle (expression; positive grows the profile).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taper: Option<String>,
}

/// Revolve axis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AxisRef {
    /// A line of the profile's sketch.
    SketchLine { curve: String },
    /// Sketch x or y axis through the sketch origin (`x` / `y`).
    SketchAxis { axis: String },
    /// World X / Y / Z axis through the origin.
    World { axis: String },
    /// Any line in the sketch plane, in world coordinates.
    Line { origin: Vec3, dir: Vec3 },
}

/// Feature definitions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FeatureKind {
    Sketch {
        plane: PlaneRef,
        sketch: Sketch,
    },
    Extrude {
        /// Feature id of the sketch.
        sketch: u64,
        profiles: ProfileSel,
        extent: Extent,
        #[serde(default)]
        operation: Operation,
        /// Target body names for join/cut/intersect (empty = all bodies).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        targets: Vec<String>,
    },
    Revolve {
        sketch: u64,
        profiles: ProfileSel,
        axis: AxisRef,
        angle: String,
        #[serde(default)]
        operation: Operation,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        targets: Vec<String>,
    },
    Fillet {
        /// Reference points on the edges to round (each re-finds the nearest edge).
        edges: Vec<Vec3>,
        radius: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
    },
    Chamfer {
        edges: Vec<Vec3>,
        distance: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
    },
    Box {
        corner: Vec3,
        length: String,
        width: String,
        height: String,
        #[serde(default)]
        operation: Operation,
    },
    Cylinder {
        base: Vec3,
        #[serde(default = "z_axis")]
        axis: Vec3,
        radius: String,
        height: String,
        #[serde(default)]
        operation: Operation,
    },
    Sphere {
        center: Vec3,
        radius: String,
        #[serde(default)]
        operation: Operation,
    },
    Torus {
        center: Vec3,
        major: String,
        minor: String,
        #[serde(default)]
        operation: Operation,
    },
    Combine {
        target: String,
        tools: Vec<String>,
        operation: Operation,
        #[serde(default)]
        keep_tools: bool,
    },
    /// Copies of other features' results (rectangular or circular).
    Pattern {
        features: Vec<String>,
        pattern: PatternKind,
    },
    /// Mirror copies of other features' results in a plane.
    Mirror {
        features: Vec<String>,
        plane: PlaneRef,
    },
    /// A construction plane (sketch placement, split tool).
    ConstructionPlane {
        plane: PlaneRef,
    },
    /// Split a body with a plane into two bodies.
    Split {
        body: String,
        plane: PlaneRef,
    },
    Move {
        bodies: Vec<String>,
        translate: [String; 3],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rotate_axis: Option<Vec3>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        angle: Option<String>,
    },
}

/// Pattern layouts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PatternKind {
    Rectangular {
        dir1: Vec3,
        count1: String,
        spacing1: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dir2: Option<Vec3>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        count2: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        spacing2: Option<String>,
    },
    Circular {
        origin: Vec3,
        axis: Vec3,
        count: String,
        /// Total angle (360 deg = evenly around).
        angle: String,
    },
}

fn plane_exprs<'a>(p: &'a PlaneRef, v: &mut Vec<&'a str>) {
    let mut p = p;
    for _ in 0..64 {
        match p {
            PlaneRef::Offset { base, distance } => {
                v.push(distance);
                p = base;
            }
            PlaneRef::AtAngle { base, angle, .. } => {
                v.push(angle);
                p = base;
            }
            _ => break,
        }
    }
}

fn z_axis() -> Vec3 {
    Vec3::Z
}

impl FeatureKind {
    /// Fusion-like type name (timeline tooltip, `inspect`).
    pub fn type_name(&self) -> &'static str {
        match self {
            FeatureKind::Sketch { .. } => "Sketch",
            FeatureKind::Extrude { .. } => "ExtrudeFeature",
            FeatureKind::Revolve { .. } => "RevolveFeature",
            FeatureKind::Fillet { .. } => "FilletFeature",
            FeatureKind::Chamfer { .. } => "ChamferFeature",
            FeatureKind::Box { .. } => "BoxFeature",
            FeatureKind::Cylinder { .. } => "CylinderFeature",
            FeatureKind::Sphere { .. } => "SphereFeature",
            FeatureKind::Torus { .. } => "TorusFeature",
            FeatureKind::Combine { .. } => "CombineFeature",
            FeatureKind::Pattern { pattern: PatternKind::Rectangular { .. }, .. } => "RectangularPatternFeature",
            FeatureKind::Pattern { .. } => "CircularPatternFeature",
            FeatureKind::Mirror { .. } => "MirrorFeature",
            FeatureKind::ConstructionPlane { .. } => "ConstructionPlane",
            FeatureKind::Split { .. } => "SplitBodyFeature",
            FeatureKind::Move { .. } => "MoveFeature",
        }
    }
    /// Default name prefix (`Extrude` → `Extrude1`).
    pub fn base_name(&self) -> &'static str {
        match self {
            FeatureKind::Sketch { .. } => "Sketch",
            FeatureKind::Extrude { .. } => "Extrude",
            FeatureKind::Revolve { .. } => "Revolve",
            FeatureKind::Fillet { .. } => "Fillet",
            FeatureKind::Chamfer { .. } => "Chamfer",
            FeatureKind::Box { .. } => "Box",
            FeatureKind::Cylinder { .. } => "Cylinder",
            FeatureKind::Sphere { .. } => "Sphere",
            FeatureKind::Torus { .. } => "Torus",
            FeatureKind::Combine { .. } => "Combine",
            FeatureKind::Pattern { pattern: PatternKind::Rectangular { .. }, .. } => "RectangularPattern",
            FeatureKind::Pattern { .. } => "CircularPattern",
            FeatureKind::Mirror { .. } => "Mirror",
            FeatureKind::ConstructionPlane { .. } => "Plane",
            FeatureKind::Split { .. } => "Split",
            FeatureKind::Move { .. } => "Move",
        }
    }
    /// Every expression the feature uses.
    pub fn expressions(&self) -> Vec<&str> {
        let mut v: Vec<&str> = Vec::new();
        match self {
            FeatureKind::Sketch { plane, .. } | FeatureKind::ConstructionPlane { plane } | FeatureKind::Split { plane, .. } => {
                plane_exprs(plane, &mut v)
            }
            FeatureKind::Extrude { extent, .. } => {
                v.push(&extent.distance);
                if let Some(d) = &extent.distance2 {
                    v.push(d);
                }
                if let Some(d) = &extent.start_offset {
                    v.push(d);
                }
                if let Some(d) = &extent.taper {
                    v.push(d);
                }
            }
            FeatureKind::Revolve { angle, .. } => v.push(angle),
            FeatureKind::Fillet { radius, .. } => v.push(radius),
            FeatureKind::Chamfer { distance, .. } => v.push(distance),
            FeatureKind::Box { length, width, height, .. } => v.extend([length.as_str(), width, height]),
            FeatureKind::Cylinder { radius, height, .. } => v.extend([radius.as_str(), height]),
            FeatureKind::Sphere { radius, .. } => v.push(radius),
            FeatureKind::Torus { major, minor, .. } => v.extend([major.as_str(), minor]),
            FeatureKind::Combine { .. } => {}
            FeatureKind::Pattern { pattern, .. } => match pattern {
                PatternKind::Rectangular { count1, spacing1, count2, spacing2, .. } => {
                    v.extend([count1.as_str(), spacing1]);
                    v.extend(count2.iter().map(String::as_str));
                    v.extend(spacing2.iter().map(String::as_str));
                }
                PatternKind::Circular { count, angle, .. } => v.extend([count.as_str(), angle]),
            },
            FeatureKind::Mirror { plane, .. } => plane_exprs(plane, &mut v),
            FeatureKind::Move { translate, angle, .. } => {
                v.extend(translate.iter().map(String::as_str));
                if let Some(a) = angle {
                    v.push(a);
                }
            }
        }
        v
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Feature {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub suppressed: bool,
    /// Names for the bodies this feature creates (in order); missing ones get `BodyN`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub body_names: Vec<String>,
    #[serde(flatten)]
    pub kind: FeatureKind,
}

/// The document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    #[serde(default = "default_format")]
    pub format: String,
    pub name: String,
    #[serde(default = "default_units")]
    pub units: String,
    pub params: Vec<Param>,
    pub features: Vec<Feature>,
    /// Timeline marker: features at or after this index are rolled back (not evaluated).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marker: Option<usize>,
    #[serde(default)]
    pub next_id: u64,
}

fn default_format() -> String {
    "solvecraft/1".into()
}
fn default_units() -> String {
    "mm".into()
}

impl Default for Document {
    fn default() -> Self {
        Document::new("Untitled")
    }
}

impl Document {
    pub fn new(name: &str) -> Self {
        Document {
            format: default_format(),
            name: name.into(),
            units: default_units(),
            params: Vec::new(),
            features: Vec::new(),
            marker: None,
            next_id: 1,
        }
    }

    pub fn from_json(s: &str) -> Result<Document> {
        let d: Document = serde_json::from_str(s).map_err(|e| DocError::Invalid(format!("document: {e}")))?;
        if d.features.len() > MAX_FEATURES || d.params.len() > MAX_PARAMS {
            return Err(DocError::Invalid("document too large".into()));
        }
        Ok(d)
    }
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    pub fn feature(&self, id: u64) -> Option<&Feature> {
        self.features.iter().find(|f| f.id == id)
    }
    pub fn feature_mut(&mut self, id: u64) -> Option<&mut Feature> {
        self.features.iter_mut().find(|f| f.id == id)
    }
    pub fn feature_index(&self, id: u64) -> Option<usize> {
        self.features.iter().position(|f| f.id == id)
    }
    /// Find a feature by id (number) or name.
    pub fn find_feature(&self, r: &str) -> Option<&Feature> {
        if let Ok(id) = r.parse::<u64>()
            && let Some(f) = self.feature(id)
        {
            return Some(f);
        }
        self.features.iter().find(|f| f.name.eq_ignore_ascii_case(r))
    }

    fn unique_name(&self, base: &str) -> String {
        let mut n = 1;
        loop {
            let name = format!("{base}{n}");
            if !self.features.iter().any(|f| f.name == name) {
                return name;
            }
            n += 1;
        }
    }

    /// Insert a feature at the marker (or the end) and return its id.
    pub fn add_feature(&mut self, kind: FeatureKind, name: Option<&str>) -> Result<u64> {
        if self.features.len() >= MAX_FEATURES {
            return Err(DocError::Invalid("too many features".into()));
        }
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        let name = match name {
            Some(n) if !n.trim().is_empty() => n.trim().to_string(),
            _ => self.unique_name(kind.base_name()),
        };
        let f = Feature { id, name, suppressed: false, body_names: Vec::new(), kind };
        match self.marker {
            Some(m) if m < self.features.len() => {
                self.features.insert(m, f);
                self.marker = Some(m + 1);
            }
            _ => self.features.push(f),
        }
        Ok(id)
    }

    /// Delete a feature (and, for a sketch, the features that use it).
    pub fn delete_feature(&mut self, id: u64) -> Result<Vec<u64>> {
        let idx = self.feature_index(id).ok_or_else(|| DocError::Unknown(format!("feature {id}")))?;
        let mut gone = vec![id];
        for f in &self.features {
            match &f.kind {
                FeatureKind::Extrude { sketch, .. } | FeatureKind::Revolve { sketch, .. } if *sketch == id => gone.push(f.id),
                _ => {}
            }
        }
        self.features.retain(|f| !gone.contains(&f.id));
        if let Some(m) = self.marker
            && m > idx
        {
            self.marker = Some(m.saturating_sub(gone.len()).max(idx));
        }
        // Drop model parameters nobody references any more.
        self.prune_model_params();
        Ok(gone)
    }

    /// Remove model parameters not referenced by any dimension, feature or other parameter.
    pub fn prune_model_params(&mut self) {
        let used = self.referenced_names();
        self.params.retain(|p| !p.model || used.contains(&p.name));
    }

    fn referenced_names(&self) -> Vec<String> {
        let mut used: Vec<String> = Vec::new();
        for f in &self.features {
            for e in f.kind.expressions() {
                used.extend(expr::references(e));
            }
            if let FeatureKind::Sketch { sketch, .. } = &f.kind {
                used.extend(sketch.constraints.iter().filter_map(|c| c.param.clone()));
            }
        }
        for p in &self.params {
            used.extend(expr::references(&p.expr));
        }
        used
    }

    pub fn param(&self, name: &str) -> Option<&Param> {
        self.params.iter().find(|p| p.name == name)
    }

    fn valid_name(name: &str) -> bool {
        let mut cs = name.chars();
        matches!(cs.next(), Some(c) if c.is_alphabetic() || c == '_')
            && cs.all(|c| c.is_alphanumeric() || c == '_')
            && !matches!(name, "mm" | "cm" | "m" | "um" | "in" | "ft" | "deg" | "rad" | "pi" | "PI")
            && name.len() <= 64
    }

    /// Add or change a parameter. Fails if the result would not evaluate.
    pub fn set_param(&mut self, name: &str, expr_s: &str, unit: Option<&str>, comment: Option<&str>) -> Result<()> {
        if !Self::valid_name(name) {
            return Err(DocError::Invalid(format!("`{name}` is not a valid parameter name")));
        }
        if expr_s.len() > 4096 {
            return Err(DocError::Expr("expression too long".into()));
        }
        let mut next = self.clone();
        match next.params.iter_mut().find(|p| p.name == name) {
            Some(p) => {
                p.expr = expr_s.to_string();
                if let Some(u) = unit {
                    p.unit = u.to_string();
                }
                if let Some(c) = comment {
                    p.comment = c.to_string();
                }
            }
            None => {
                if next.params.len() >= MAX_PARAMS {
                    return Err(DocError::Invalid("too many parameters".into()));
                }
                next.params.push(Param {
                    name: name.into(),
                    expr: expr_s.into(),
                    unit: unit.unwrap_or("mm").into(),
                    comment: comment.unwrap_or_default().into(),
                    model: false,
                })
            }
        }
        let (_, errors) = next.param_values();
        if let Some(e) = errors.get(name) {
            return Err(DocError::Expr(e.clone()));
        }
        *self = next;
        Ok(())
    }

    /// Create the next free model parameter `dN` with the given expression and unit.
    pub fn new_model_param(&mut self, expr_s: &str, unit: &str) -> String {
        let mut n = 1;
        let name = loop {
            let c = format!("d{n}");
            if self.param(&c).is_none() {
                break c;
            }
            n += 1;
        };
        self.params.push(Param { name: name.clone(), expr: expr_s.into(), unit: unit.into(), comment: String::new(), model: true });
        name
    }

    /// Remove a user parameter (fails while something references it).
    pub fn remove_param(&mut self, name: &str) -> Result<()> {
        if self.param(name).is_none() {
            return Err(DocError::Unknown(format!("parameter `{name}`")));
        }
        let mut probe = self.clone();
        probe.params.retain(|p| p.name != name);
        if probe.referenced_names().iter().any(|n| n == name) {
            return Err(DocError::Invalid(format!("`{name}` is still used")));
        }
        *self = probe;
        Ok(())
    }

    /// Evaluate all parameters: values (mm / rad / unit-less) and errors by name.
    pub fn param_values(&self) -> (std::collections::BTreeMap<String, expr::Value>, std::collections::BTreeMap<String, String>) {
        let list: Vec<(String, String, Kind)> = self.params.iter().map(|p| (p.name.clone(), p.expr.clone(), p.kind())).collect();
        expr::eval_params(&list)
    }

    /// Evaluate an expression against the document's parameters.
    pub fn eval(&self, e: &str, kind: Kind) -> Result<f64> {
        let (vals, _) = self.param_values();
        Self::eval_in(&vals, e, kind)
    }

    pub fn eval_in(vals: &std::collections::BTreeMap<String, expr::Value>, e: &str, kind: Kind) -> Result<f64> {
        expr::eval_with(e, &|n| vals.get(n).copied().ok_or_else(|| DocError::Expr(format!("unknown parameter `{n}`"))))?.to_kind(kind)
    }

    /// Resolve a plane reference.
    pub fn resolve_plane(&self, vals: &std::collections::BTreeMap<String, expr::Value>, p: &PlaneRef, depth: usize) -> Result<Plane> {
        if depth > 32 {
            return Err(DocError::Invalid("plane offsets nested too deeply".into()));
        }
        match p {
            PlaneRef::Origin { name } => Plane::named(name).ok_or_else(|| DocError::Unknown(format!("plane `{name}`"))),
            PlaneRef::Custom { plane } => Plane::new(plane.origin, plane.x, plane.y).ok_or_else(|| DocError::Invalid("degenerate plane".into())),
            PlaneRef::Offset { base, distance } => {
                let b = self.resolve_plane(vals, base, depth + 1)?;
                Ok(b.offset(Self::eval_in(vals, distance, Kind::Length)?))
            }
            PlaneRef::AtAngle { base, axis_origin, axis_dir, angle } => {
                let b = self.resolve_plane(vals, base, depth + 1)?;
                let a = Self::eval_in(vals, angle, Kind::Angle)?;
                let d = axis_dir.normalized().ok_or_else(|| DocError::Invalid("rotation axis".into()))?;
                let rot = |v: Vec3| v * a.cos() + d.cross(v) * a.sin() + d * (d.dot(v) * (1.0 - a.cos()));
                let o = *axis_origin + rot(b.origin - *axis_origin);
                Plane::new(o, rot(b.x), rot(b.y)).ok_or_else(|| DocError::Invalid("degenerate plane".into()))
            }
            PlaneRef::Construction { name } => match self.find_feature(name).map(|f| &f.kind) {
                Some(FeatureKind::ConstructionPlane { plane }) => self.resolve_plane(vals, plane, depth + 1),
                _ => Err(DocError::Unknown(format!("construction plane `{name}`"))),
            },
        }
    }

    /// Mutable access to a sketch feature's sketch.
    pub fn sketch_mut(&mut self, id: u64) -> Result<&mut Sketch> {
        match self.feature_mut(id).map(|f| &mut f.kind) {
            Some(FeatureKind::Sketch { sketch, .. }) => Ok(sketch),
            Some(_) => Err(DocError::Invalid(format!("feature {id} is not a sketch"))),
            None => Err(DocError::Unknown(format!("sketch {id}"))),
        }
    }
    pub fn sketch(&self, id: u64) -> Result<&Sketch> {
        match self.feature(id).map(|f| &f.kind) {
            Some(FeatureKind::Sketch { sketch, .. }) => Ok(sketch),
            Some(_) => Err(DocError::Invalid(format!("feature {id} is not a sketch"))),
            None => Err(DocError::Unknown(format!("sketch {id}"))),
        }
    }

    /// Apply parameter values to every dimension in a sketch (in place).
    pub fn apply_dimension_values(&self, vals: &std::collections::BTreeMap<String, expr::Value>, sk: &mut Sketch) -> Result<()> {
        for c in &mut sk.constraints {
            if let Some(p) = &c.param {
                let kind = if c.kind.is_angle() { Kind::Angle } else { Kind::Length };
                let v = vals.get(p).copied().ok_or_else(|| DocError::Unknown(format!("parameter `{p}`")))?.to_kind(kind)?;
                c.kind.set_value(v);
            }
        }
        Ok(())
    }
}
