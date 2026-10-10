//! Linked (associative) reference geometry: curves and points projected into a sketch from
//! model edges, faces, bodies, other sketches and the origin. The sketch only stores the link
//! (what it refers to) and the geometry it produced last time; resolving a link against the
//! model happens in the document crate, which hands the new geometry to [`Sketch::update_link`].
//!
//! Linked points are fixed and linked circles keep their radius, so the solver treats them as
//! constants; they still bound profiles and take constraints and dimensions like any curve.

use serde::{Deserialize, Serialize};
use solvecraft_geom::{Vec2, Vec3};

use crate::model::{Curve, CurveKind, Result, Sketch, SketchError};

/// Points of one link closer than this are merged into one point.
const MERGE_TOL: f64 = 1e-7;
/// Most entities a single link may produce.
pub const MAX_LINK_ENTITIES: usize = 20_000;

/// How the geometry was brought in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    /// Projected onto the sketch plane along its normal.
    #[default]
    Project,
    /// Included as is (curves in the sketch plane; others are flattened onto it).
    Include,
    /// Section curves of a body or face with the sketch plane.
    Intersect,
    /// Text outlines (drawn like normal geometry; edit the text to change them).
    Text,
}

/// What a link refers to. Model geometry is referenced by a point on it plus the body name and
/// re-found on every recompute (like fillet edges), so it follows upstream edits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LinkSource {
    /// A body edge through `at`.
    Edge { body: String, at: Vec3 },
    /// The boundary loops of the body face through `at` (Intersect: the face's section).
    Face { body: String, at: Vec3 },
    /// A whole body: its silhouette (Project) or its section (Intersect).
    Body { body: String },
    /// A body vertex at `at`.
    Vertex { body: String, at: Vec3 },
    /// A curve of another sketch.
    SketchCurve { sketch: u64, curve: String },
    /// A point of another sketch.
    SketchPoint { sketch: u64, point: String },
    /// The model origin.
    Origin,
    /// An origin axis (`X`, `Y`, `Z`).
    Axis { name: String },
    /// An origin plane (`XY`, `XZ`, `YZ`) or a construction plane (by name): its trace on the
    /// sketch plane.
    Plane { name: String },
    /// A curve of a sketch projected onto a body face (through `at`) along the normal of the
    /// sketch that holds the link: a 3D curve.
    OnSurface { sketch: u64, curve: String, body: String, at: Vec3 },
    /// Where two bodies or faces (`Body` / `Face` sources) meet: 3D curves.
    Intersection { a: Box<LinkSource>, b: Box<LinkSource> },
    /// An isoparametric curve of the face through `at`: `dir` is `u` (around a curved face),
    /// `v` (along it) or an explicit direction `[x, y, z]` (as text).
    Iso { body: String, at: Vec3, dir: String },
    /// The outline of a body spun about an axis (world line), laid into the sketch plane.
    Spun { body: String, origin: Vec3, dir: Vec3 },
    /// Text: baseline start `at` (sketch coordinates), capital height and angle (radians).
    Text {
        text: String,
        at: Vec2,
        height: f64,
        #[serde(default)]
        angle: f64,
    },
}

impl LinkSource {
    /// Short description for messages and the browser.
    pub fn describe(&self) -> String {
        match self {
            LinkSource::Edge { body, .. } => format!("edge of {body}"),
            LinkSource::Face { body, .. } => format!("face of {body}"),
            LinkSource::Body { body } => body.clone(),
            LinkSource::Vertex { body, .. } => format!("vertex of {body}"),
            LinkSource::SketchCurve { curve, .. } => format!("sketch curve {curve}"),
            LinkSource::SketchPoint { point, .. } => format!("sketch point {point}"),
            LinkSource::Origin => "origin".into(),
            LinkSource::Axis { name } => format!("{name} axis"),
            LinkSource::Plane { name } => format!("{name} plane"),
            LinkSource::OnSurface { curve, .. } => format!("{curve} on a face"),
            LinkSource::Intersection { a, b } => format!("intersection of {} and {}", a.describe(), b.describe()),
            LinkSource::Spun { body, .. } => format!("spun profile of {body}"),
            LinkSource::Iso { body, .. } => format!("isoparametric curve of {body}"),
            LinkSource::Text { text, .. } => format!("text \"{}\"", text.chars().take(20).collect::<String>()),
        }
    }
}

