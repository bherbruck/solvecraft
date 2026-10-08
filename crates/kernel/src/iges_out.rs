//! IGES 5.3 export, written from the published specification: each body as a manifold solid
//! B-rep (186) with its shell, faces, loops, edge and vertex lists, on analytic surfaces
//! (planes, cylinders, cones, spheres, tori), surfaces of revolution and extrusion, and
//! rational B-splines, with body names (406 name properties) and colours (314). The bodies are
//! first written as STEP (so faces are merged and surfaces recognised exactly as in a STEP
//! export) and restated entity by entity.

use std::collections::HashMap;
use std::fmt::Write as _;

use crate::step_in::p21::{self, Exchange, Param};
use crate::{ExportBody, KernelError, Result, StepHeader};

type V3 = [f64; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}
fn unit(a: V3) -> Option<V3> {
    let n = norm(a);
    (n > 1e-300 && n.is_finite()).then(|| a.map(|x| x / n))
}

/// A real in IGES free format (always with a decimal point).
fn real(x: f64) -> String {
    let x = if x.is_finite() { x } else { 0.0 };
    let s = format!("{x:?}");
    let s = s.replace('e', "E");
    if s.contains('.') {
        s
    } else if let Some((m, e)) = s.split_once('E') {
        format!("{m}.E{e}")
    } else {
        format!("{s}.")
    }
}

/// A Hollerith string.
fn holl(s: &str) -> String {
    let clean: String = s.chars().map(|c| if c.is_ascii() && !c.is_ascii_control() { c } else { '_' }).collect();
    format!("{}H{clean}", clean.len())
}

struct Ent {
    typ: i64,
    form: i64,
    params: Vec<String>,
    xform: i64,
    color: i64,
    label: String,
    dependent: bool,
}

#[derive(Default)]
struct W {
    ents: Vec<Ent>,
    colors: HashMap<[u32; 3], i64>,
}

impl W {
    fn add(&mut self, typ: i64, form: i64, params: Vec<String>) -> i64 {
        self.ents.push(Ent { typ, form, params, xform: 0, color: 0, label: String::new(), dependent: true });
        2 * self.ents.len() as i64 - 1
    }
    fn get_mut(&mut self, de: i64) -> Option<&mut Ent> {
        usize::try_from((de - 1) / 2).ok().and_then(|i| self.ents.get_mut(i))
    }
    fn point(&mut self, p: V3) -> i64 {
        self.add(116, 0, vec![real(p[0]), real(p[1]), real(p[2])])
    }
    fn direction(&mut self, d: V3) -> i64 {
        self.add(123, 0, vec![real(d[0]), real(d[1]), real(d[2])])
    }
    /// Transformation (124) taking a frame's local coordinates to the model.
    fn frame(&mut self, o: V3, x: V3, z: V3) -> i64 {
        let y = cross(z, x);
        let rows: Vec<String> = (0..3).flat_map(|i| [real(x[i]), real(y[i]), real(z[i]), real(o[i])]).collect();
        self.add(124, 0, rows)
    }
    fn color(&mut self, c: [f32; 3]) -> i64 {
        let key = c.map(f32::to_bits);
        if let Some(d) = self.colors.get(&key) {
            return *d;
        }
        let d = self.add(314, 0, vec![real(c[0] as f64 * 100.0), real(c[1] as f64 * 100.0), real(c[2] as f64 * 100.0), holl("")]);
        if let Some(e) = self.get_mut(d) {
            e.dependent = false;
        }
        self.colors.insert(key, d);
        d
    }
}

/// Reading the STEP exchange.
struct S<'a> {
    ex: &'a Exchange,
}

