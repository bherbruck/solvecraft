//! Sheet metal: rules (thickness, K-factor, bend radius, reliefs as expressions that may use
//! parameters), and the sheet body model behind every sheet-metal feature.
//!
//! A sheet body is kept as its flat pattern: the base panel, flanges hanging off panel edges
//! (each a bend zone of width BA = θ·(R + K·T) followed by a flat panel), and cut-outs, all in
//! the flat frame (the base panel's bottom face; thickness grows along its normal). Folding maps
//! each flange's flat coordinates through its parents' bends: past a bend line, a point loses
//! the bend zone's width and turns by θ about the bend axis (R from the inner face). The folded
//! solid is built from extruded pieces (panels, and bend zones as annular sectors) joined
//! together; the flat solid is the outline extruded through the thickness.

use serde::{Deserialize, Serialize};
use solvecraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use solvecraft_kernel::{self as kernel, Body, EdgeSpec, FaceSpec, SurfSpec};

use crate::expr::{self, Kind, Value};
use crate::{DocError, Document, IDENTITY, Mat, Result, apply_point, apply_vector, mat_inverse, mat_mul};

/// A sheet metal rule. Values are expressions; `Thickness` inside them is the rule's thickness.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetRule {
    pub name: String,
    pub thickness: String,
    pub k_factor: String,
    pub bend_radius: String,
    #[serde(default = "rule_relief_width")]
    pub relief_width: String,
    #[serde(default = "rule_relief_depth")]
    pub relief_depth: String,
    #[serde(default = "rule_corner_relief")]
    pub corner_relief: String,
    /// Inner radius of flat hems.
    #[serde(default = "rule_hem_gap")]
    pub hem_gap: String,
    /// Gap between flanges that meet at a corner.
    #[serde(default = "rule_gap")]
    pub gap: String,
}

fn rule_gap() -> String {
    "Thickness".into()
}

fn rule_relief_width() -> String {
    "Thickness".into()
}
fn rule_relief_depth() -> String {
    "Thickness * 0.5".into()
}
fn rule_corner_relief() -> String {
    "Thickness * 4.0".into()
}
fn rule_hem_gap() -> String {
    "0.01 mm".into()
}

impl SheetRule {
    /// Fusion's default: Steel (mm), 2.5 mm, K 0.44, bend radius = Thickness.
    pub fn steel() -> SheetRule {
        SheetRule {
            name: "Steel (mm)".into(),
            thickness: "2.5 mm".into(),
            k_factor: "0.44".into(),
            bend_radius: "Thickness".into(),
            relief_width: rule_relief_width(),
            relief_depth: rule_relief_depth(),
            corner_relief: rule_corner_relief(),
            hem_gap: rule_hem_gap(),
            gap: rule_gap(),
        }
    }
}

/// The design's sheet metal rules and the one new sheet bodies use.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SheetSettings {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<SheetRule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
}

impl SheetSettings {
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty() && self.active.is_none()
    }
}

/// A rule's values (mm, unit-less K).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct RuleValues {
    pub thickness: f64,
    pub k_factor: f64,
    pub bend_radius: f64,
    pub relief_width: f64,
    pub relief_depth: f64,
    pub corner_relief: f64,
    pub hem_gap: f64,
    pub gap: f64,
}

impl Document {
    /// A rule by name, the active one, or the default steel rule.
    pub fn sheet_rule(&self, name: Option<&str>) -> SheetRule {
        let want = name.or(self.sheet.active.as_deref());
        want.and_then(|n| self.sheet.rules.iter().find(|r| r.name == n))
            .or_else(|| self.sheet.rules.first())
            .cloned()
            .unwrap_or_else(SheetRule::steel)
    }

    /// Evaluate a rule: `Thickness` in its expressions is the rule's thickness.
    pub fn rule_values(&self, vals: &std::collections::BTreeMap<String, Value>, r: &SheetRule) -> Result<RuleValues> {
        let look = |n: &str| vals.get(n).copied().ok_or_else(|| DocError::Expr(format!("unknown parameter `{n}`")));
        let t = expr::eval_with(&r.thickness, &look)?.to_kind(Kind::Length)?;
        if !(t > 0.0 && t < 1e4) {
            return Err(DocError::Invalid("the sheet thickness must be positive".into()));
        }
        let with_t = |e: &str, k: Kind| -> Result<f64> {
            let f = |n: &str| if n == "Thickness" && !vals.contains_key("Thickness") { Ok(Value::length(t)) } else { look(n) };
            expr::eval_with(e, &f)?.to_kind(k)
        };
        let v = RuleValues {
            thickness: t,
            k_factor: with_t(&r.k_factor, Kind::Unitless)?,
            bend_radius: with_t(&r.bend_radius, Kind::Length)?,
            relief_width: with_t(&r.relief_width, Kind::Length)?,
            relief_depth: with_t(&r.relief_depth, Kind::Length)?,
            corner_relief: with_t(&r.corner_relief, Kind::Length)?,
            hem_gap: with_t(&r.hem_gap, Kind::Length)?,
            gap: with_t(&r.gap, Kind::Length)?,
        };
        if !(0.0..=1.0).contains(&v.k_factor) || v.bend_radius < 0.0 {
            return Err(DocError::Invalid("the K-factor must be 0…1 and the bend radius positive".into()));
        }
        Ok(v)
    }
}

/// A flange: a bend along a panel edge and the flat panel past it (flat coordinates).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SheetFlange {
    /// The flange it hangs off (None = the base panel).
    pub parent: Option<usize>,
    /// Bend start line: from `p` along `d` for `len`; `n` points into the flange.
    pub p: Vec2,
    pub d: Vec2,
    pub len: f64,
    pub n: Vec2,
    /// Bend angle (radians; positive bends toward the top face, +z of the flat frame).
    pub angle: f64,
    /// Inner bend radius.
    pub radius: f64,
    /// Flat length of the panel past the bend zone.
    pub leg: f64,
    /// The panel runs on past the bend's ends by these (toward a neighbouring flange).
    #[serde(default)]
    pub ext: (f64, f64),
    /// A fold's panel: the part of the sheet past the bend (flat coordinates, counter-clockwise)
    /// instead of a rectangle `leg` long.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<Vec<Vec2>>,
}

/// A sheet metal body (see the module docs).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SheetBody {
    pub body: String,
    pub rule: String,
    pub t: f64,
    pub k: f64,
    /// The flat frame: base panel bottom face; z = thickness direction.
    pub frame: Plane,
    pub base: Region2,
    pub flanges: Vec<SheetFlange>,
    /// Cut-outs (flat coordinates) through the sheet.
    pub holes: Vec<Region2>,
    /// Unfolded.
    pub flat: bool,
    /// Gap between flanges that meet at a corner.
    pub gap: f64,
}

/// A piece of the sheet: a panel (None = the base) or a flange's bend zone.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Piece {
    Panel(Option<usize>),
    Zone(usize),
}

/// A flat piece of the sheet (counter-clockwise outer loop, clockwise holes).
#[derive(Clone, Debug)]
struct Cell {
    piece: Piece,
    outer: Vec<Seg2>,
    holes: Vec<Vec<Seg2>>,
}

fn rev_edge(e: EdgeSpec) -> EdgeSpec {
    match e {
        EdgeSpec::Line { a, b } => EdgeSpec::Line { a: b, b: a },
        EdgeSpec::Arc { a, b, mid } => EdgeSpec::Arc { a: b, b: a, mid },
    }
}

fn dedup(p: &[Vec2]) -> Vec<Vec2> {
    let mut v: Vec<Vec2> = Vec::new();
    for q in p {
        if v.last().is_none_or(|l| l.dist(*q) > 1e-9) {
            v.push(*q);
        }
    }
    if v.len() > 1 && v.first().zip(v.last()).is_some_and(|(a, b)| a.dist(*b) < 1e-9) {
        v.pop();
    }
    v
}

/// A bend for the bend table.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BendInfo {
    pub index: usize,
    pub angle_deg: f64,
    pub radius: f64,
    pub up: bool,
    pub allowance: f64,
    pub length: f64,
    /// Centre line in flat coordinates.
    pub start: Vec2,
    pub end: Vec2,
}