/// One link and its state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Link {
    pub id: String,
    #[serde(default)]
    pub kind: LinkKind,
    pub source: LinkSource,
    /// Persistent model face identity, resolved by the document before geometry refresh.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face_name: Option<String>,
    /// The reference could not be found on the last recompute; the geometry is the last good one.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub lost: bool,
}

/// Geometry a link produces, in sketch coordinates.
#[derive(Clone, Debug, PartialEq)]
pub enum LinkGeom {
    Point(Vec2),
    Line(Vec2, Vec2),
    Circle(Vec2, f64),
    /// Counter-clockwise arc around `c` from `a` to `b`.
    Arc {
        c: Vec2,
        a: Vec2,
        b: Vec2,
    },
    /// Conic from `a` to `b` with control `apex` (rho 0.5: a quadratic Bézier).
    Conic {
        a: Vec2,
        apex: Vec2,
        b: Vec2,
        rho: f64,
    },
    /// Full ellipse: centre, end of the major axis, minor radius.
    Ellipse {
        c: Vec2,
        major: Vec2,
        minor: f64,
    },
    /// Spline through (`control == false`) or over (`control`) the points.
    Spline {
        pts: Vec<Vec2>,
        control: bool,
        degree: u8,
    },
}

impl LinkGeom {
    fn finite(&self) -> bool {
        let ok = |p: &Vec2| p.is_finite() && p.x.abs() < 1e9 && p.y.abs() < 1e9;
        match self {
            LinkGeom::Point(p) => ok(p),
            LinkGeom::Line(a, b) => ok(a) && ok(b) && a.dist(*b) > MERGE_TOL,
            LinkGeom::Circle(c, r) => ok(c) && r.is_finite() && *r > 1e-9 && *r < 1e9,
            LinkGeom::Arc { c, a, b } => ok(c) && ok(a) && ok(b) && c.dist(*a) > 1e-9 && a.dist(*b) > MERGE_TOL,
            LinkGeom::Conic { a, apex, b, rho } => ok(a) && ok(apex) && ok(b) && a.dist(*b) > MERGE_TOL && *rho > 1e-6 && *rho < 1.0 - 1e-6,
            LinkGeom::Ellipse { c, major, minor } => {
                ok(c) && ok(major) && c.dist(*major) > 1e-9 && minor.is_finite() && *minor > 1e-9 && *minor < 1e9
            }
            LinkGeom::Spline { pts, degree, .. } => {
                (2..=crate::curves::MAX_SPLINE_POINTS).contains(&pts.len()) && pts.iter().all(ok) && (1..=7).contains(degree)
            }
        }
    }
}

