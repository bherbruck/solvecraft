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
    /// The reference could not be found on the last recompute; the geometry is the last good one.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub lost: bool,
}

/// Geometry a link produces, in sketch coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
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

#[derive(Clone, Copy, PartialEq)]
enum LCurve {
    Line(usize, usize),
    Circle(usize, f64),
    Arc(usize, usize, usize),
    Conic(usize, usize, usize, f64),
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
        match *g {
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
        }
    }
    // Lone points that a curve also uses are not lone.
    let used = |i: usize, cs: &[LCurve]| {
        cs.iter().any(|c| match *c {
            LCurve::Line(a, b) => a == i || b == i,
            LCurve::Circle(c, _) => c == i,
            LCurve::Arc(c, a, b) => c == i || a == i || b == i,
            LCurve::Conic(a, b, x, _) => a == i || b == i || x == i,
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
    )
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
        self.links.push(Link { id: id.clone(), kind, source, lost: false });
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
            };
            let cid = self.fresh(prefix);
            self.curves.push(Curve { id: cid, kind, construction: false, reversed: false, link: Some(id.to_string()) });
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
            }
            return Ok(false);
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
        self.links.retain(|l| l.id != id);
        Ok(())
    }

    /// Remove links that no longer own any curve or point.
    pub(crate) fn prune_links(&mut self) {
        if self.links.is_empty() {
            return;
        }
        let used = |id: &str, sk: &Sketch| {
            sk.curves.iter().any(|c| c.link.as_deref() == Some(id)) || sk.points.iter().any(|p| p.link.as_deref() == Some(id))
        };
        let keep: Vec<bool> = self.links.iter().map(|l| used(&l.id, self)).collect();
        let mut k = keep.iter();
        self.links.retain(|_| k.next().copied().unwrap_or(true));
    }
}