fn rot_about(o: Vec3, k: Vec3, a: f64) -> Mat {
    crate::rigid(Vec3::ZERO, o, k, a)
}

fn translate(t: Vec3) -> Mat {
    let mut m = IDENTITY;
    m[3] = [t.x, t.y, t.z, 1.0];
    m
}

fn v3(v: Vec2) -> Vec3 {
    Vec3::new(v.x, v.y, 0.0)
}

fn plane_of(m: &Mat, o: Vec3, x: Vec3, y: Vec3) -> Result<Plane> {
    Plane::new(apply_point(m, o), apply_vector(m, x), apply_vector(m, y)).ok_or_else(|| DocError::Invalid("sheet: degenerate plane".into()))
}

fn rect(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> Region2 {
    let mut lp = Loop2::polygon(&[a, b, c, d]);
    if lp.signed_area() < 0.0 {
        lp = lp.reversed();
    }
    Region2 { outer: lp, holes: Vec::new() }
}

/// Round to a 1e-9 mm grid (keeps trimmed edges exactly where the kernel expects them).
fn snap(v: Vec2) -> Vec2 {
    let r = |x: f64| (x * 1e9).round() / 1e9;
    Vec2::new(r(v.x), r(v.y))
}

/// Clip a polygon to the half-plane (p − o)·n ≥ 0.
fn clip(poly: &[Vec2], o: Vec2, n: Vec2) -> Vec<Vec2> {
    let mut out = Vec::new();
    let m = poly.len();
    for i in 0..m {
        let (Some(&a), Some(&b)) = (poly.get(i), poly.get((i + 1) % m)) else { continue };
        let (da, db) = ((a - o).dot(n), (b - o).dot(n));
        if da >= 0.0 {
            out.push(a);
        }
        if (da >= 0.0) != (db >= 0.0) {
            let t = da / (da - db);
            out.push(snap(a + (b - a) * t));
        }
    }
    out
}

impl SheetBody {
    /// Bend allowance of a flange.
    pub fn allowance(&self, f: &SheetFlange) -> f64 {
        f.angle.abs() * (f.radius + self.k * self.t)
    }

    /// Fold transform of flange `i`'s panel (flat 3D coordinates → folded, in the flat frame).
    pub fn fold(&self, i: usize) -> Mat {
        let mut chain = Vec::new();
        let mut cur = Some(i);
        for _ in 0..1000 {
            let Some(c) = cur else { break };
            chain.push(c);
            cur = self.flanges.get(c).and_then(|f| f.parent);
        }
        let mut m = IDENTITY;
        for c in chain.into_iter().rev() {
            if let Some(f) = self.flanges.get(c) {
                m = mat_mul(&m, &self.bend_motion(f));
            }
        }
        m
    }

    /// The fold of one bend: drop the bend zone, then turn about the bend axis.
    fn bend_motion(&self, f: &SheetFlange) -> Mat {
        let ba = self.allowance(f);
        let (axis_o, k, a) = self.bend_axis(f);
        mat_mul(&rot_about(axis_o, k, a), &translate(v3(f.n) * -ba))
    }

    /// Bend axis point, direction and signed turn (flat coordinates).
    fn bend_axis(&self, f: &SheetFlange) -> (Vec3, Vec3, f64) {
        let up = f.angle >= 0.0;
        let z = if up { self.t + f.radius } else { -f.radius };
        let o = v3(f.p) + Vec3::Z * z;
        let k = v3(f.n).cross(Vec3::Z);
        (o, k, if up { f.angle.abs() } else { -f.angle.abs() })
    }

    /// Flat region of a flange's panel.
    pub fn panel(&self, f: &SheetFlange) -> Region2 {
        if let Some(sh) = &f.shape {
            return Region2 { outer: Loop2::polygon(sh).ccw(), holes: Vec::new() };
        }
        let ba = self.allowance(f);
        let a = f.p + f.n * ba - f.d * f.ext.0;
        let b = f.p + f.n * ba + f.d * (f.len + f.ext.1);
        rect(a, b, b + f.n * f.leg, a + f.n * f.leg)
    }

    /// Flat region of a flange's bend zone.
    pub fn zone(&self, f: &SheetFlange) -> Region2 {
        let ba = self.allowance(f);
        let (u0, u1) = self.zone_span(f);
        let (a, b) = (f.p + f.d * u0, f.p + f.d * u1);
        rect(a, b, b + f.n * ba, a + f.n * ba)
    }

    /// The part of a bend along its parent's edge: where a later flange trimmed that edge back
    /// (a corner), the bend stops and the flange panel runs on alone (a corner relief).
    pub fn zone_span(&self, f: &SheetFlange) -> (f64, f64) {
        if f.parent.is_some() {
            return (0.0, f.len);
        }
        let pts = self.base.outer.ccw().polyline(1e-3);
        let m = pts.len();
        let mut span: Option<(f64, f64)> = None;
        for k in 0..m {
            let (Some(&a), Some(&b)) = (pts.get(k), pts.get((k + 1) % m)) else { continue };
            let on = (a - f.p).dot(f.n).abs() < 1e-6 && (b - f.p).dot(f.n).abs() < 1e-6;
            if !on {
                continue;
            }
            let (ua, ub) = ((a - f.p).dot(f.d), (b - f.p).dot(f.d));
            let (lo, hi) = (ua.min(ub).max(0.0), ua.max(ub).min(f.len));
            if hi > lo + 1e-9 {
                span = Some(span.map_or((lo, hi), |(x, y)| (x.min(lo), y.max(hi))));
            }
        }
        span.unwrap_or((0.0, f.len))
    }

    fn frame_mat(&self) -> Mat {
        let fr = &self.frame;
        let n = fr.normal();
        [[fr.x.x, fr.x.y, fr.x.z, 0.0], [fr.y.x, fr.y.y, fr.y.z, 0.0], [n.x, n.y, n.z, 0.0], [fr.origin.x, fr.origin.y, fr.origin.z, 1.0]]
    }

    /// Flat coordinates (x, y, z) → world, through the fold of flange `i` (None = base).
    pub fn world(&self, i: Option<usize>) -> Mat {
        let f = match i {
            Some(i) if !self.flat => self.fold(i),
            _ => IDENTITY,
        };
        mat_mul(&self.frame_mat(), &f)
    }

    /// Where a flat point of a piece lands (folded), at height `z` in the sheet.
    fn map3(&self, piece: Piece, q: Vec2, z: f64) -> Vec3 {
        match piece {
            Piece::Panel(i) => apply_point(&self.world(i), v3(q) + Vec3::Z * z),
            Piece::Zone(i) => {
                let Some(f) = self.flanges.get(i) else { return Vec3::ZERO };
                let s = (q - f.p).dot(f.n);
                let q0 = q - f.n * s;
                let neutral = (f.radius + self.k * self.t).max(1e-12);
                let phi = (s / neutral).clamp(0.0, f.angle.abs());
                let (axis_o, k, turn) = self.bend_axis(f);
                let m = mat_mul(&self.world(f.parent), &rot_about(axis_o, k, turn.signum() * phi));
                apply_point(&m, v3(q0) + Vec3::Z * z)
            }
        }
    }

    /// A flat edge of a piece as a 3D edge at height `z`.
    fn edge3(&self, piece: Piece, e: &Seg2, z: f64) -> Result<EdgeSpec> {
        let (a, b) = (e.start(), e.end());
        let (pa, pb) = (self.map3(piece, a, z), self.map3(piece, b, z));
        match (piece, e) {
            (Piece::Panel(_), Seg2::Line { .. }) => Ok(EdgeSpec::Line { a: pa, b: pb }),
            (Piece::Panel(_), Seg2::Arc { .. }) => Ok(EdgeSpec::Arc { a: pa, b: pb, mid: self.map3(piece, e.point_at(0.5), z) }),
            (Piece::Zone(i), Seg2::Line { .. }) => {
                let f = self.flanges.get(i).ok_or_else(|| DocError::Invalid("bend".into()))?;
                let dlt = b - a;
                if dlt.dot(f.n).abs() < 1e-9 * dlt.len().max(1.0) {
                    Ok(EdgeSpec::Line { a: pa, b: pb })
                } else if dlt.dot(f.d).abs() < 1e-9 * dlt.len().max(1.0) {
                    Ok(EdgeSpec::Arc { a: pa, b: pb, mid: self.map3(piece, (a + b) * 0.5, z) })
                } else {
                    Err(DocError::Invalid("not supported yet: cut-out edges running across a bend at an angle".into()))
                }
            }
            (Piece::Zone(_), _) => Err(DocError::Invalid("not supported yet: curved cut-outs across a bend".into())),
            (_, _) => Err(DocError::Invalid("not supported yet: spline or conic cut-outs in sheet metal".into())),
        }
    }

    /// The pieces of the sheet as cells: base and flange panels and bend zones, each less the
    /// cut-outs over it (cut-outs inside a panel stay exact holes; ones crossing a piece's edge
    /// must be convex polygons).
    fn cells(&self) -> Result<Vec<Cell>> {
        let mut pieces: Vec<(Piece, Vec<Vec2>)> =
            self.panels().into_iter().map(|(pi, r)| (Piece::Panel(pi), dedup(&r.outer.ccw().polyline(1e-3)))).collect();
        for (i, f) in self.flanges.iter().enumerate() {
            pieces.push((Piece::Zone(i), dedup(&self.zone(f).outer.ccw().polyline(1e-3))));
        }
        let mut out = Vec::new();
        for (piece, poly) in pieces {
            let mut inner: Vec<Vec<Seg2>> = Vec::new();
            let mut crossing: Vec<Vec<Vec2>> = Vec::new();
            for h in &self.holes {
                let hp = dedup(&h.outer.ccw().polyline(1e-3));
                if matches!(piece, Piece::Panel(_)) && hp.iter().all(|p| point_in(&poly, *p)) {
                    inner.push(h.outer.ccw().reversed().segs.clone());
                } else if hp.iter().any(|p| point_in(&poly, *p)) || poly.iter().any(|p| point_in(&hp, *p)) || polys_cross(&poly, &hp) {
                    if !is_convex(&hp) {
                        return Err(DocError::Invalid(
                            "not supported yet: cut-outs over a panel's edge or a bend that are not convex polygons".into(),
                        ));
                    }
                    crossing.push(hp);
                }
            }
            if crossing.is_empty() {
                let outer = Loop2::polygon(&poly).segs;
                out.push(Cell {
                    piece,
                    outer: if matches!(piece, Piece::Panel(None)) { self.base.outer.ccw().segs.clone() } else { outer },
                    holes: inner,
                });
                continue;
            }
            let mut parts = vec![poly];
            for h in &crossing {
                parts = parts.iter().flat_map(|p| convex_diff(p, h)).collect();
            }
            let loops = merge_loops(&parts);
            let (outers, holes): (Vec<Vec<Vec2>>, Vec<Vec<Vec2>>) = loops.into_iter().partition(|l| signed_area(l) > 0.0);
            for o in outers {
                let mut hs: Vec<Vec<Seg2>> =
                    holes.iter().filter(|h| h.first().is_some_and(|p| point_in(&o, *p))).map(|h| Loop2::polygon(h).segs).collect();
                hs.extend(inner.iter().filter(|h| h.first().is_some_and(|s| point_in(&o, s.start()))).cloned());
                out.push(Cell { piece, outer: Loop2::polygon(&o).segs, holes: hs });
            }
        }
        Ok(out)
    }

    /// Faces of the solid made by `cells` between heights `z0` and `z1`.
    fn faces(&self, cells: &[Cell], z0: f64, z1: f64) -> Result<Vec<FaceSpec>> {
        // Split straight edges where other cells' corners touch them, so neighbours share edges.
        let corners: Vec<Vec2> = cells.iter().flat_map(|c| c.outer.iter().chain(c.holes.iter().flatten()).map(Seg2::start)).collect();
        let split_loop = |segs: &[Seg2]| -> Vec<Seg2> {
            let mut v = Vec::new();
            for e in segs {
                let Seg2::Line { a, b } = *e else {
                    v.push(*e);
                    continue;
                };
                let d = b - a;
                let l2 = d.len2();
                let mut ts: Vec<f64> = corners
                    .iter()
                    .filter_map(|p| {
                        let t = (*p - a).dot(d) / l2.max(1e-30);
                        ((t > 1e-9 && t < 1.0 - 1e-9) && (a + d * t).dist(*p) < 1e-7).then_some(t)
                    })
                    .collect();
                ts.sort_by(|x, y| x.total_cmp(y));
                ts.dedup_by(|x, y| (*x - *y).abs() < 1e-12);
                let mut prev = a;
                for t in ts {
                    let q = a + d * t;
                    v.push(Seg2::Line { a: prev, b: q });
                    prev = q;
                }
                v.push(Seg2::Line { a: prev, b });
            }
            v
        };
        let cells: Vec<Cell> = cells
            .iter()
            .map(|c| Cell { piece: c.piece, outer: split_loop(&c.outer), holes: c.holes.iter().map(|h| split_loop(h)).collect() })
            .collect();
        let near = |p: Vec2, q: Vec2| p.dist(q) < 1e-6;
        // Shared with a neighbour: the same flat edge the other way, landing in the same place
        // (flat neighbours across a corner of two bends do not).
        let shared = |ci: usize, e: &Seg2| {
            let Some(me) = cells.get(ci) else { return false };
            let here = self.map3(me.piece, e.point_at(0.5), z0);
            cells.iter().enumerate().any(|(cj, c)| {
                cj != ci
                    && c.outer.iter().chain(c.holes.iter().flatten()).any(|g| {
                        near(g.start(), e.end())
                            && near(g.end(), e.start())
                            && matches!((g, e), (Seg2::Line { .. }, Seg2::Line { .. }))
                            && self.map3(c.piece, g.point_at(0.5), z0).dist(here) < 1e-6
                    })
            })
        };
        let mut faces = Vec::new();
        for (ci, c) in cells.iter().enumerate() {
            // Top (z1, outward +z) and bottom (z0, outward −z, loops reversed).
            let loops = |z: f64, rev: bool| -> Result<Vec<Vec<EdgeSpec>>> {
                std::iter::once(&c.outer)
                    .chain(c.holes.iter())
                    .map(|l| {
                        let mut v: Vec<EdgeSpec> = l.iter().map(|e| self.edge3(c.piece, e, z)).collect::<Result<_>>()?;
                        if rev {
                            v = v.into_iter().rev().map(rev_edge).collect();
                        }
                        Ok(v)
                    })
                    .collect()
            };
            let centre = c.outer.iter().map(Seg2::start).fold(Vec2::default(), |a, b| a + b) * (1.0 / c.outer.len().max(1) as f64);
            for (z, rev, sign) in [(z1, false, 1.0), (z0, true, -1.0)] {
                let surface = match c.piece {
                    Piece::Panel(_) => SurfSpec::Plane,
                    Piece::Zone(i) => {
                        let f = self.flanges.get(i).ok_or_else(|| DocError::Invalid("bend".into()))?;
                        let ss: Vec<f64> = c.outer.iter().map(|e| (e.start() - f.p).dot(f.n)).collect();
                        let (s0, s1) = (ss.iter().cloned().fold(f64::MAX, f64::min), ss.iter().cloned().fold(f64::MIN, f64::max));
                        let us: Vec<f64> = c.outer.iter().map(|e| (e.start() - f.p).dot(f.d)).collect();
                        let (u0, u1) = (us.iter().cloned().fold(f64::MAX, f64::min), us.iter().cloned().fold(f64::MIN, f64::max));
                        let neutral = (f.radius + self.k * self.t).max(1e-12);
                        let (axis_o, k, turn) = self.bend_axis(f);
                        let wp = self.world(f.parent);
                        let q = |u: f64| f.p + f.n * s0 + f.d * u;
                        SurfSpec::Revolved {
                            a: self.map3(c.piece, q(u0), z),
                            b: self.map3(c.piece, q(u1), z),
                            axis_origin: apply_point(&wp, axis_o),
                            axis: apply_vector(&wp, k),
                            angle: turn.signum() * (s1 - s0) / neutral,
                        }
                    }
                };
                let p = self.map3(c.piece, centre, z);
                let outward = (self.map3(c.piece, centre, z + 1.0) - p) * sign;
                faces.push(FaceSpec { loops: loops(z, rev)?, surface, probe: Some((p, outward)) });
            }
            // Walls along edges no other cell shares.
            for e in c.outer.iter().chain(c.holes.iter().flatten()) {
                if shared(ci, e) {
                    continue;
                }
                let (a, b) = (e.start(), e.end());
                let bottom = self.edge3(c.piece, e, z0)?;
                let top = rev_edge(self.edge3(c.piece, e, z1)?);
                let up_b = EdgeSpec::Line { a: self.map3(c.piece, b, z0), b: self.map3(c.piece, b, z1) };
                let down_a = EdgeSpec::Line { a: self.map3(c.piece, a, z1), b: self.map3(c.piece, a, z0) };
                let surface = match (c.piece, e) {
                    (Piece::Panel(_), Seg2::Arc { .. }) => {
                        let (pa, pb, pm) = (self.map3(c.piece, a, z0), self.map3(c.piece, b, z0), self.map3(c.piece, e.point_at(0.5), z0));
                        SurfSpec::Extruded { a: pa, b: pb, mid: pm, dir: self.map3(c.piece, a, z1) - pa }
                    }
                    _ => SurfSpec::Plane,
                };
                let mid = e.point_at(0.5);
                let tdir = e.point_at(0.51) - e.point_at(0.49);
                let right = Vec2::new(tdir.y, -tdir.x);
                let pm = self.map3(c.piece, mid, (z0 + z1) / 2.0);
                let outward = self.map3(c.piece, mid + right * 1e-3, (z0 + z1) / 2.0) - pm;
                faces.push(FaceSpec { loops: vec![vec![bottom, up_b, top, down_a]], surface, probe: Some((pm, outward)) });
            }
        }
        Ok(faces)
    }

    /// The folded (or, when unfolded, flat) solid, sewn from its faces.
    pub fn solid(&self) -> Result<Body> {
        if self.flat {
            return self.flat_solid();
        }
        Ok(kernel::sew(&self.faces(&self.cells()?, 0.0, self.t)?)?)
    }

    /// The flat pattern as one extrusion (outline with its cut-outs).
    pub fn flat_solid(&self) -> Result<Body> {
        let loops = self.outline()?;
        let mut loops: Vec<Loop2> = loops.into_iter().map(|l| Loop2::polygon(&l)).collect();
        loops.sort_by(|a, b| b.signed_area().abs().total_cmp(&a.signed_area().abs()));
        let mut it = loops.into_iter();
        let outer = it.next().ok_or_else(|| DocError::Invalid("empty flat pattern".into()))?.ccw();
        let mut holes: Vec<Loop2> = it.map(|l| l.ccw().reversed()).collect();
        let op = outer.polyline(1e-3);
        for h in &self.holes {
            let hp = h.outer.polyline(1e-3);
            if hp.iter().all(|p| point_in(&op, *p)) {
                holes.push(h.outer.ccw().reversed());
            } else {
                return Err(DocError::Invalid("not supported yet: cut-outs over the flat pattern's edge".into()));
            }
        }
        let pl = plane_of(&self.frame_mat(), Vec3::ZERO, Vec3::X, Vec3::Y)?;
        kernel::extrude(&pl, &[Region2 { outer, holes }], 0.0, self.t)?.into_iter().next().ok_or_else(|| DocError::Invalid("flat pattern".into()))
    }

    /// Outline loops of the flat pattern (flat coordinates): the pieces' edges, shared ones left out.
    pub fn outline(&self) -> Result<Vec<Vec<Vec2>>> {
        let mut polys: Vec<Vec<Vec2>> = vec![dedup(&self.base.outer.ccw().polyline(1e-3))];
        for f in &self.flanges {
            polys.push(dedup(&self.zone(f).outer.ccw().polyline(1e-3)));
            if f.leg > 1e-9 {
                polys.push(dedup(&self.panel(f).outer.ccw().polyline(1e-3)));
            }
        }
        let loops = merge_loops(&polys);
        if loops.is_empty() {
            return Err(DocError::Invalid("empty flat pattern".into()));
        }
        Ok(loops)
    }

    /// The bends: angle, radius, direction, allowance and centre line (flat coordinates).
    pub fn bends(&self) -> Vec<BendInfo> {
        self.flanges
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let ba = self.allowance(f);
                let (u0, u1) = self.zone_span(f);
                let a = f.p + f.n * (ba / 2.0) + f.d * u0;
                BendInfo {
                    index: i + 1,
                    angle_deg: f.angle.abs().to_degrees(),
                    radius: f.radius,
                    up: f.angle >= 0.0,
                    allowance: ba,
                    length: u1 - u0,
                    start: a,
                    end: a + f.d * (u1 - u0),
                }
            })
            .collect()
    }

    /// Topology counted as Fusion does: a round hole is one face bounded by two closed circular
    /// edges (one vertex each), where the sewn solid splits it into arcs. Returns the change to
    /// the sewn (faces, edges, vertices).
    pub fn round_hole_correction(&self) -> (i64, i64, i64) {
        let (mut df, mut de, mut dv) = (0i64, 0i64, 0i64);
        for h in &self.holes {
            let arcs: Vec<(Vec2, f64)> =
                h.outer.segs.iter().filter_map(|s| if let Seg2::Arc { center, radius, .. } = s { Some((*center, *radius)) } else { None }).collect();
            let n = h.outer.segs.len() as i64;
            let full = !arcs.is_empty()
                && arcs.len() == h.outer.segs.len()
                && arcs.iter().all(|(c, r)| c.dist(arcs[0].0) < 1e-9 && (r - arcs[0].1).abs() < 1e-9);
            if full && n > 1 {
                df += 1 - n;
                de += 2 - 3 * n;
                dv += 2 - 2 * n;
            }
        }
        (df, de, dv)
    }

    /// Bounding size of the flat pattern (x, y in the flat frame).
    pub fn flat_size(&self) -> Result<(f64, f64)> {
        let loops = self.outline()?;
        let (mut lo, mut hi) = (Vec2::new(f64::MAX, f64::MAX), Vec2::new(f64::MIN, f64::MIN));
        for p in loops.iter().flatten() {
            lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
        }
        Ok((hi.x - lo.x, hi.y - lo.y))
    }

    /// Panels with their regions and world placements: (flange index or None, region).
    fn panels(&self) -> Vec<(Option<usize>, Region2)> {
        let mut v = vec![(None, self.base.clone())];
        for (i, f) in self.flanges.iter().enumerate() {
            if f.leg > 1e-9 {
                v.push((Some(i), self.panel(f)));
            }
        }
        v
    }

    /// The panel edge nearest a world point: (panel, edge a, edge b in flat coordinates, on the
    /// top face?, distance).
    pub fn edge_at(&self, p: Vec3) -> Option<(Option<usize>, Vec2, Vec2, bool, f64)> {
        let mut best: Option<(Option<usize>, Vec2, Vec2, bool, f64)> = None;
        for (pi, r) in self.panels() {
            let w = self.world(pi);
            let Some(inv) = mat_inverse(&w) else { continue };
            let q = apply_point(&inv, p);
            let pts = r.outer.ccw().polyline(1e-3);
            let m = pts.len();
            for i in 0..m {
                let (Some(&a), Some(&b)) = (pts.get(i), pts.get((i + 1) % m)) else { continue };
                if a.dist(b) < 1e-9 {
                    continue;
                }
                for (z, top) in [(0.0, false), (self.t, true)] {
                    let q2 = Vec2::new(q.x, q.y);
                    let d = b - a;
                    let t = ((q2 - a).dot(d) / d.len2()).clamp(0.0, 1.0);
                    let dist = ((a + d * t) - q2).len().hypot(q.z - z);
                    if best.as_ref().is_none_or(|x| dist < x.4) {
                        best = Some((pi, a, b, top, dist));
                    }
                }
            }
        }
        best
    }

    /// Trim a panel's edge a→b back by `by` (toward the panel interior).
    fn trim(&mut self, panel: Option<usize>, a: Vec2, b: Vec2, by: f64) -> Result<()> {
        if by.abs() < 1e-12 {
            return Ok(());
        }
        let d = (b - a).normalized().ok_or_else(|| DocError::Invalid("edge".into()))?;
        // Interior side of a counter-clockwise loop is to the left.
        let inward = Vec2::new(-d.y, d.x);
        match panel {
            None => {
                let pts = self.base.outer.ccw().polyline(1e-3);
                let cut = clip(&pts, a + inward * by, inward);
                if cut.len() < 3 {
                    return Err(DocError::Invalid("the flange's setback removes the whole panel".into()));
                }
                self.base = Region2 { outer: Loop2::polygon(&cut).ccw(), holes: self.base.holes.clone() };
            }
            Some(i) => {
                // A flange panel: its far edge shortens the leg; side edges are not trimmed.
                let Some(f) = self.flanges.get_mut(i) else { return Err(DocError::Invalid("flange".into())) };
                if inward.dot(f.n * -1.0) > 0.99 {
                    f.leg -= by;
                    if f.leg < -1e-9 {
                        return Err(DocError::Invalid("the flange's setback is longer than the panel".into()));
                    }
                } else {
                    return Err(DocError::Invalid("not supported yet: a flange on the side of another flange".into()));
                }
            }
        }
        Ok(())
    }

    /// Add an edge flange on the panel edge nearest `pick` (world): `height` to the outer face
    /// from the virtual sharp, `angle` (0…180°), inside / outside / middle bend position.
    pub fn add_flange(&mut self, pick: Vec3, height: f64, angle: f64, radius: f64, position: &str, flip: bool) -> Result<usize> {
        self.add_flanges(&[pick], height, angle, radius, position, flip)?.into_iter().next().ok_or_else(|| DocError::Invalid("flange".into()))
    }

    /// Edge flanges on several edges at once: every edge is trimmed back first, so flanges on
    /// adjacent edges stop at each other's bends (the corners stay open).
    pub fn add_flanges(&mut self, picks: &[Vec3], height: f64, angle: f64, radius: f64, position: &str, flip: bool) -> Result<Vec<usize>> {
        if !(angle > 1e-6 && angle <= std::f64::consts::PI - 1e-6) {
            return Err(DocError::Invalid("the flange angle must be between 0 and 180°".into()));
        }
        if picks.is_empty() || picks.len() > 1000 {
            return Err(DocError::Invalid("pick 1…1000 edges".into()));
        }
        let sharp = (radius + self.t) * (angle / 2.0).tan();
        let setback = match position {
            "inside" | "" => sharp,
            "outside" => 0.0,
            "middle" | "mid" => sharp / 2.0,
            other => return Err(DocError::Invalid(format!("unknown bend position `{other}` (inside, outside or middle)"))),
        };
        let leg = height - sharp;
        if leg < -1e-9 {
            return Err(DocError::Invalid(format!("the flange height must be at least {sharp:.3} mm for this bend")));
        }
        // Find every edge before anything moves.
        let mut found = Vec::new();
        for p in picks {
            let (panel, a, b, top, dist) = self.edge_at(*p).ok_or_else(|| DocError::Invalid("no sheet edge there".into()))?;
            if dist > self.t * 2.0 + 1e-3 {
                return Err(DocError::Invalid("pick an edge of the sheet's top or bottom face".into()));
            }
            let d = (b - a).normalized().ok_or_else(|| DocError::Invalid("edge".into()))?;
            found.push((panel, a, d, top));
        }
        for (panel, a, d, _) in &found {
            self.trim(*panel, *a, *a + *d, setback)?;
        }
        let mut made = Vec::new();
        for (panel, a, d, top) in found {
            let outward = Vec2::new(d.y, -d.x);
            let line_o = a - outward * setback;
            // The trimmed edge: along d, on the moved line.
            let (start, len) = match panel {
                None => {
                    let pts = self.base.outer.ccw().polyline(1e-3);
                    let m = pts.len();
                    (0..m)
                        .filter_map(|k| {
                            let (p0, p1) = (*pts.get(k)?, *pts.get((k + 1) % m)?);
                            let e = p1 - p0;
                            let on = (p0 - line_o).dot(outward).abs() < 1e-6 && (p1 - line_o).dot(outward).abs() < 1e-6;
                            (on && e.dot(d) > 0.0).then_some((p0, e.len()))
                        })
                        .next()
                        .ok_or_else(|| DocError::Invalid("the flange's edge was trimmed away".into()))?
                }
                Some(i) => {
                    let f = self.flanges.get(i).ok_or_else(|| DocError::Invalid("flange".into()))?;
                    let u0 = (line_o - f.p).dot(f.d).clamp(0.0, f.len);
                    (f.p + f.n * (self.allowance(f) + f.leg) + f.d * u0, f.len)
                }
            };
            let up = top != flip;
            self.flanges.push(SheetFlange {
                parent: panel,
                p: snap(start),
                d,
                len,
                n: outward,
                angle: if up { angle } else { -angle },
                radius,
                leg: leg.max(0.0),
                shape: None,
                ext: (0.0, 0.0),
            });
            made.push(self.flanges.len() - 1);
        }
        self.close_corners(&made);
        Ok(made)
    }

    /// Where two new 90° flanges on the base meet at a corner, run their panels on toward each
    /// other until the gap between their inner corners is the rule's gap.
    fn close_corners(&mut self, made: &[usize]) {
        let ends: Vec<(usize, bool, Vec2)> = made
            .iter()
            .filter_map(|&i| self.flanges.get(i).map(|f| (i, f)))
            .filter(|(_, f)| f.parent.is_none() && (f.angle.abs() - std::f64::consts::FRAC_PI_2).abs() < 1e-9)
            .flat_map(|(i, f)| [(i, false, f.p), (i, true, f.p + f.d * f.len)])
            .collect();
        for &(i, at_end, c) in &ends {
            let Some(f) = self.flanges.get(i).cloned() else { continue };
            let other = ends.iter().find(|&&(j, _, c2)| j != i && c2.dist(c) < 1e-6).and_then(|&(j, _, _)| self.flanges.get(j).cloned());
            let Some(g) = other else { continue };
            if g.n.dot(f.n).abs() > 1e-6 || (g.angle > 0.0) != (f.angle > 0.0) {
                continue;
            }
            // The neighbour's setback: its bend starts that far in from the corner of the outer faces.
            let setback = (g.radius + self.t) * (g.angle.abs() / 2.0).tan();
            let ext = setback - (self.t + self.gap / std::f64::consts::SQRT_2);
            if ext <= 1e-9 {
                continue;
            }
            if let Some(x) = self.flanges.get_mut(i) {
                if at_end {
                    x.ext.1 = ext;
                } else {
                    x.ext.0 = ext;
                }
            }
        }
    }

    /// A flat hem: a 180° bend with a tiny radius at the panel edge, `length` to its outer face.
    pub fn add_hem(&mut self, pick: Vec3, length: f64, gap: f64, flip: bool) -> Result<usize> {
        let (panel, a, b, top, dist) = self.edge_at(pick).ok_or_else(|| DocError::Invalid("no sheet edge there".into()))?;
        if dist > self.t * 2.0 + 1e-3 {
            return Err(DocError::Invalid("pick an edge of the sheet".into()));
        }
        let d = (b - a).normalized().ok_or_else(|| DocError::Invalid("edge".into()))?;
        let outward = Vec2::new(d.y, -d.x);
        let leg = length - (gap + self.t);
        if leg < 0.0 {
            return Err(DocError::Invalid("the hem is shorter than its bend".into()));
        }
        // A hem folds back over the face it starts from (toward the bottom unless flipped).
        let up = !top == flip;
        let up = !up;
        self.flanges.push(SheetFlange {
            parent: panel,
            p: a,
            d,
            len: a.dist(b),
            n: outward,
            angle: if up { std::f64::consts::PI } else { -std::f64::consts::PI },
            radius: gap,
            leg,
            ext: (0.0, 0.0),
            shape: None,
        });
        Ok(self.flanges.len() - 1)
    }

    /// Fold the base panel along a line on its top or bottom face (world points `a`, `b`).
    /// The bend zone (width BA = θ·(R + K·T)) is taken out of the existing sheet, so the flat
    /// pattern keeps its size; `position` puts the line at the zone's centre (`centerline`),
    /// start or end (`start`/`end`, from the fixed side), or on the mould line (`mould`: where
    /// the outer faces meet, so the fixed side measures the same to it folded as flat).
    /// The side holding `fixed` (default: the larger side) stays put; `flip` folds the other way.
    #[allow(clippy::too_many_arguments)]
    pub fn add_fold(&mut self, a: Vec3, b: Vec3, angle: f64, radius: f64, position: &str, flip: bool, fixed: Option<Vec3>) -> Result<usize> {
        if !(angle > 1e-6 && angle <= std::f64::consts::PI - 1e-6) {
            return Err(DocError::Invalid("the fold angle must be between 0 and 180°".into()));
        }
        if !(radius >= 0.0) {
            return Err(DocError::Invalid("the bend radius must not be negative".into()));
        }
        let inv = mat_inverse(&self.world(None)).ok_or_else(|| DocError::Invalid("sheet frame".into()))?;
        let (fa, fb) = (apply_point(&inv, a), apply_point(&inv, b));
        let on_face = |q: Vec3| q.z.abs() < 1e-4 || (q.z - self.t).abs() < 1e-4;
        if !(on_face(fa) && on_face(fb)) {
            return Err(DocError::Invalid("the fold line must lie on the base face of the sheet (folds on flanges are not supported yet)".into()));
        }
        let (a2, b2) = (Vec2::new(fa.x, fa.y), Vec2::new(fb.x, fb.y));
        let d = (b2 - a2).normalized().ok_or_else(|| DocError::Invalid("the fold line has no length".into()))?;
        let pts = dedup(&self.base.outer.ccw().polyline(1e-3));
        let mut n = Vec2::new(-d.y, d.x);
        let l0 = a2.dot(n);
        // Which side moves: away from `fixed`, else the smaller side.
        let side_area = |n: Vec2| signed_area(&clip(&pts, n * l0, n)).abs();
        let moves_n = match fixed {
            Some(p) => {
                let q = apply_point(&inv, p);
                Vec2::new(q.x, q.y).dot(n) < l0
            }
            None => side_area(n) <= side_area(n * -1.0),
        };
        if !moves_n {
            n = n * -1.0;
        }
        let l = a2.dot(n);
        let probe = SheetFlange { parent: None, p: a2, d, len: 1.0, n, angle, radius, leg: 0.0, ext: (0.0, 0.0), shape: None };
        let ba = self.allowance(&probe);
        let s0 = match position {
            "centerline" | "center" | "centre" => l - ba / 2.0,
            "start" => l,
            "end" => l - ba,
            "mould" | "mold" => l - (radius + self.t) * (angle / 2.0).tan(),
            o => return Err(DocError::Invalid(format!("unknown fold position `{o}` (centerline, start, end or mould)"))),
        };
        let fixed_part = clip(&pts, n * s0, n * -1.0);
        let moving = clip(&pts, n * (s0 + ba), n);
        let strip = clip(&clip(&pts, n * s0, n), n * (s0 + ba), n * -1.0);
        if signed_area(&fixed_part).abs() < 1e-9 || signed_area(&moving).abs() < 1e-9 {
            return Err(DocError::Invalid("the fold line must cross the sheet with material on both sides of the bend".into()));
        }
        // The bend zone must be a rectangle across the sheet.
        let us: Vec<f64> = strip.iter().map(|q| q.dot(d)).collect();
        let (u0, u1) = (us.iter().cloned().fold(f64::MAX, f64::min), us.iter().cloned().fold(f64::MIN, f64::max));
        if ((u1 - u0) * ba - signed_area(&strip).abs()).abs() > 1e-6 * (u1 - u0).max(1.0) * ba.max(1.0) {
            return Err(DocError::Invalid("not supported yet: folds whose bend zone crosses a slanted or curved edge of the sheet".into()));
        }
        if self.holes.iter().any(|h| {
            let hp = dedup(&h.outer.ccw().polyline(1e-3));
            hp.iter().any(|q| point_in(&strip, *q)) || polys_cross(&strip, &hp)
        }) {
            return Err(DocError::Invalid("not supported yet: folds through a cut-out".into()));
        }
        let up = !flip;
        let leg = moving.iter().map(|q| q.dot(n) - s0 - ba).fold(0.0, f64::max);
        let p0 = n * s0 + d * u0;
        let mut ccw = moving.clone();
        if signed_area(&ccw) < 0.0 {
            ccw.reverse();
        }
        self.base = Region2 { outer: Loop2::polygon(&fixed_part).ccw(), holes: self.base.holes.clone() };
        self.flanges.push(SheetFlange {
            parent: None,
            p: snap(p0),
            d,
            len: u1 - u0,
            n,
            angle: if up { angle } else { -angle },
            radius,
            leg,
            ext: (0.0, 0.0),
            shape: Some(ccw.into_iter().map(snap).collect()),
        });
        let i = self.flanges.len() - 1;
        // Flanges on the moving part's edges now hang off the fold and move with it.
        for k in 0..i {
            let Some(f) = self.flanges.get(k) else { continue };
            if f.parent.is_some() {
                continue;
            }
            let (e0, e1) = ((f.p - p0).dot(n), (f.p + f.d * f.len - p0).dot(n));
            if e0.min(e1) >= ba - 1e-9 {
                if let Some(f) = self.flanges.get_mut(k) {
                    f.parent = Some(i);
                }
            } else if e0.max(e1) > 1e-9 {
                return Err(DocError::Invalid("not supported yet: a fold through a flange's bend".into()));
            }
        }
        Ok(i)
    }

    /// Map a region in a world plane to flat coordinates through a panel's placement.
    pub fn region_to_flat(&self, panel: Option<usize>, plane: &Plane, r: &Region2) -> Option<Region2> {
        let inv = mat_inverse(&self.world(panel))?;
        let map = |p: Vec2| {
            let q = apply_point(&inv, plane.to_world(p));
            Vec2::new(q.x, q.y)
        };
        // A rigid map of the plane: lines and arcs stay lines and arcs (mirrored or not).
        let (o, ex, ey) = (map(Vec2::default()), map(Vec2::new(1.0, 0.0)), map(Vec2::new(0.0, 1.0)));
        let (ax, ay) = (ex - o, ey - o);
        let mirrored = ax.cross(ay) < 0.0;
        let rot = ax.y.atan2(ax.x);
        let seg = |sg: &Seg2| -> Seg2 {
            match *sg {
                Seg2::Line { a, b } => Seg2::Line { a: map(a), b: map(b) },
                Seg2::Arc { center, radius, start, sweep } => {
                    if mirrored {
                        Seg2::Arc { center: map(center), radius, start: rot - start, sweep: -sweep }
                    } else {
                        Seg2::Arc { center: map(center), radius, start: rot + start, sweep }
                    }
                }
                other => Seg2::Line { a: map(other.start()), b: map(other.end()) },
            }
        };
        let curved_other = r.outer.segs.iter().any(|x| !matches!(x, Seg2::Line { .. } | Seg2::Arc { .. }));
        let mut lp = if curved_other {
            Loop2::polygon(&r.outer.polyline(1e-3).into_iter().map(map).collect::<Vec<_>>())
        } else {
            Loop2 { segs: r.outer.segs.iter().map(seg).collect() }
        };
        if lp.signed_area() < 0.0 {
            lp = lp.reversed();
        }
        Some(Region2 { outer: lp, holes: Vec::new() })
    }

    /// Panels whose plane is parallel to `plane` (world): (panel, region).
    pub fn parallel_panels(&self, plane: &Plane) -> Vec<(Option<usize>, Region2)> {
        self.panels()
            .into_iter()
            .filter(|(pi, _)| {
                let w = self.world(*pi);
                let n = apply_vector(&w, Vec3::Z);
                n.cross(plane.normal()).len() < 1e-6
            })
            .collect()
    }
}