/// Link geometry as merged points plus curves over them (indices into the point list).
struct Layout {
    pts: Vec<Vec2>,
    /// Free-standing points (not owned by a curve).
    lone: Vec<usize>,
    curves: Vec<LCurve>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum LCurve {
    Line(usize, usize),
    Circle(usize, f64),
    Arc(usize, usize, usize),
    Conic(usize, usize, usize, f64),
    Ellipse(usize, usize, f64),
    /// Start, end, first interior point and interior count (interior points are consecutive
    /// and never shared), control flag, degree.
    Spline(usize, usize, usize, usize, bool, u8),
}

fn layout(geom: &[LinkGeom]) -> Result<Layout> {
    if geom.len() > MAX_LINK_ENTITIES {
        return Err(SketchError::TooLarge);
    }
    let mut l = Layout { pts: Vec::new(), lone: Vec::new(), curves: Vec::new() };
    fn pt(pts: &mut Vec<Vec2>, p: Vec2) -> usize {
        if let Some(i) = pts.iter().position(|q| q.dist(p) <= MERGE_TOL) {
            return i;
        }
        pts.push(p);
        pts.len() - 1
    }
    for g in geom.iter().filter(|g| g.finite()) {
        match g.clone() {
            LinkGeom::Point(p) => {
                let i = pt(&mut l.pts, p);
                if !l.lone.contains(&i) {
                    l.lone.push(i);
                }
            }
            LinkGeom::Line(a, b) => {
                let (a, b) = (pt(&mut l.pts, a), pt(&mut l.pts, b));
                if a != b {
                    l.curves.push(LCurve::Line(a, b));
                }
            }
            LinkGeom::Circle(c, r) => {
                let c = pt(&mut l.pts, c);
                l.curves.push(LCurve::Circle(c, r));
            }
            LinkGeom::Arc { c, a, b } => {
                let r = c.dist(a);
                let b = c + (b - c).normalized().unwrap_or(Vec2::X) * r;
                let (c, a, b) = (pt(&mut l.pts, c), pt(&mut l.pts, a), pt(&mut l.pts, b));
                if a != b && c != a {
                    l.curves.push(LCurve::Arc(c, a, b));
                }
            }
            LinkGeom::Conic { a, apex, b, rho } => {
                let (a, b, x) = (pt(&mut l.pts, a), pt(&mut l.pts, b), pt(&mut l.pts, apex));
                if a != b && x != a && x != b {
                    l.curves.push(LCurve::Conic(a, b, x, rho));
                }
            }
            LinkGeom::Ellipse { c, major, minor } => {
                let (c, m) = (pt(&mut l.pts, c), pt(&mut l.pts, major));
                if c != m {
                    l.curves.push(LCurve::Ellipse(c, m, minor));
                }
            }
            LinkGeom::Spline { pts, control, degree } => {
                let (Some(first), Some(last)) = (pts.first(), pts.last()) else { continue };
                let s0 = pt(&mut l.pts, *first);
                let first_in = l.pts.len();
                let inner = pts.get(1..pts.len() - 1).unwrap_or_default();
                l.pts.extend(inner.iter().copied());
                let s1 = pt(&mut l.pts, *last);
                l.curves.push(LCurve::Spline(s0, s1, first_in, inner.len(), control, degree));
            }
        }
    }
    // Lone points that a curve also uses are not lone.
    let used = |i: usize, cs: &[LCurve]| {
        cs.iter().any(|c| match *c {
            LCurve::Line(a, b) => a == i || b == i,
            LCurve::Circle(c, _) => c == i,
            LCurve::Arc(c, a, b) => c == i || a == i || b == i,
            LCurve::Conic(a, b, x, _) => a == i || b == i || x == i,
            LCurve::Ellipse(c, m, _) => c == i || m == i,
            LCurve::Spline(a, b, f, n, ..) => a == i || b == i || (i >= f && i < f + n),
        })
    };
    let curves = l.curves.clone();
    l.lone.retain(|i| !used(*i, &curves));
    Ok(l)
}

fn same_shape(a: &LCurve, b: &LCurve) -> bool {
    matches!(
        (a, b),
        (LCurve::Line(..), LCurve::Line(..))
            | (LCurve::Circle(..), LCurve::Circle(..))
            | (LCurve::Arc(..), LCurve::Arc(..))
            | (LCurve::Conic(..), LCurve::Conic(..))
            | (LCurve::Ellipse(..), LCurve::Ellipse(..))
            | (LCurve::Spline(..), LCurve::Spline(..))
    )
}

/// Do two curve sets describe the same geometry, regardless of the order the curves come in or
/// the direction a loop was walked? Lines match with either end first; circles by centre and
/// radius; arcs by centre and their (start, end) pair; conics by all their points and rho.
fn same_geometry(a: &[(LCurve, Vec<Vec2>)], b: &[(LCurve, Vec<Vec2>)]) -> bool {
    const TOL: f64 = 1e-6;
    let near = |p: Vec2, q: Vec2| p.dist(q) <= TOL * (1.0 + p.len().max(q.len()));
    let close = |x: f64, y: f64| (x - y).abs() <= TOL * (1.0 + x.abs().max(y.abs()));
    let matches = |(ca, pa): &(LCurve, Vec<Vec2>), (cb, pb): &(LCurve, Vec<Vec2>)| -> bool {
        if pa.len() != pb.len() {
            return false;
        }
        let all = || pa.iter().zip(pb).all(|(p, q)| near(*p, *q));
        match (ca, cb) {
            (LCurve::Line(..), LCurve::Line(..)) => {
                all() || matches!((pa.as_slice(), pb.as_slice()), ([a0, a1], [b0, b1]) if near(*a0, *b1) && near(*a1, *b0))
            }
            (LCurve::Circle(_, ra), LCurve::Circle(_, rb)) => all() && close(*ra, *rb),
            (LCurve::Arc(..), LCurve::Arc(..)) => all(),
            (LCurve::Conic(.., ra), LCurve::Conic(.., rb)) => all() && close(*ra, *rb),
            (LCurve::Ellipse(.., ra), LCurve::Ellipse(.., rb)) => all() && close(*ra, *rb),
            (LCurve::Spline(.., ca, da), LCurve::Spline(.., cb, db)) => ca == cb && da == db && all(),
            _ => false,
        }
    };
    if a.len() != b.len() {
        return false;
    }
    let mut used = vec![false; b.len()];
    a.iter().all(|x| {
        let hit = b.iter().enumerate().find(|(j, y)| !used.get(*j).copied().unwrap_or(true) && matches(x, y)).map(|(j, _)| j);
        match hit.and_then(|j| used.get_mut(j)) {
            Some(u) => {
                *u = true;
                true
            }
            None => false,
        }
    })
}

/// The point indices a layout curve uses, in order.
fn curve_points(c: &LCurve) -> Vec<usize> {
    match *c {
        LCurve::Line(a, b) => vec![a, b],
        LCurve::Circle(c, _) => vec![c],
        LCurve::Arc(c, a, b) => vec![c, a, b],
        LCurve::Conic(a, b, x, _) => vec![a, b, x],
        LCurve::Ellipse(c, m, _) => vec![c, m],
        LCurve::Spline(a, b, f, n, ..) => {
            let mut v = vec![a];
            v.extend(f..f + n);
            v.push(b);
            v
        }
    }
}

impl Sketch {
    pub fn link(&self, id: &str) -> Option<&Link> {
        self.links.iter().find(|l| l.id == id)
    }