impl<'a> S<'a> {
    fn e(&self, id: u64) -> Result<&'a p21::Entity> {
        self.ex.get(id).ok_or_else(|| KernelError::Failed(format!("IGES export: missing #{id}")))
    }
    fn r(&self, p: Option<&Param>) -> Result<u64> {
        p.and_then(Param::as_ref_id).ok_or_else(|| KernelError::Failed("IGES export: bad reference".into()))
    }
    fn f(&self, p: Option<&Param>) -> Result<f64> {
        p.and_then(Param::as_f64).ok_or_else(|| KernelError::Failed("IGES export: bad number".into()))
    }
    fn list(&self, p: Option<&'a Param>) -> Result<&'a [Param]> {
        p.and_then(Param::as_list).ok_or_else(|| KernelError::Failed("IGES export: bad list".into()))
    }
    fn point(&self, id: u64) -> Result<V3> {
        let e = self.e(id)?;
        let c = self.list(e.params().get(1))?;
        Ok([self.f(c.first())?, self.f(c.get(1))?, self.f(c.get(2))?])
    }
    fn dir(&self, id: u64) -> Result<V3> {
        let d = self.point(id)?;
        unit(d).ok_or_else(|| KernelError::Failed("IGES export: zero direction".into()))
    }
    fn vertex(&self, id: u64) -> Result<V3> {
        self.point(self.r(self.e(id)?.params().get(1))?)
    }
    /// AXIS2_PLACEMENT_3D: origin, z, x.
    fn placement(&self, id: u64) -> Result<(V3, V3, V3)> {
        let p = self.e(id)?.params();
        let o = self.point(self.r(p.get(1))?)?;
        let z = match p.get(2).and_then(Param::as_ref_id) {
            Some(d) => self.dir(d)?,
            None => [0.0, 0.0, 1.0],
        };
        let x0 = match p.get(3).and_then(Param::as_ref_id) {
            Some(d) => self.dir(d)?,
            None => [1.0, 0.0, 0.0],
        };
        let x = unit(sub(x0, z.map(|c| c * dot(x0, z)))).unwrap_or_else(|| {
            let a = if z[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
            unit(sub(a, z.map(|c| c * dot(a, z)))).unwrap_or([1.0, 0.0, 0.0])
        });
        Ok((o, z, x))
    }
}

/// B-spline data of a STEP curve or surface record set: degrees, control rows, weights, knots.
struct Spline {
    deg: Vec<usize>,
    pts: Vec<Vec<V3>>,
    weights: Option<Vec<Vec<f64>>>,
    knots: Vec<Vec<f64>>,
}

fn expand(mults: &[Param], knots: &[Param]) -> Option<Vec<f64>> {
    let mut out = Vec::new();
    for (m, k) in mults.iter().zip(knots) {
        let (m, k) = (m.as_i64()?, k.as_f64()?);
        if !(1..=64).contains(&m) {
            return None;
        }
        for _ in 0..m {
            out.push(k);
        }
    }
    Some(out)
}

fn spline(s: &S, e: &p21::Entity, surface: bool) -> Result<Spline> {
    let bad = || KernelError::Failed("IGES export: unreadable B-spline".into());
    let (base, kn): (Vec<Param>, Vec<Param>) = if e.is_complex() {
        let b = e.record(if surface { "B_SPLINE_SURFACE" } else { "B_SPLINE_CURVE" }).ok_or_else(bad)?;
        let k = e.record(if surface { "B_SPLINE_SURFACE_WITH_KNOTS" } else { "B_SPLINE_CURVE_WITH_KNOTS" }).ok_or_else(bad)?;
        let mut v = vec![Param::Str(String::new())];
        v.extend(b.iter().cloned());
        (v, k.to_vec())
    } else {
        let p = e.params();
        let skip = if surface { 8 } else { 6 };
        (p.to_vec(), p.get(skip..).map(<[Param]>::to_vec).unwrap_or_default())
    };
    let deg_n = if surface { 2 } else { 1 };
    let deg: Vec<usize> = (0..deg_n)
        .map(|i| base.get(1 + i).and_then(Param::as_i64).filter(|d| (1..=32).contains(d)).map(|d| d as usize).ok_or_else(bad))
        .collect::<Result<_>>()?;
    let ctrl = base.get(1 + deg_n).and_then(Param::as_list).ok_or_else(bad)?;
    let pts: Vec<Vec<V3>> = if surface {
        ctrl.iter()
            .map(|row| row.as_list().ok_or_else(bad)?.iter().map(|c| s.point(c.as_ref_id().ok_or_else(bad)?)).collect::<Result<Vec<_>>>())
            .collect::<Result<_>>()?
    } else {
        vec![ctrl.iter().map(|c| s.point(c.as_ref_id().ok_or_else(bad)?)).collect::<Result<Vec<_>>>()?]
    };
    let knots: Vec<Vec<f64>> = if surface {
        let (mu, mv, ku, kv) = (kn.first(), kn.get(1), kn.get(2), kn.get(3));
        vec![
            expand(mu.and_then(Param::as_list).ok_or_else(bad)?, ku.and_then(Param::as_list).ok_or_else(bad)?).ok_or_else(bad)?,
            expand(mv.and_then(Param::as_list).ok_or_else(bad)?, kv.and_then(Param::as_list).ok_or_else(bad)?).ok_or_else(bad)?,
        ]
    } else {
        vec![expand(kn.first().and_then(Param::as_list).ok_or_else(bad)?, kn.get(1).and_then(Param::as_list).ok_or_else(bad)?).ok_or_else(bad)?]
    };
    let weights = match e.record(if surface { "RATIONAL_B_SPLINE_SURFACE" } else { "RATIONAL_B_SPLINE_CURVE" }) {
        Some(r) => {
            let w = r.first().and_then(Param::as_list).ok_or_else(bad)?;
            Some(if surface {
                w.iter()
                    .map(|row| row.as_list().ok_or_else(bad)?.iter().map(|x| x.as_f64().ok_or_else(bad)).collect::<Result<Vec<_>>>())
                    .collect::<Result<_>>()?
            } else {
                vec![w.iter().map(|x| x.as_f64().ok_or_else(bad)).collect::<Result<Vec<_>>>()?]
            })
        }
        None => None,
    };
    Ok(Spline { deg, pts, weights, knots })
}

/// A curve as an IGES entity running from `a` to `b` (its direction in the STEP file), the DE
/// pointer.
fn curve(w: &mut W, s: &S, id: u64, a: V3, b: V3, closed: bool) -> Result<i64> {
    let e = s.e(id)?;
    match e.name() {
        "LINE" => Ok(w.add(110, 0, [a, b].iter().flatten().map(|x| real(*x)).collect())),
        "CIRCLE" | "ELLIPSE" => {
            let p = e.params();
            let (o, z, x) = s.placement(s.r(p.get(1))?)?;
            let y = cross(z, x);
            let local = |q: V3| {
                let v = sub(q, o);
                [dot(v, x), dot(v, y)]
            };
            let (la, lb) = (local(a), if closed { local(a) } else { local(b) });
            let tr = w.frame(o, x, z);
            let de = if e.name() == "CIRCLE" {
                w.add(100, 0, vec![real(0.0), real(0.0), real(0.0), real(la[0]), real(la[1]), real(lb[0]), real(lb[1])])
            } else {
                let (ra, rb) = (s.f(p.get(2))?, s.f(p.get(3))?);
                // A x² + C y² + F = 0 with F = -1: A = 1/ra², C = 1/rb².
                w.add(
                    104,
                    1,
                    vec![
                        real(1.0 / (ra * ra)),
                        real(0.0),
                        real(1.0 / (rb * rb)),
                        real(0.0),
                        real(0.0),
                        real(-1.0),
                        real(0.0),
                        real(la[0]),
                        real(la[1]),
                        real(lb[0]),
                        real(lb[1]),
                    ],
                )
            };
            if let Some(ent) = w.get_mut(de) {
                ent.xform = tr;
            }
            Ok(de)
        }
        _ if e.has("B_SPLINE_CURVE") || e.name() == "B_SPLINE_CURVE_WITH_KNOTS" => {
            let sp = spline(s, e, false)?;
            let (Some(pts), Some(knots), Some(&deg)) = (sp.pts.first(), sp.knots.first(), sp.deg.first()) else {
                return Err(KernelError::Failed("IGES export: empty B-spline".into()));
            };
            let k = pts.len().saturating_sub(1);
            let ws = sp.weights.as_ref().and_then(|w| w.first().cloned()).unwrap_or_else(|| vec![1.0; pts.len()]);
            let rational = ws.iter().any(|w| (w - 1.0).abs() > 1e-12);
            let (t0, t1) = (knots.get(deg).copied().unwrap_or(0.0), knots.get(pts.len()).copied().unwrap_or(1.0));
            let mut p = vec![k.to_string(), deg.to_string(), "0".into(), "0".into(), if rational { "0" } else { "1" }.into(), "0".into()];
            p.extend(knots.iter().map(|x| real(*x)));
            p.extend(ws.iter().map(|x| real(*x)));
            p.extend(pts.iter().flatten().map(|x| real(*x)));
            p.extend([real(t0), real(t1), real(0.0), real(0.0), real(0.0)]);
            Ok(w.add(126, 0, p))
        }
        n => Err(KernelError::Failed(format!("IGES export: curve {n} is not written yet"))),
    }
}

/// Start point of a STEP curve (for extrusion directrices).
fn curve_start(s: &S, id: u64) -> Result<V3> {
    let e = s.e(id)?;
    match e.name() {
        "LINE" => s.point(s.r(e.params().get(1))?),
        "CIRCLE" => {
            let (o, _, x) = s.placement(s.r(e.params().get(1))?)?;
            let r = s.f(e.params().get(2))?;
            Ok([o[0] + r * x[0], o[1] + r * x[1], o[2] + r * x[2]])
        }
        _ => {
            let sp = spline(s, e, false)?;
            sp.pts.first().and_then(|r| r.first()).copied().ok_or_else(|| KernelError::Failed("IGES export: empty curve".into()))
        }
    }
}

/// A curve written whole (a revolution's generatrix, an extrusion's directrix).
fn whole_curve(w: &mut W, s: &S, id: u64) -> Result<i64> {
    let e = s.e(id)?;
    match e.name() {
        "LINE" => {
            let p = e.params();
            let o = s.point(s.r(p.get(1))?)?;
            let v = s.e(s.r(p.get(2))?)?;
            let d = s.dir(s.r(v.params().get(1))?)?;
            let m = s.f(v.params().get(2))?;
            curve(w, s, id, o, [o[0] + d[0] * m, o[1] + d[1] * m, o[2] + d[2] * m], false)
        }
        "CIRCLE" | "ELLIPSE" => {
            let a = curve_start(s, id)?;
            curve(w, s, id, a, a, true)
        }
        _ => curve(w, s, id, [0.0; 3], [0.0; 3], false),
    }
}

fn surface(w: &mut W, s: &S, id: u64) -> Result<i64> {
    let e = s.e(id)?;
    let p = e.params();
    let analytic = |w: &mut W, typ: i64, place: u64, extra: &[f64]| -> Result<i64> {
        let (o, z, x) = s.placement(place)?;
        let (po, pz, px) = (w.point(o), w.direction(z), w.direction(x));
        let mut v = vec![po.to_string()];
        match typ {
            190 => v.extend([pz.to_string(), px.to_string()]),
            192 => v.extend([pz.to_string(), real(extra.first().copied().unwrap_or(0.0)), px.to_string()]),
            194 => v.extend([
                pz.to_string(),
                real(extra.first().copied().unwrap_or(0.0)),
                real(extra.get(1).copied().unwrap_or(0.0).to_degrees()),
                px.to_string(),
            ]),
            196 => v.extend([real(extra.first().copied().unwrap_or(0.0)), pz.to_string(), px.to_string()]),
            _ => v.extend([pz.to_string(), real(extra.first().copied().unwrap_or(0.0)), real(extra.get(1).copied().unwrap_or(0.0)), px.to_string()]),
        }
        Ok(w.add(typ, 1, v))
    };
    match e.name() {
        "PLANE" => analytic(w, 190, s.r(p.get(1))?, &[]),
        "CYLINDRICAL_SURFACE" => analytic(w, 192, s.r(p.get(1))?, &[s.f(p.get(2))?]),
        "CONICAL_SURFACE" => analytic(w, 194, s.r(p.get(1))?, &[s.f(p.get(2))?, s.f(p.get(3))?]),
        "SPHERICAL_SURFACE" => analytic(w, 196, s.r(p.get(1))?, &[s.f(p.get(2))?]),
        "TOROIDAL_SURFACE" => analytic(w, 198, s.r(p.get(1))?, &[s.f(p.get(2))?, s.f(p.get(3))?]),
        "SURFACE_OF_REVOLUTION" => {
            let gen_curve = whole_curve(w, s, s.r(p.get(1))?)?;
            let ax = s.e(s.r(p.get(2))?)?;
            let o = s.point(s.r(ax.params().get(1))?)?;
            let d = s.dir(s.r(ax.params().get(2))?)?;
            let axis = w.add(110, 0, [o, [o[0] + d[0], o[1] + d[1], o[2] + d[2]]].iter().flatten().map(|x| real(*x)).collect());
            Ok(w.add(120, 0, vec![axis.to_string(), gen_curve.to_string(), real(0.0), real(std::f64::consts::TAU)]))
        }
        "SURFACE_OF_LINEAR_EXTRUSION" => {
            let c = s.r(p.get(1))?;
            let dir = whole_curve(w, s, c)?;
            let start = curve_start(s, c)?;
            let v = s.e(s.r(p.get(2))?)?;
            let d = s.dir(s.r(v.params().get(1))?)?;
            let m = s.f(v.params().get(2))?;
            Ok(w.add(122, 0, vec![dir.to_string(), real(start[0] + d[0] * m), real(start[1] + d[1] * m), real(start[2] + d[2] * m)]))
        }
        _ if e.has("B_SPLINE_SURFACE") || e.name() == "B_SPLINE_SURFACE_WITH_KNOTS" => {
            let sp = spline(s, e, true)?;
            let (Some(&du), Some(&dv), Some(ku), Some(kv)) = (sp.deg.first(), sp.deg.get(1), sp.knots.first(), sp.knots.get(1)) else {
                return Err(KernelError::Failed("IGES export: bad B-spline surface".into()));
            };
            let nu = sp.pts.len();
            let nv = sp.pts.first().map(Vec::len).unwrap_or(0);
            if nu == 0 || nv == 0 || sp.pts.iter().any(|r| r.len() != nv) {
                return Err(KernelError::Failed("IGES export: ragged B-spline surface".into()));
            }
            let wt = |i: usize, j: usize| sp.weights.as_ref().and_then(|w| w.get(i)?.get(j).copied()).unwrap_or(1.0);
            let rational = (0..nu).any(|i| (0..nv).any(|j| (wt(i, j) - 1.0).abs() > 1e-12));
            let mut v = vec![
                (nu - 1).to_string(),
                (nv - 1).to_string(),
                du.to_string(),
                dv.to_string(),
                "0".into(),
                "0".into(),
                if rational { "0" } else { "1" }.into(),
                "0".into(),
                "0".into(),
            ];
            v.extend(ku.iter().map(|x| real(*x)));
            v.extend(kv.iter().map(|x| real(*x)));
            // The first index runs fastest.
            for j in 0..nv {
                for i in 0..nu {
                    v.push(real(wt(i, j)));
                }
            }
            for j in 0..nv {
                for i in 0..nu {
                    let q = sp.pts.get(i).and_then(|r| r.get(j)).copied().unwrap_or([0.0; 3]);
                    v.extend(q.map(real));
                }
            }
            let range = |k: &Vec<f64>, d: usize, n: usize| (k.get(d).copied().unwrap_or(0.0), k.get(n).copied().unwrap_or(1.0));
            let ((u0, u1), (v0, v1)) = (range(ku, du, nu), range(kv, dv, nv));
            v.extend([real(u0), real(u1), real(v0), real(v1)]);
            Ok(w.add(128, 0, v))
        }
        n => Err(KernelError::Failed(format!("IGES export: surface {n} is not written yet"))),
    }
}

/// Colours of styled items (solids and faces).
fn colours(ex: &Exchange) -> HashMap<u64, [f32; 3]> {
    let mut out = HashMap::new();
    for e in ex.entities.values() {
        if e.name() != "STYLED_ITEM" {
            continue;
        }
        let Some(item) = e.params().get(2).and_then(Param::as_ref_id) else { continue };
        let mut queue: Vec<u64> =
            e.params().get(1).and_then(Param::as_list).map(|l| l.iter().filter_map(Param::as_ref_id).collect()).unwrap_or_default();
        let mut seen = 0;
        while let Some(id) = queue.pop() {
            seen += 1;
            if seen > 64 {
                break;
            }
            let Some(s) = ex.get(id) else { continue };
            if s.name() == "COLOUR_RGB" {
                let c = |i: usize| s.params().get(i).and_then(Param::as_f64).map(|x| x.clamp(0.0, 1.0) as f32);
                if let (Some(r), Some(g), Some(b)) = (c(1), c(2), c(3)) {
                    out.insert(item, [r, g, b]);
                }
                break;
            }
            for r in &s.records {
                for p in &r.params {
                    match p {
                        Param::Ref(x) => queue.push(*x),
                        Param::List(v) => queue.extend(v.iter().filter_map(Param::as_ref_id)),
                        _ => {}
                    }
                }
            }
        }
    }
    out
}

/// A shell (514) with its vertex list, edge list, loops and faces.
fn shell(w: &mut W, s: &S, id: u64, colors: &HashMap<u64, [f32; 3]>) -> Result<i64> {
    let sh = s.e(id)?;
    let faces = s.list(sh.params().get(1))?;
    // Vertices and edges of the shell, in first-use order.
    let mut vidx: HashMap<u64, usize> = HashMap::new();
    let mut verts: Vec<V3> = Vec::new();
    let mut eidx: HashMap<u64, usize> = HashMap::new();
    let mut edges: Vec<(u64, bool, usize, usize)> = Vec::new(); // curve, same sense, start, end
    let mut face_data = Vec::new();
    for f in faces {
        let fid = s.r(Some(f))?;
        let fe = s.e(fid)?;
        let sense = fe.params().get(3).and_then(Param::as_bool).unwrap_or(true);
        let mut loops: Vec<Vec<(u64, bool)>> = Vec::new();
        let bounds = s.list(fe.params().get(1))?;
        // The outer bound first.
        let mut ordered: Vec<&Param> =
            bounds.iter().filter(|b| b.as_ref_id().and_then(|x| s.ex.get(x)).is_some_and(|e| e.name() == "FACE_OUTER_BOUND")).collect();
        ordered.extend(bounds.iter().filter(|b| b.as_ref_id().and_then(|x| s.ex.get(x)).is_some_and(|e| e.name() != "FACE_OUTER_BOUND")));
        for b in ordered {
            let be = s.e(s.r(Some(b))?)?;
            let orient = be.params().get(2).and_then(Param::as_bool).unwrap_or(true);
            let lp = s.e(s.r(be.params().get(1))?)?;
            if lp.name() != "EDGE_LOOP" {
                continue;
            }
            let mut l = Vec::new();
            for oe in s.list(lp.params().get(1))? {
                let oe = s.e(s.r(Some(oe))?)?;
                let ec = s.r(oe.params().get(3))?;
                let d = oe.params().get(4).and_then(Param::as_bool).unwrap_or(true);
                #[allow(clippy::map_entry)] // the edge's vertices are numbered before it
                if !eidx.contains_key(&ec) {
                    let ee = s.e(ec)?;
                    let (v0, v1) = (s.r(ee.params().get(1))?, s.r(ee.params().get(2))?);
                    let mut vi = |v: u64| -> Result<usize> {
                        if let Some(i) = vidx.get(&v) {
                            return Ok(*i);
                        }
                        verts.push(s.vertex(v)?);
                        vidx.insert(v, verts.len() - 1);
                        Ok(verts.len() - 1)
                    };
                    let (a, bb) = (vi(v0)?, vi(v1)?);
                    let same = ee.params().get(4).and_then(Param::as_bool).unwrap_or(true);
                    eidx.insert(ec, edges.len());
                    edges.push((s.r(ee.params().get(3))?, same, a, bb));
                }
                l.push((ec, d));
            }
            // IGES loops run counter-clockwise about the surface normal.
            if orient != sense {
                l.reverse();
                for x in &mut l {
                    x.1 = !x.1;
                }
            }
            loops.push(l);
        }
        face_data.push((fid, s.r(fe.params().get(2))?, sense, loops));
    }
    let mut vp = vec![verts.len().to_string()];
    vp.extend(verts.iter().flatten().map(|x| real(*x)));
    let vlist = w.add(502, 1, vp);
    // Edges: the IGES edge runs along its curve (from the STEP edge's start when same sense).
    let mut ep = vec![edges.len().to_string()];
    for &(c, same, a, b) in &edges {
        let (sv, tv) = if same { (a, b) } else { (b, a) };
        let (pa, pb) = (verts.get(sv).copied().unwrap_or([0.0; 3]), verts.get(tv).copied().unwrap_or([0.0; 3]));
        let cd = curve(w, s, c, pa, pb, sv == tv)?;
        ep.extend([cd.to_string(), vlist.to_string(), (sv + 1).to_string(), vlist.to_string(), (tv + 1).to_string()]);
    }
    let elist = w.add(504, 1, ep);
    let mut sp = vec![face_data.len().to_string()];
    for (fid, surf, sense, loops) in face_data {
        let sd = surface(w, s, surf)?;
        let mut lids = Vec::new();
        for l in loops {
            let mut lp = vec![l.len().to_string()];
            for (ec, d) in l {
                let k = eidx.get(&ec).copied().unwrap_or(0);
                let same = edges.get(k).is_none_or(|e| e.1);
                lp.extend(["0".into(), elist.to_string(), (k + 1).to_string(), if d == same { "1" } else { "0" }.into(), "0".into()]);
            }
            lids.push(w.add(508, 1, lp).to_string());
        }
        let mut fp = vec![sd.to_string(), lids.len().to_string(), "1".into()];
        fp.extend(lids);
        let fd = w.add(510, 1, fp);
        if let Some(c) = colors.get(&fid) {
            let cd = w.color(*c);
            if let Some(e) = w.get_mut(fd) {
                e.color = -cd;
            }
        }
        sp.extend([fd.to_string(), if sense { "1" } else { "0" }.into()]);
    }
    Ok(w.add(514, 1, sp))
}

/// IGES text (5.3, millimetres) for bodies.
pub fn iges_export_bodies(bodies: &[ExportBody], header: &StepHeader) -> Result<String> {
    let step = crate::step_export_bodies(bodies, header)?;
    let ex = p21::parse(&step).map_err(|e| KernelError::Failed(format!("IGES export: {e}")))?;
    let s = S { ex: &ex };
    let colors = colours(&ex);
    let mut w = W::default();
    let mut solids: Vec<u64> =
        ex.entities.iter().filter(|(_, e)| matches!(e.name(), "MANIFOLD_SOLID_BREP" | "BREP_WITH_VOIDS")).map(|(i, _)| *i).collect();
    solids.sort();
    let mut max_coord: f64 = 1.0;
    for p in ex.entities.values().filter(|e| e.name() == "CARTESIAN_POINT") {
        if let Some(c) = p.params().get(1).and_then(Param::as_list) {
            for x in c.iter().filter_map(Param::as_f64) {
                max_coord = max_coord.max(x.abs());
            }
        }
    }
    for id in solids {
        let e = s.e(id)?;
        let name = e.params().first().and_then(Param::as_str).unwrap_or("").to_string();
        let outer = shell(&mut w, &s, s.r(e.params().get(1))?, &colors)?;
        let mut p = vec![outer.to_string(), "1".into()];
        let voids: Vec<i64> = if e.name() == "BREP_WITH_VOIDS" {
            s.list(e.params().get(2))?.iter().map(|v| shell(&mut w, &s, s.r(Some(v))?, &colors)).collect::<Result<_>>()?
        } else {
            Vec::new()
        };
        p.push(voids.len().to_string());
        for v in voids {
            p.extend([v.to_string(), "1".into()]);
        }
        // The body's name as a name property (406 form 15).
        let np = w.add(406, 15, vec!["1".into(), holl(&name)]);
        p.extend(["0".into(), "1".into(), np.to_string()]);
        let d = w.add(186, 0, p);
        let c = colors.get(&id).map(|c| w.color(*c));
        if let Some(ent) = w.get_mut(d) {
            ent.dependent = false;
            ent.label = name.chars().filter(|c| c.is_ascii_alphanumeric()).take(8).collect();
            if let Some(c) = c {
                ent.color = -c;
            }
        }
    }
    if w.ents.is_empty() {
        return Err(KernelError::Invalid("nothing to export".into()));
    }
    Ok(layout(&w, header, max_coord))
}

/// The file: start, global, directory and parameter sections and terminator.
fn layout(w: &W, header: &StepHeader, max_coord: f64) -> String {
    let mut out = String::new();
    let line = |out: &mut String, data: &str, sect: char, seq: usize| {
        let _ = writeln!(out, "{data:<72}{sect}{seq:>7}");
    };
    line(&mut out, "SolveCraft IGES export", 'S', 1);
    let stamp = {
        let t = header.time_stamp.replace(['-', ':', 'T'], "");
        let t: String = t.chars().filter(char::is_ascii_digit).collect();
        if t.len() >= 14 { format!("{}.{}", &t[..8], &t[8..14]) } else { "20000101.000000".to_string() }
    };
    let g = [
        "1H,".to_string(),
        "1H;".to_string(),
        holl("SolveCraft"),
        holl(&header.file_name),
        holl("SolveCraft"),
        holl("SolveCraft IGES 5.3"),
        "32".into(),
        "38".into(),
        "6".into(),
        "308".into(),
        "15".into(),
        holl(&header.file_name),
        real(1.0),
        "2".into(),
        holl("MM"),
        "1".into(),
        real(1.0),
        holl(&stamp),
        real(1e-6),
        real(max_coord),
        holl(&header.author),
        holl(&header.organization),
        "11".into(),
        "0".into(),
        holl(&stamp),
    ];
    let mut gseq = 0;
    for chunk in wrap(&g.iter().map(String::as_str).collect::<Vec<_>>(), ',', ';', 72) {
        gseq += 1;
        line(&mut out, &chunk, 'G', gseq);
    }
    // Parameter lines per entity.
    let mut pdata: Vec<Vec<String>> = Vec::with_capacity(w.ents.len());
    for e in &w.ents {
        let mut fields = vec![e.typ.to_string()];
        fields.extend(e.params.iter().cloned());
        pdata.push(wrap(&fields.iter().map(String::as_str).collect::<Vec<_>>(), ',', ';', 64));
    }
    let mut pstart = Vec::with_capacity(w.ents.len());
    let mut next = 1;
    for p in &pdata {
        pstart.push(next);
        next += p.len();
    }
    let f8 = |x: i64| format!("{x:>8}");
    for (k, e) in w.ents.iter().enumerate() {
        let status = if e.dependent { "00010000" } else { "00000000" };
        let a = format!("{}{}{}{}{}{}{}{}{status}", f8(e.typ), f8(pstart[k] as i64), f8(0), f8(0), f8(0), f8(0), f8(e.xform), f8(0));
        let b = format!("{}{}{}{}{}{}{}{:>8}{}", f8(e.typ), f8(0), f8(e.color), f8(pdata[k].len() as i64), f8(e.form), f8(0), f8(0), e.label, f8(0));
        line(&mut out, &a, 'D', 2 * k + 1);
        line(&mut out, &b, 'D', 2 * k + 2);
    }
    let mut pseq = 0;
    for (k, p) in pdata.iter().enumerate() {
        for l in p {
            pseq += 1;
            line(&mut out, &format!("{l:<64}{:>8}", 2 * k + 1), 'P', pseq);
        }
    }
    line(&mut out, &format!("S{:>7}G{:>7}D{:>7}P{:>7}", 1, gseq, 2 * w.ents.len(), pseq), 'T', 1);
    out
}

/// Fields joined by `delim`, ended by `end`, broken into lines of at most `width` columns
/// between fields (a longer field is split, as strings may be).
fn wrap(fields: &[&str], delim: char, end: char, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for (i, f) in fields.iter().enumerate() {
        let sep = if i + 1 == fields.len() { end } else { delim };
        let item = format!("{f}{sep}");
        if cur.len() + item.len() > width && !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
        }
        if item.len() > width {
            let chars: Vec<char> = item.chars().collect();
            for c in chars.chunks(width) {
                let piece: String = c.iter().collect();
                if piece.len() == width {
                    lines.push(piece);
                } else {
                    cur = piece;
                }
            }
            continue;
        }
        cur.push_str(&item);
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}