fn point_in(poly: &[Vec2], p: Vec2) -> bool {
    let mut inside = false;
    let m = poly.len();
    for i in 0..m {
        let (Some(&a), Some(&b)) = (poly.get(i), poly.get((i + m - 1) % m)) else { continue };
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
    }
    inside
}

/// The boundary of a union of counter-clockwise polygons that meet edge to edge: edges are
/// split where corners touch them, edges running both ways cancel, the rest chain into loops
/// (outer loops counter-clockwise, holes clockwise; straight runs merged).
fn merge_loops(polys: &[Vec<Vec2>]) -> Vec<Vec<Vec2>> {
    let mut segs: Vec<(Vec2, Vec2)> = Vec::new();
    for p in polys {
        let m = p.len();
        for i in 0..m {
            if let (Some(&a), Some(&b)) = (p.get(i), p.get((i + 1) % m)) {
                segs.push((a, b));
            }
        }
    }
    let pts: Vec<Vec2> = segs.iter().flat_map(|(a, b)| [*a, *b]).collect();
    let mut split: Vec<(Vec2, Vec2)> = Vec::new();
    for (a, b) in &segs {
        let d = *b - *a;
        let l2 = d.len2();
        if l2 < 1e-18 {
            continue;
        }
        let mut ts: Vec<f64> = pts
            .iter()
            .filter_map(|p| {
                let t = (*p - *a).dot(d) / l2;
                ((t > 1e-9 && t < 1.0 - 1e-9) && (*a + d * t).dist(*p) < 1e-7).then_some(t)
            })
            .collect();
        ts.push(0.0);
        ts.push(1.0);
        ts.sort_by(|x, y| x.total_cmp(y));
        ts.dedup_by(|x, y| (*x - *y).abs() < 1e-12);
        for w in ts.windows(2) {
            split.push((*a + d * w[0], *a + d * w[1]));
        }
    }
    let same = |p: Vec2, q: Vec2| p.dist(q) < 1e-6;
    let keep: Vec<(Vec2, Vec2)> = split
        .iter()
        .enumerate()
        .filter(|(i, (a, b))| !split.iter().enumerate().any(|(j, (c, d))| j != *i && same(*a, *d) && same(*b, *c)))
        .map(|(_, s)| *s)
        .collect();
    let mut loops = Vec::new();
    let mut used = vec![false; keep.len()];
    for s0 in 0..keep.len() {
        if used.get(s0).copied().unwrap_or(true) {
            continue;
        }
        let mut lp = Vec::new();
        let mut cur = s0;
        for _ in 0..keep.len() + 1 {
            if let Some(u) = used.get_mut(cur) {
                *u = true;
            }
            let Some(&(a, b)) = keep.get(cur) else { break };
            lp.push(a);
            match (0..keep.len()).find(|j| !used.get(*j).copied().unwrap_or(true) && keep.get(*j).is_some_and(|(c, _)| same(*c, b))) {
                Some(n) => cur = n,
                None => break,
            }
        }
        let m = lp.len();
        let clean: Vec<Vec2> = (0..m)
            .filter_map(|i| {
                let (p, q, r) = (*lp.get((i + m - 1) % m)?, *lp.get(i)?, *lp.get((i + 1) % m)?);
                ((q - p).cross(r - q).abs() > 1e-9 * (q - p).len().max(1.0) * (r - q).len().max(1.0)).then_some(q)
            })
            .collect();
        if clean.len() >= 3 {
            loops.push(clean);
        }
    }
    loops
}