    /// Curves owned by a link, in creation order.
    pub fn link_curves(&self, id: &str) -> Vec<usize> {
        self.curves.iter().enumerate().filter(|(_, c)| c.link.as_deref() == Some(id)).map(|(i, _)| i).collect()
    }

    /// Points owned by a link, in creation order.
    pub fn link_points(&self, id: &str) -> Vec<usize> {
        self.points.iter().enumerate().filter(|(_, p)| p.link.as_deref() == Some(id)).map(|(i, _)| i).collect()
    }

    /// Is this curve linked reference geometry?
    pub fn is_linked_curve(&self, ci: usize) -> bool {
        self.curves.get(ci).is_some_and(|c| c.link.is_some())
    }

    /// Is this curve text outline (linked, but drawn like normal geometry)?
    pub fn is_text_curve(&self, ci: usize) -> bool {
        self.curves.get(ci).and_then(|c| c.link.as_deref()).and_then(|l| self.link(l)).is_some_and(|l| l.kind == LinkKind::Text)
    }

    /// Is the curve's link lost (its reference no longer found)?
    pub fn is_lost_curve(&self, ci: usize) -> bool {
        self.curves.get(ci).and_then(|c| c.link.as_deref()).and_then(|l| self.link(l)).is_some_and(|l| l.lost)
    }

