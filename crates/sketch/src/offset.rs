//! Associative offsets keep their output entity ids while their source curves move.
use crate::{CurveKind, Shape, Sketch, SketchError, intersections};
use serde::{Deserialize, Serialize};
use solvecraft_geom::Vec2;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OffsetChain {
    pub sources: Vec<(String, bool)>,
    pub outputs: Vec<String>,
    pub distance: f64,
    pub closed: bool,
}

impl Sketch {
    pub fn refresh_offsets(&mut self) -> Result<(), SketchError> {
        for offset in self.offsets.clone() {
            if offset.sources.len() != offset.outputs.len() || offset.sources.len() > 10_000 || !offset.distance.is_finite() {
                return Err(SketchError::Invalid("damaged offset chain".into()));
            }
            let bad = || SketchError::Invalid("an offset source vanished or changed shape".into());
            let mut pieces = Vec::new();
            for (id, forward) in &offset.sources {
                let i = self.curve_index(id).ok_or_else(bad)?;
                let kind = &self.curves.get(i).ok_or_else(bad)?.kind;
                let piece = match *kind {
                    CurveKind::Line { a, b } => {
                        let (a, b) = (self.point(a).ok_or_else(bad)?, self.point(b).ok_or_else(bad)?);
                        let (a, b) = if *forward { (a, b) } else { (b, a) };
                        let n = (b - a).normalized().ok_or_else(bad)?.perp() * offset.distance;
                        (Shape::Line { a: a + n, b: b + n }, a + n, b + n)
                    }
                    CurveKind::Arc { c, a, b } => {
                        let (c, a, b) = (self.point(c).ok_or_else(bad)?, self.point(a).ok_or_else(bad)?, self.point(b).ok_or_else(bad)?);
                        let r = c.dist(a) - if *forward { offset.distance } else { -offset.distance };
                        if r <= 1e-9 {
                            return Err(SketchError::Invalid("the offset collapses an arc".into()));
                        }
                        let a = c + (a - c).normalized().ok_or_else(bad)? * r;
                        let b = c + (b - c).normalized().ok_or_else(bad)? * r;
                        let (a, b) = if *forward { (a, b) } else { (b, a) };
                        (Shape::Round { c, r, start: 0.0, sweep: std::f64::consts::TAU }, a, b)
                    }
                    CurveKind::Circle { c, r } => {
                        let c = self.point(c).ok_or_else(bad)?;
                        let r = r - offset.distance;
                        if r <= 1e-9 {
                            return Err(SketchError::Invalid("the offset collapses a circle".into()));
                        }
                        let out = offset.outputs.first().and_then(|id| self.curve_index(id)).ok_or_else(bad)?;
                        if let Some(crate::Curve { kind: CurveKind::Circle { r: target, .. }, .. }) = self.curves.get_mut(out) {
                            *target = r;
                        } else {
                            return Err(bad());
                        }
                        let _ = c;
                        continue;
                    }
                    _ => return Err(bad()),
                };
                pieces.push(piece);
            }
            if pieces.is_empty() {
                continue;
            }
            let joint = |i: usize, j: usize| -> Result<Vec2, SketchError> {
                let (x, y) = (pieces.get(i).ok_or_else(bad)?, pieces.get(j).ok_or_else(bad)?);
                if x.2.dist(y.1) < 1e-6 {
                    return Ok(x.2);
                }
                let guess = (x.2 + y.1) * 0.5;
                intersections(&x.0, &y.0)
                    .into_iter()
                    .min_by(|a, b| a.dist(guess).total_cmp(&b.dist(guess)))
                    .ok_or_else(|| SketchError::Invalid("the offset curves no longer meet".into()))
            };
            let n = pieces.len();
            let mut joints = vec![if offset.closed { joint(n - 1, 0)? } else { pieces.first().ok_or_else(bad)?.1 }];
            for i in 0..n.saturating_sub(1) {
                joints.push(joint(i, i + 1)?);
            }
            if !offset.closed {
                joints.push(pieces.last().ok_or_else(bad)?.2);
            }
            for (i, id) in offset.outputs.iter().enumerate() {
                let a = *joints.get(i).ok_or_else(bad)?;
                let b = *joints.get(if offset.closed { (i + 1) % n } else { i + 1 }).ok_or_else(bad)?;
                let shape = &pieces.get(i).ok_or_else(bad)?.0;
                if let Shape::Line { a: p, b: q } = shape
                    && (b - a).dot(*q - *p) <= 1e-9
                {
                    return Err(SketchError::Invalid("the offset curves would cross".into()));
                }
                let ci = self.curve_index(id).ok_or_else(bad)?;
                let kind = self.curves.get(ci).ok_or_else(bad)?.kind.clone();
                let (pa, pb) = match kind {
                    CurveKind::Line { a, b } => (a, b),
                    CurveKind::Arc { a, b, .. } => {
                        if offset.sources.get(i).is_some_and(|x| x.1) {
                            (a, b)
                        } else {
                            (b, a)
                        }
                    }
                    _ => return Err(bad()),
                };
                for (pi, pos) in [(pa, a), (pb, b)] {
                    if let Some(p) = self.points.get_mut(pi) {
                        p.pos = pos;
                        p.fixed = true;
                    }
                }
            }
        }
        Ok(())
    }
}