fn signed_area(p: &[Vec2]) -> f64 {
    let m = p.len();
    (0..m).filter_map(|i| Some(p.get(i)?.cross(*p.get((i + 1) % m)?))).sum::<f64>() / 2.0
}

/// A convex polygon minus a convex polygon (both counter-clockwise), as convex pieces.
fn convex_diff(a: &[Vec2], h: &[Vec2]) -> Vec<Vec<Vec2>> {
    let mut out = Vec::new();
    let mut rest = a.to_vec();
    let m = h.len();
    for i in 0..m {
        let (Some(&p), Some(&q)) = (h.get(i), h.get((i + 1) % m)) else { continue };
        let Some(d) = (q - p).normalized() else { continue };
        let left = Vec2::new(-d.y, d.x);
        let outside = dedup(&clip(&rest, p, left * -1.0));
        if outside.len() >= 3 && signed_area(&outside).abs() > 1e-12 {
            out.push(outside);
        }
        rest = dedup(&clip(&rest, p, left));
        if rest.len() < 3 {
            break;
        }
    }
    out
}

/// Do two polygons' edges cross?
fn polys_cross(a: &[Vec2], b: &[Vec2]) -> bool {
    let seg = |p: &[Vec2], i: usize| -> Option<(Vec2, Vec2)> { Some((*p.get(i)?, *p.get((i + 1) % p.len())?)) };
    (0..a.len()).filter_map(|i| seg(a, i)).any(|(p, q)| {
        (0..b.len()).filter_map(|j| seg(b, j)).any(|(r, s)| {
            let d1 = (q - p).cross(r - p);
            let d2 = (q - p).cross(s - p);
            let d3 = (s - r).cross(p - r);
            let d4 = (s - r).cross(q - r);
            d1 * d2 < -1e-12 && d3 * d4 < -1e-12
        })
    })
}