    /// Add linked geometry. Returns the link id; the new curve and point ids are
    /// [`Sketch::link_curves`] / [`Sketch::link_points`] of it.
    pub fn add_link(&mut self, kind: LinkKind, source: LinkSource, geom: &[LinkGeom]) -> Result<String> {
        let lay = layout(geom)?;
        if lay.curves.is_empty() && lay.lone.is_empty() {
            return Err(SketchError::Invalid("the reference projects to nothing on this sketch plane".into()));
        }
        if self.points.len() + lay.pts.len() > crate::model::MAX_POINTS {
            return Err(SketchError::TooLarge);
        }
        let id = self.fresh("j");
        self.links.push(Link { id: id.clone(), kind, source, face_name: None, lost: false });
        self.build_link(&id, &lay)?;
        Ok(id)
    }

    fn build_link(&mut self, id: &str, lay: &Layout) -> Result<()> {
        let mut idx: Vec<Option<usize>> = vec![None; lay.pts.len()];
        let mut point = |sk: &mut Sketch, li: usize| -> Result<usize> {
            if let Some(Some(i)) = idx.get(li) {
                return Ok(*i);
            }
            let p = lay.pts.get(li).copied().ok_or_else(|| SketchError::Invalid("link point".into()))?;
            let i = sk.add_point(p, None)?;
            if let Some(sp) = sk.points.get_mut(i) {
                sp.fixed = true;
                sp.link = Some(id.to_string());
            }
            if let Some(slot) = idx.get_mut(li) {
                *slot = Some(i);
            }
            Ok(i)
        };
        for c in &lay.curves {
            let (kind, prefix) = match *c {
                LCurve::Line(a, b) => (CurveKind::Line { a: point(self, a)?, b: point(self, b)? }, "l"),
                LCurve::Circle(c, r) => (CurveKind::Circle { c: point(self, c)?, r }, "c"),
                LCurve::Arc(c, a, b) => (CurveKind::Arc { c: point(self, c)?, a: point(self, a)?, b: point(self, b)? }, "a"),
                LCurve::Conic(a, b, x, rho) => (CurveKind::Conic { a: point(self, a)?, b: point(self, b)?, apex: point(self, x)?, rho }, "k"),
                LCurve::Ellipse(c, m, r) => (CurveKind::Ellipse { c: point(self, c)?, m: point(self, m)?, r }, "e"),
                LCurve::Spline(a, b, f, n, control, degree) => {
                    let mut pts = vec![point(self, a)?];
                    for i in f..f + n {
                        pts.push(point(self, i)?);
                    }
                    pts.push(point(self, b)?);
                    (CurveKind::Spline { pts, control, degree }, "s")
                }
            };
            let cid = self.fresh(prefix);
            self.curves.push(Curve {
                id: cid,
                kind,
                construction: false,
                reversed: false,
                link: Some(id.to_string()),
                centerline: false,
                fixed: false,
            });
        }
        for li in &lay.lone {
            point(self, *li)?;
        }
        Ok(())
    }

    /// Replace a link's geometry with a newly resolved one. When the shape is the same (same
    /// curve kinds sharing the same points) the entities keep their ids and only move, so
    /// constraints and dimensions on them survive; otherwise the link is rebuilt. Returns
    /// `true` when it was rebuilt.
    pub fn update_link(&mut self, id: &str, geom: &[LinkGeom]) -> Result<bool> {
        if self.link(id).is_none() {
            return Err(SketchError::Unknown(id.into()));
        }
        let lay = layout(geom)?;
        let curves = self.link_curves(id);
        let points = self.link_points(id);
        // Current layout of the link, in the same terms.
        let local = |p: usize| points.iter().position(|q| *q == p);
        let cur: Option<Vec<LCurve>> = curves
            .iter()
            .map(|ci| match self.curves.get(*ci).map(|c| &c.kind) {
                Some(CurveKind::Line { a, b }) => Some(LCurve::Line(local(*a)?, local(*b)?)),
                Some(CurveKind::Circle { c, r }) => Some(LCurve::Circle(local(*c)?, *r)),
                Some(CurveKind::Arc { c, a, b }) => Some(LCurve::Arc(local(*c)?, local(*a)?, local(*b)?)),
                Some(CurveKind::Conic { a, b, apex, rho }) => Some(LCurve::Conic(local(*a)?, local(*b)?, local(*apex)?, *rho)),
                Some(CurveKind::Ellipse { c, m, r }) => Some(LCurve::Ellipse(local(*c)?, local(*m)?, *r)),
                Some(CurveKind::Spline { pts, control, degree }) => {
                    let (a, b) = (local(*pts.first()?)?, local(*pts.last()?)?);
                    let inner: Vec<usize> =
                        pts.get(1..pts.len().saturating_sub(1)).unwrap_or_default().iter().map(|q| local(*q)).collect::<Option<_>>()?;
                    let f = inner.first().copied().unwrap_or(0);
                    // Interior points were made one after another.
                    if inner.iter().enumerate().any(|(k, x)| *x != f + k) {
                        return None;
                    }
                    Some(LCurve::Spline(a, b, f, inner.len(), *control, *degree))
                }
                _ => None,
            })
            .collect();
        // The new layout numbers points in first-use order: curves first, then lone points,
        // exactly as `build_link` creates them.
        let mut order: Vec<usize> = Vec::new();
        let mut visit = |i: usize| {
            if !order.contains(&i) {
                order.push(i);
            }
        };
        for c in &lay.curves {
            match *c {
                LCurve::Line(a, b) => {
                    visit(a);
                    visit(b);
                }
                LCurve::Circle(c, _) => visit(c),
                LCurve::Arc(c, a, b) => {
                    visit(c);
                    visit(a);
                    visit(b);
                }
                LCurve::Conic(a, b, x, _) => {
                    visit(a);
                    visit(b);
                    visit(x);
                }
                LCurve::Ellipse(c, m, _) => {
                    visit(c);
                    visit(m);
                }
                LCurve::Spline(a, b, f, n, ..) => {
                    visit(a);
                    for i in f..f + n {
                        visit(i);
                    }
                    visit(b);
                }
            }
        }
        for i in &lay.lone {
            visit(*i);
        }
        let renum = |i: usize| order.iter().position(|q| *q == i).unwrap_or(usize::MAX);
        let new: Vec<LCurve> = lay
            .curves
            .iter()
            .map(|c| match *c {
                LCurve::Line(a, b) => LCurve::Line(renum(a), renum(b)),
                LCurve::Circle(c, r) => LCurve::Circle(renum(c), r),
                LCurve::Arc(c, a, b) => LCurve::Arc(renum(c), renum(a), renum(b)),
                LCurve::Conic(a, b, x, r) => LCurve::Conic(renum(a), renum(b), renum(x), r),
                LCurve::Ellipse(c, m, r) => LCurve::Ellipse(renum(c), renum(m), r),
                LCurve::Spline(a, b, f, n, ct, d) => LCurve::Spline(renum(a), renum(b), renum(f), n, ct, d),
            })
            .collect();
        let same = match &cur {
            Some(cur) => {
                cur.len() == new.len()
                    && order.len() == points.len()
                    && cur.iter().zip(&new).all(|(a, b)| {
                        same_shape(a, b)
                            && match (a, b) {
                                (LCurve::Line(a0, a1), LCurve::Line(b0, b1)) => a0 == b0 && a1 == b1,
                                (LCurve::Circle(a0, _), LCurve::Circle(b0, _)) => a0 == b0,
                                (LCurve::Arc(a0, a1, a2), LCurve::Arc(b0, b1, b2)) => a0 == b0 && a1 == b1 && a2 == b2,
                                (LCurve::Conic(a0, a1, a2, _), LCurve::Conic(b0, b1, b2, _)) => a0 == b0 && a1 == b1 && a2 == b2,
                                (LCurve::Ellipse(a0, a1, _), LCurve::Ellipse(b0, b1, _)) => a0 == b0 && a1 == b1,
                                (LCurve::Spline(a0, a1, a2, a3, a4, a5), LCurve::Spline(b0, b1, b2, b3, b4, b5)) => {
                                    a0 == b0 && a1 == b1 && (a2 == b2 || *a3 == 0) && a3 == b3 && a4 == b4 && a5 == b5
                                }
                                _ => false,
                            }
                    })
            }
            None => false,
        };
        if let Some(l) = self.links.iter_mut().find(|l| l.id == id) {
            l.lost = false;
        }
        if same {
            for (k, pi) in points.iter().enumerate() {
                let p = order.get(k).and_then(|li| lay.pts.get(*li)).copied();
                if let (Some(p), Some(sp)) = (p, self.points.get_mut(*pi)) {
                    sp.pos = p;
                }
            }
            for (ci, c) in curves.iter().zip(&new) {
                if let (LCurve::Circle(_, r), Some(Curve { kind: CurveKind::Circle { r: slot, .. }, .. })) = (c, self.curves.get_mut(*ci)) {
                    *slot = *r;
                }
                if let (LCurve::Conic(.., r), Some(Curve { kind: CurveKind::Conic { rho, .. }, .. })) = (c, self.curves.get_mut(*ci)) {
                    *rho = *r;
                }
                if let (LCurve::Ellipse(.., r), Some(Curve { kind: CurveKind::Ellipse { r: slot, .. }, .. })) = (c, self.curves.get_mut(*ci)) {
                    *slot = *r;
                }
            }
            return Ok(false);
        }
        // The same geometry walked in another order or direction (e.g. a face loop re-read after
        // a rollback) is not a change: keep the entities, and the constraints on them.
        if let Some(cur) = &cur {
            let pos = |pi: usize| self.points.get(pi).map(|p| p.pos);
            let old: Option<Vec<(LCurve, Vec<Vec2>)>> = cur
                .iter()
                .map(|c| curve_points(c).into_iter().map(|li| points.get(li).and_then(|pi| pos(*pi))).collect::<Option<Vec<_>>>().map(|ps| (*c, ps)))
                .collect();
            let fresh: Option<Vec<(LCurve, Vec<Vec2>)>> = lay
                .curves
                .iter()
                .map(|c| curve_points(c).into_iter().map(|li| lay.pts.get(li).copied()).collect::<Option<Vec<_>>>().map(|ps| (*c, ps)))
                .collect();
            let lone_old: Vec<Vec2> = points.iter().filter_map(|pi| pos(*pi)).collect();
            if let (Some(old), Some(fresh)) = (old, fresh)
                && lay.lone.is_empty()
                && points.len() == lay.pts.len()
                && lone_old.len() == points.len()
                && same_geometry(&old, &fresh)
            {
                return Ok(false);
            }
        }
        // A resized face can enumerate the same boundary in another order. Match the
        // shared-point graph in normalized sketch space before discarding its identity.
        if let Some(cur) = &cur
            && points.len() == lay.pts.len()
            && cur.len() == lay.curves.len()
            && points.len() <= 512
        {
            let old: Vec<Vec2> = points.iter().filter_map(|i| self.point(*i)).collect();
            let normalized = |ps: &[Vec2]| -> Vec<Vec2> {
                let lo = ps.iter().fold(Vec2::new(f64::INFINITY, f64::INFINITY), |a, p| Vec2::new(a.x.min(p.x), a.y.min(p.y)));
                let hi = ps.iter().fold(Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY), |a, p| Vec2::new(a.x.max(p.x), a.y.max(p.y)));
                ps.iter().map(|p| Vec2::new((p.x - lo.x) / (hi.x - lo.x).max(1e-9), (p.y - lo.y) / (hi.y - lo.y).max(1e-9))).collect()
            };
            let (a, b) = (normalized(&old), normalized(&lay.pts));
            let mut mapping = Vec::new();
            for p in &a {
                let mut candidates: Vec<(usize, f64)> = b.iter().enumerate().map(|(i, q)| (i, p.dist(*q))).collect();
                candidates.sort_by(|x, y| x.1.total_cmp(&y.1));
                if let Some((i, d)) = candidates.first()
                    && *d < 0.1
                    && candidates.get(1).is_none_or(|(_, second)| *d < *second * 0.5)
                    && !mapping.contains(i)
                {
                    mapping.push(*i);
                } else {
                    break;
                }
            }
            let mut used = Vec::new();
            let mut curve_map = Vec::new();
            if mapping.len() == points.len() {
                for c in cur {
                    let mapped: Option<Vec<usize>> = curve_points(c).iter().map(|i| mapping.get(*i).copied()).collect();
                    let found = lay
                        .curves
                        .iter()
                        .enumerate()
                        .find(|(i, fresh)| {
                            if used.contains(i) || !same_shape(c, fresh) {
                                return false;
                            }
                            let fresh_pts = curve_points(fresh);
                            mapped.as_ref().is_some_and(|p| {
                                *p == fresh_pts || matches!(c, LCurve::Line(..)) && p.iter().rev().copied().collect::<Vec<_>>() == fresh_pts
                            })
                        })
                        .map(|(i, _)| i);
                    if let Some(i) = found {
                        used.push(i);
                        curve_map.push(i);
                    } else {
                        break;
                    }
                }
            }
            if curve_map.len() == cur.len() && mapping.len() == points.len() {
                for (pi, li) in points.iter().zip(mapping) {
                    if let (Some(p), Some(q)) = (self.points.get_mut(*pi), lay.pts.get(li)) {
                        p.pos = *q;
                    }
                }
                for (ci, li) in curves.iter().zip(curve_map) {
                    if let (Some(Curve { kind: CurveKind::Circle { r, .. }, .. }), Some(LCurve::Circle(_, value))) =
                        (self.curves.get_mut(*ci), lay.curves.get(li))
                    {
                        *r = *value;
                    }
                }
                return Ok(false);
            }
        }
        // Rebuild: drop the old entities (constraints on them go too) and make new ones.
        self.drop_link_entities(id);
        self.build_link(id, &lay)?;
        Ok(true)
    }