fn is_convex(p: &[Vec2]) -> bool {
    let m = p.len();
    (0..m).all(|i| match (p.get(i), p.get((i + 1) % m), p.get((i + 2) % m)) {
        (Some(a), Some(b), Some(c)) => (*b - *a).cross(*c - *b) >= -1e-9,
        _ => true,
    })
}

/// A sheet from a closed profile (base flange): the region in `plane`, thickness along its normal.
pub fn base_flange(body: &str, rule: &str, rv: &RuleValues, plane: Plane, region: Region2) -> SheetBody {
    SheetBody {
        body: body.into(),
        rule: rule.into(),
        t: rv.thickness,
        k: rv.k_factor,
        frame: plane,
        base: region,
        flanges: Vec::new(),
        holes: Vec::new(),
        flat: false,
        gap: rv.gap,
    }
}

/// A sheet from an open chain of lines (contour flange): `pts` in `plane`, the material on the
/// right of the travel (or the left when `flip`), `width` along the plane normal.
pub fn contour_flange(body: &str, rule: &str, rv: &RuleValues, plane: &Plane, pts: &[Vec2], width: f64, flip: bool) -> Result<SheetBody> {
    if pts.len() < 2 {
        return Err(DocError::Invalid("a contour flange needs an open chain of lines".into()));
    }
    let (t, r) = (rv.thickness, rv.bend_radius);
    let seg_dir = |i: usize| -> Result<(Vec2, f64)> {
        let (Some(a), Some(b)) = (pts.get(i), pts.get(i + 1)) else { return Err(DocError::Invalid("chain".into())) };
        let l = a.dist(*b);
        Ok(((*b - *a).normalized().ok_or_else(|| DocError::Invalid("zero-length line in the contour".into()))?, l))
    };
    let side = if flip { -1.0 } else { 1.0 };
    // Material side in the sketch: right of travel.
    let (d0, _) = seg_dir(0)?;
    let m0 = Vec2::new(d0.y, -d0.x) * side;
    let start = pts.first().copied().unwrap_or_default();
    let (x, z) = (plane.dir_to_world(d0), plane.dir_to_world(m0));
    let frame = Plane::new(plane.to_world(start), x, z.cross(x)).ok_or_else(|| DocError::Invalid("contour frame".into()))?;
    // Width along the sketch normal: +y or −y of the frame.
    let ysign = if frame.y.dot(plane.normal()) >= 0.0 { 1.0 } else { -1.0 };
    let n = pts.len() - 1;
    // Turn at each corner: angle and whether it bends toward the material (up).
    let mut corners = Vec::new();
    for i in 0..n.saturating_sub(1) {
        let (da, _) = seg_dir(i)?;
        let (db, _) = seg_dir(i + 1)?;
        let cross = da.cross(db) * side;
        let ang = da.dot(db).clamp(-1.0, 1.0).acos();
        if ang < 1e-9 {
            return Err(DocError::Invalid("straight corners in a contour flange".into()));
        }
        // Right-hand material: turning right (negative cross) bends toward it.
        corners.push((ang, cross < 0.0));
    }
    let setback = |ang: f64, up: bool| if up { (r + t) * (ang / 2.0).tan() } else { r * (ang / 2.0).tan() };
    let mut sheet = SheetBody {
        body: body.into(),
        rule: rule.into(),
        t,
        k: rv.k_factor,
        frame,
        base: Region2 { outer: Loop2 { segs: Vec::new() }, holes: Vec::new() },
        flanges: Vec::new(),
        holes: Vec::new(),
        flat: false,
        gap: rv.gap,
    };
    let (y0, y1) = if ysign > 0.0 { (0.0, width) } else { (-width, 0.0) };
    // First segment: the base panel.
    let (_, l0) = seg_dir(0)?;
    let end0 = l0 - corners.first().map(|(a, u)| setback(*a, *u)).unwrap_or(0.0);
    if end0 <= 0.0 {
        return Err(DocError::Invalid("a contour line is shorter than its bends".into()));
    }
    sheet.base = rect(Vec2::new(0.0, y0), Vec2::new(end0, y0), Vec2::new(end0, y1), Vec2::new(0.0, y1));
    let mut parent: Option<usize> = None;
    let mut at = end0;
    for (i, (ang, up)) in corners.iter().enumerate() {
        let (_, l) = seg_dir(i + 1)?;
        let before = setback(*ang, *up);
        let after = corners.get(i + 1).map(|(a, u)| setback(*a, *u)).unwrap_or(0.0);
        let leg = l - before - after;
        if leg < -1e-9 {
            return Err(DocError::Invalid("a contour line is shorter than its bends".into()));
        }
        // The material flips sides relative to the panel when the bend goes down.
        let f = SheetFlange {
            parent,
            p: Vec2::new(at, y0),
            d: Vec2::new(0.0, 1.0),
            len: y1 - y0,
            n: Vec2::new(1.0, 0.0),
            angle: if *up { *ang } else { -*ang },
            radius: r,
            leg: leg.max(0.0),
            ext: (0.0, 0.0),
            shape: None,
        };
        let ba = sheet.allowance(&f);
        sheet.flanges.push(f);
        parent = Some(sheet.flanges.len() - 1);
        at += ba + leg.max(0.0);
    }
    Ok(sheet)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steel() -> RuleValues {
        RuleValues {
            thickness: 2.5,
            k_factor: 0.44,
            bend_radius: 2.5,
            relief_width: 2.5,
            relief_depth: 1.25,
            corner_relief: 10.0,
            hem_gap: 0.01,
            gap: 2.5,
        }
    }

    fn plate(w: f64, h: f64) -> SheetBody {
        let r = rect(Vec2::new(0.0, 0.0), Vec2::new(w, 0.0), Vec2::new(w, h), Vec2::new(0.0, h));
        base_flange(
            "Body1",
            "Steel (mm)",
            &steel(),
            Plane::new(Vec3::ZERO, Vec3::X, Vec3::Y)
                .unwrap_or_else(|| Plane::named("XY").unwrap_or(Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Y })),
            r,
        )
    }

    fn vol(b: &Body) -> f64 {
        kernel::measure(b).map(|m| m.volume).unwrap_or(f64::NAN)
    }

    #[test]
    fn edge_flange_matches_fusion() {
        // Oracle 68: 100 x 60 plate, 20 mm flange on the y = 0 top edge.
        let mut s = plate(100.0, 60.0);
        s.add_flange(Vec3::new(50.0, 0.0, 2.5), 20.0, std::f64::consts::FRAC_PI_2, 2.5, "inside", false).unwrap();
        let b = s.solid().unwrap();
        assert!((vol(&b) - 18972.622).abs() < 2.0, "{}", vol(&b));
        let (w, h) = s.flat_size().unwrap();
        assert!((w - 100.0).abs() < 1e-9 && (h - 75.654867).abs() < 1e-5, "{w} {h}");
        let bends = s.bends();
        assert_eq!(bends.len(), 1);
        assert!((bends[0].allowance - std::f64::consts::FRAC_PI_2 * 3.6).abs() < 1e-9);
        // Unfolded: the same volume, flat.
        s.flat = true;
        let f = s.solid().unwrap();
        assert!((vol(&f) - 100.0 * 75.654867 * 2.5).abs() < 2.0);
        // Oracle 70: 45°, height 15.
        let mut s = plate(100.0, 60.0);
        s.add_flange(Vec3::new(50.0, 0.0, 2.5), 15.0, std::f64::consts::FRAC_PI_4, 2.5, "inside", false).unwrap();
        assert!((vol(&s.solid().unwrap()) - 18450.777).abs() < 2.0);
        assert!((s.flat_size().unwrap().1 - 73.685298).abs() < 1e-5);
    }

    #[test]
    fn contour_flange_matches_fusion() {
        // Oracle 72: XZ chain (0,0)→(40,0)→(40,−30)→(70,−30), 50 wide.
        let xz = Plane::new(Vec3::ZERO, Vec3::X, Vec3::new(0.0, 0.0, -1.0)).unwrap();
        let pts = [Vec2::new(0.0, 0.0), Vec2::new(40.0, 0.0), Vec2::new(40.0, -30.0), Vec2::new(70.0, -30.0)];
        let s = contour_flange("Body1", "Steel (mm)", &steel(), &xz, &pts, 50.0, false).unwrap();
        let b = s.solid().unwrap();
        assert!((vol(&b) - 12097.622).abs() < 1.2, "{}", vol(&b));
        let m = kernel::measure(&b).unwrap();
        assert!((m.bbox.max.z - 32.5).abs() < 1e-6 && (m.bbox.max.y - 50.0).abs() < 1e-6, "{:?}", m.bbox);
        assert!((s.flat_size().unwrap().0 - 96.309734).abs() < 1e-5, "{:?}", s.flat_size());
    }

    #[test]
    fn hem_and_holes() {
        // Oracle 73: flange then a flat hem of 8 on its top edge.
        let mut s = plate(100.0, 60.0);
        s.add_flange(Vec3::new(50.0, 0.0, 2.5), 20.0, std::f64::consts::FRAC_PI_2, 2.5, "inside", false).unwrap();
        s.add_hem(Vec3::new(50.0, 0.0, 20.0), 8.0, 0.01, false).unwrap();
        let b = s.solid().unwrap();
        assert!((vol(&b) - 21334.723).abs() < 2.0, "{}", vol(&b));
        assert!((s.flat_size().unwrap().1 - 84.632035).abs() < 1e-4, "{:?}", s.flat_size());
        // A hole in the base and a slot across the bend.
        let mut s = plate(80.0, 50.0);
        s.add_flange(Vec3::new(40.0, 0.0, 2.5), 20.0, std::f64::consts::FRAC_PI_2, 2.5, "inside", false).unwrap();
        s.holes.push(Region2 { outer: Loop2::circle(Vec2::new(40.0, 30.0), 3.0), holes: vec![] });
        let v0 = vol(&s.solid().unwrap());
        s.holes.push(rect(Vec2::new(35.0, -12.0), Vec2::new(45.0, -12.0), Vec2::new(45.0, 10.0), Vec2::new(35.0, 10.0)));
        let v1 = vol(&s.solid().unwrap());
        let ba = std::f64::consts::FRAC_PI_2 * 3.6;
        let want = 10.0 * 2.5 * (22.0 - ba) + 10.0 * std::f64::consts::FRAC_PI_2 * (2.5 + 1.25) * 2.5;
        assert!((v0 - v1 - want).abs() < 1.0, "{v0} {v1} {want}");
        s.flat = true;
        assert!(s.solid().is_ok());
    }
}