    fn drop_link_entities(&mut self, id: &str) {
        let curves = self.link_curves(id);
        let keep = self.links.clone();
        self.remove_curves(&curves);
        let pts = self.link_points(id);
        self.remove_points(&pts);
        // Removal prunes empty links; this one stays.
        self.links = keep;
    }

    /// Mark a link as lost (its reference was not found); its geometry stays as it was.
    pub fn set_link_lost(&mut self, id: &str, lost: bool) {
        if let Some(l) = self.links.iter_mut().find(|l| l.id == id) {
            l.lost = lost;
        }
    }

    /// Store an updated reference (e.g. the point on an edge after an upstream edit).
    pub fn set_link_source(&mut self, id: &str, source: LinkSource) {
        if let Some(l) = self.links.iter_mut().find(|l| l.id == id) {
            l.source = source;
        }
    }

    /// Turn linked geometry into normal sketch geometry: the link goes, the curves and points
    /// stay where they are and become free.
    pub fn break_link(&mut self, id: &str) -> Result<()> {
        if self.link(id).is_none() {
            return Err(SketchError::Unknown(id.into()));
        }
        for c in &mut self.curves {
            if c.link.as_deref() == Some(id) {
                c.link = None;
            }
        }
        for p in &mut self.points {
            if p.link.as_deref() == Some(id) {
                p.link = None;
                p.fixed = false;
            }
        }
        for w in &mut self.wires {
            if w.link.as_deref() == Some(id) {
                w.link = None;
            }
        }
        self.links.retain(|l| l.id != id);
        Ok(())
    }

    /// Remove links that no longer own any curve or point.
    pub(crate) fn prune_links(&mut self) {
        if self.links.is_empty() {
            return;
        }
        let used = |id: &str, sk: &Sketch| {
            sk.curves.iter().any(|c| c.link.as_deref() == Some(id))
                || sk.points.iter().any(|p| p.link.as_deref() == Some(id))
                || sk.wires.iter().any(|w| w.link.as_deref() == Some(id))
        };
        let keep: Vec<bool> = self.links.iter().map(|l| used(&l.id, self)).collect();
        let mut k = keep.iter();
        self.links.retain(|_| k.next().copied().unwrap_or(true));
    }
}
