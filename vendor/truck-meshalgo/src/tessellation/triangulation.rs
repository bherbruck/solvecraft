#![allow(clippy::many_single_char_names)]

use super::*;
use crate::filters::NormalFilters;
use crate::Point2;
use array_macro::array;
use itertools::Itertools;
use rustc_hash::FxHashMap as HashMap;

#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;

type SPoint2 = spade::Point2<f64>;
type Cdt = ConstrainedDelaunayTriangulation<SPoint2>;
type MeshedShell = Shell<Point3, PolylineCurve, Option<PolygonMesh>>;
type MeshedCShell = CompressedShell<Point3, PolylineCurve, Option<PolygonMesh>>;

pub(super) trait SP<S>:
    Fn(&S, Point3, Option<(f64, f64)>) -> Option<(f64, f64)> + Parallelizable {
}
impl<S, F> SP<S> for F where F: Fn(&S, Point3, Option<(f64, f64)>) -> Option<(f64, f64)> + Parallelizable {}

/// SolveCraft: a parameter inside the surface's bounded, non-periodic ranges.
fn in_domain<S: ParametricSurface>(surface: &S, (u, v): (f64, f64)) -> bool {
    let ok = |x: f64, range: Option<(f64, f64)>, period: Option<f64>| {
        x.is_finite()
            && match (range, period) {
                (_, Some(_)) | (None, None) => true,
                (Some((a, b)), None) => {
                    let m = (b - a).abs() * 1e-6 + 1e-9;
                    x >= a.min(b) - m && x <= a.max(b) + m
                }
            }
    };
    let (ur, vr) = surface.try_range_tuple();
    ok(u, ur, surface.u_period()) && ok(v, vr, surface.v_period())
}

/// SolveCraft: the parameter clamped into the surface's bounded, non-periodic ranges.
fn clamped<S: ParametricSurface>(surface: &S, (u, v): (f64, f64)) -> (f64, f64) {
    let c = |x: f64, range: Option<(f64, f64)>, period: Option<f64>| match (range, period) {
        (Some((a, b)), None) => x.clamp(a.min(b), a.max(b)),
        _ => x,
    };
    let (ur, vr) = surface.try_range_tuple();
    (c(u, ur, surface.u_period()), c(v, vr, surface.v_period()))
}

/// SolveCraft: a nearest-parameter search can run off a B-spline's domain and extrapolate to
/// thousands of parameter units (meshing that grid never finishes). Outside the nominal domain
/// keep the result only if it is really closer to the point than the clamped one (planes report
/// a nominal unit range but extend for ever).
fn sane<S: RobustMeshableSurface>(surface: &S, point: Point3, uv: (f64, f64)) -> (f64, f64) {
    if in_domain(surface, uv) {
        return uv;
    }
    let c = clamped(surface, uv);
    let d = |(u, v): (f64, f64)| surface.subs(u, v).distance2(point);
    if d(uv) <= d(c) { uv } else { c }
}

pub(super) fn by_search_parameter<S>(
    surface: &S,
    point: Point3,
    hint: Option<(f64, f64)>,
) -> Option<(f64, f64)>
where
    S: MeshableSurface,
{
    surface
        .search_parameter(point, hint, 100)
        .or_else(|| surface.search_parameter(point, None, 100))
}

pub(super) fn by_search_nearest_parameter<S>(
    surface: &S,
    point: Point3,
    hint: Option<(f64, f64)>,
) -> Option<(f64, f64)>
where
    S: RobustMeshableSurface,
{
    surface
        .search_parameter(point, hint, 100)
        .or_else(|| surface.search_parameter(point, None, 100))
        .or_else(|| surface.search_nearest_parameter(point, hint, 100).map(|uv| sane(surface, point, uv)))
        .or_else(|| surface.search_nearest_parameter(point, None, 100).map(|uv| sane(surface, point, uv)))
}

/// Tessellates faces
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn shell_tessellation<'a, C, S>(
    shell: &Shell<Point3, C, S>,
    tol: f64,
    sp: impl SP<S>,
) -> MeshedShell
where
    C: PolylineableCurve + 'a,
    S: PreMeshableSurface + 'a,
{
    let cap = face_cap(shell.len());
    let vmap: HashMap<_, _> = shell
        .vertex_par_iter()
        .map(|v| (v.id(), v.mapped(Point3::clone)))
        .collect();
    let eset: HashMap<_, _> = shell.edge_par_iter().map(move |e| (e.id(), e)).collect();
    let edge_map: HashMap<_, _> = eset
        .into_par_iter()
        .map(move |(id, edge)| {
            let v0 = vmap.get(&edge.absolute_front().id()).unwrap();
            let v1 = vmap.get(&edge.absolute_back().id()).unwrap();
            let curve = edge.curve();
            let poly = bounded_polyline(&curve, curve.range_tuple(), tol);
            (id, Edge::new_unchecked(v0, v1, poly))
        })
        .collect();
    let create_edge = |edge: &Edge<Point3, C>| -> Edge<_, _> {
        let new_edge = edge_map.get(&edge.id()).unwrap();
        match edge.orientation() {
            true => new_edge.clone(),
            false => new_edge.inverse(),
        }
    };
    let create_boundary =
        |wire: &Wire<Point3, C>| -> Wire<_, _> { wire.edge_iter().map(create_edge).collect() };
    let create_face = move |face: &Face<Point3, C, S>| -> Face<_, _, _> {
        let wires: Vec<_> = face
            .absolute_boundaries()
            .iter()
            .map(create_boundary)
            .collect();
        shell_create_polygon(&face.surface(), wires, face.orientation(), tol, &sp, cap)
    };
    shell.face_par_iter().map(create_face).collect()
}

/// Tessellates faces
#[cfg(any(target_arch = "wasm32", test))]
pub(super) fn shell_tessellation_single_thread<'a, C, S>(
    shell: &'a Shell<Point3, C, S>,
    tol: f64,
    sp: impl SP<S>,
) -> MeshedShell
where
    C: PolylineableCurve + 'a,
    S: PreMeshableSurface + 'a,
{
    use truck_base::entry_map::FxEntryMap as EntryMap;
    use truck_topology::Vertex as TVertex;
    let cap = face_cap(shell.len());
    let mut vmap = EntryMap::new(
        move |v: &TVertex<Point3>| v.id(),
        move |v| v.mapped(Point3::clone),
    );
    let mut edge_map = EntryMap::new(
        move |edge: &'a Edge<Point3, C>| edge.id(),
        move |edge| {
            let vf = edge.absolute_front();
            let v0 = vmap.entry_or_insert(vf).clone();
            let vb = edge.absolute_back();
            let v1 = vmap.entry_or_insert(vb).clone();
            let curve = edge.curve();
            let poly = bounded_polyline(&curve, curve.range_tuple(), tol);
            Edge::new_unchecked(&v0, &v1, poly)
        },
    );
    let mut create_edge = move |edge: &'a Edge<Point3, C>| -> Edge<_, _> {
        let new_edge = edge_map.entry_or_insert(edge);
        match edge.orientation() {
            true => new_edge.clone(),
            false => new_edge.inverse(),
        }
    };
    let mut create_boundary = move |wire: &'a Wire<Point3, C>| -> Wire<_, _> {
        wire.edge_iter().map(&mut create_edge).collect()
    };
    let create_face = move |face: &'a Face<Point3, C, S>| -> Face<_, _, _> {
        let wires: Vec<_> = face
            .absolute_boundaries()
            .iter()
            .map(&mut create_boundary)
            .collect();
        shell_create_polygon(&face.surface(), wires, face.orientation(), tol, &sp, cap)
    };
    shell.face_iter().map(create_face).collect()
}

/// Tessellates faces
pub(super) fn cshell_tessellation<'a, C, S>(
    shell: &CompressedShell<Point3, C, S>,
    tol: f64,
    sp: impl SP<S>,
) -> MeshedCShell
where
    C: PolylineableCurve + 'a,
    S: PreMeshableSurface + 'a,
{
    let vertices = shell.vertices.clone();
    let cap = face_cap(shell.faces.len());
    let tessellate_edge = |edge: &CompressedEdge<C>| {
        let curve = &edge.curve;
        CompressedEdge {
            vertices: edge.vertices,
            curve: bounded_polyline(curve, curve.range_tuple(), tol),
        }
    };
    #[cfg(not(target_arch = "wasm32"))]
    let edges: Vec<_> = shell.edges.par_iter().map(tessellate_edge).collect();
    #[cfg(target_arch = "wasm32")]
    let edges: Vec<_> = shell.edges.iter().map(tessellate_edge).collect();
    let tessellate_face = |face: &CompressedFace<S>| {
        let boundaries = face.boundaries.clone();
        let surface = &face.surface;
        let create_edge = |edge_idx: &CompressedEdgeIndex| match edge_idx.orientation {
            true => Some(edges.get(edge_idx.index)?.curve.clone()),
            false => Some(edges.get(edge_idx.index)?.curve.inverse()),
        };
        let create_boundary = |wire: &Vec<CompressedEdgeIndex>| {
            let wire_iter = wire.iter().filter_map(create_edge);
            PolyBoundaryPiece::try_new(surface, wire_iter, &sp)
        };
        let preboundary: Option<Vec<_>> = boundaries.iter().map(create_boundary).collect();
        let polygon: Option<PolygonMesh> = (|| {
            let boundary = PolyBoundary::new(preboundary?, &surface, tol);
            Some(trimming_tessellation(&surface, &boundary, tol, cap))
        })();
        CompressedFace {
            boundaries,
            orientation: face.orientation,
            surface: polygon,
        }
    };
    #[cfg(not(target_arch = "wasm32"))]
    let faces = shell.faces.par_iter().map(tessellate_face).collect();
    #[cfg(target_arch = "wasm32")]
    let faces = shell.faces.iter().map(tessellate_face).collect();
    MeshedCShell {
        vertices,
        edges,
        faces,
    }
}

fn shell_create_polygon<S: PreMeshableSurface>(
    surface: &S,
    wires: Vec<Wire<Point3, PolylineCurve>>,
    orientation: bool,
    tol: f64,
    sp: impl SP<S>,
    cap: usize,
) -> Face<Point3, PolylineCurve, Option<PolygonMesh>> {
    let preboundary = wires
        .iter()
        .map(|wire: &Wire<_, _>| {
            let wire_iter = wire.iter().map(Edge::oriented_curve);
            PolyBoundaryPiece::try_new(surface, wire_iter, &sp)
        })
        .collect::<Option<Vec<_>>>();
    let polygon: Option<PolygonMesh> = (|| {
        let boundary = PolyBoundary::new(preboundary?, &surface, tol);
        Some(trimming_tessellation(surface, &boundary, tol, cap))
    })();
    // SolveCraft: faces with seam edges (a loop running along an edge both ways, as STEP
    // writes cylinders) are valid here; `debug_new` asserts simple loops in debug builds.
    let mut new_face = Face::new_unchecked(wires, polygon);
    if !orientation {
        new_face.invert();
    }
    new_face
}

#[derive(Debug, Default, Clone)]
struct PolyBoundaryPiece(Vec<Point2>);

impl PolyBoundaryPiece {
    fn try_new<S: PreMeshableSurface>(
        surface: &S,
        wire: impl Iterator<Item = PolylineCurve>,
        sp: impl SP<S>,
    ) -> Option<Self> {
        let (up, vp) = (surface.u_period(), surface.v_period());
        let (urange, vrange) = surface.try_range_tuple();
        let mut bdry3d: Vec<Point3> = wire
            .flat_map(|poly_edge| {
                let n = poly_edge.len() - 1;
                poly_edge.into_iter().take(n)
            })
            .collect();
        bdry3d.push(bdry3d[0]);
        let mut previous = None;
        let mut vec = bdry3d
            .into_iter()
            .flat_map(|pt| {
                let (mut u, mut v) = match sp(surface, pt, previous) {
                    Some(hint) => hint,
                    None => return vec![None],
                };
                if let (Some(up), Some((u0, _))) = (up, previous) {
                    u = get_mindiff(u, u0, up);
                }
                if let (Some(vp), Some((_, v0))) = (vp, previous) {
                    v = get_mindiff(v, v0, vp);
                }
                let res = (|| {
                    if let Some((u0, v0)) = previous {
                        if !u0.near(&u) && surface.uder(u0, v0).so_small() {
                            return vec![Some(Point2::new(u, v0)), Some(Point2::new(u, v))];
                        } else if !v0.near(&v) && surface.vder(u0, v0).so_small() {
                            return vec![Some(Point2::new(u0, v)), Some(Point2::new(u, v))];
                        }
                    }
                    vec![Some(Point2::new(u, v))]
                })();
                previous = Some((u, v));
                res
            })
            .collect::<Option<Vec<Point2>>>()?;
        let grav = vec.iter().fold(Point2::origin(), |g, p| g + p.to_vec()) / vec.len() as f64;
        if let (Some(up), Some((u0, _))) = (up, urange) {
            let quot = f64::floor((grav.x - u0) / up);
            vec.iter_mut().for_each(|p| p.x -= quot * up);
        }
        if let (Some(vp), Some((v0, _))) = (vp, vrange) {
            let quot = f64::floor((grav.y - v0) / vp);
            vec.iter_mut().for_each(|p| p.y -= quot * vp);
        }
        let last = *vec.last().unwrap();
        if !vec[0].near(&last) {
            let Point2 { x: u0, y: v0 } = last;
            if surface.uder(u0, v0).so_small() || surface.vder(u0, v0).so_small() {
                vec.push(vec[0]);
            }
        }
        Some(Self(vec))
    }
}

fn abs_diff(previous: f64) -> impl Fn(&f64, &f64) -> std::cmp::Ordering {
    let f = move |x: &f64| f64::abs(x - previous);
    move |x: &f64, y: &f64| f(x).partial_cmp(&f(y)).unwrap()
}
fn get_mindiff(u: f64, u0: f64, up: f64) -> f64 {
    let closure = |i| u + i as f64 * up;
    (-2..=2).map(closure).min_by(abs_diff(u0)).unwrap()
}

#[derive(Debug, Default, Clone)]
struct PolyBoundary(Vec<Vec<Point2>>);

fn normalize_range(curve: &mut Vec<Point2>, compidx: usize, (u0, u1): (f64, f64)) {
    let p = curve[0];
    let q = curve[curve.len() - 1];
    let tmp = f64::min(p[compidx], q[compidx]) + TOLERANCE;
    let del = f64::floor((tmp - u0) / (u1 - u0)) * (u1 - u0);
    curve.iter_mut().for_each(|p| p[compidx] -= del);
    let Some(i) = curve
        .iter()
        .position(|p| (curve[0][compidx] - u1) * (p[compidx] - u1) < 0.0)
    else {
        return;
    };
    let mut curve1 = curve.split_off(i + 1);
    curve1.pop();
    curve1.insert(0, curve[i]);
    match curve[0][compidx] < curve[curve.len() - 1][compidx] {
        true => curve1.iter_mut(),
        false => curve.iter_mut(),
    }
    .for_each(|p| p[compidx] -= u1 - u0);
    curve1.append(curve);
    *curve = curve1;
}

fn loop_orientation(curve: &[Point2]) -> bool {
    curve
        .iter()
        .circular_tuple_windows()
        .fold(0.0, |sum, (p, q)| sum + (q.x + p.x) * (q.y - p.y))
        > 0.0
}

impl PolyBoundary {
    fn new(pieces: Vec<PolyBoundaryPiece>, surface: &impl PreMeshableSurface, tol: f64) -> Self {
        let (mut closed, mut open) = (Vec::new(), Vec::new());
        pieces.into_iter().for_each(|PolyBoundaryPiece(mut vec)| {
            match vec[0].distance(vec[vec.len() - 1]) < 1.0e-3 {
                true => {
                    vec.pop();
                    closed.push(vec)
                }
                false => open.push(vec),
            }
        });
        fn connect_edges(vecs: impl IntoIterator<Item = Vec<Point2>>) -> Vec<Point2> {
            let closure = |vec: Vec<Point2>| {
                let len = vec.len();
                vec.into_iter().take(len - 1)
            };
            vecs.into_iter().flat_map(closure).collect()
        }
        match open.len() {
            1 => {
                let mut curve = open.pop().unwrap();
                let p = curve[0];
                let q = curve[curve.len() - 1];
                if let (Some((u0, u1)), Some((v0, v1))) = surface.try_range_tuple() {
                    if p.x < q.x - TOLERANCE {
                        normalize_range(&mut curve, 0, (u0, u1));
                        let p = curve[0];
                        let q = curve[curve.len() - 1];
                        let x = Point2::new(u0, v1);
                        let y = Point2::new(u1, v1);
                        let vec0 = polyline_on_surface(surface, q, y, tol);
                        let vec1 = polyline_on_surface(surface, y, x, tol);
                        let vec2 = polyline_on_surface(surface, x, p, tol);
                        closed.push(connect_edges([vec0, vec1, vec2, curve]));
                    } else if q.x < p.x - TOLERANCE {
                        normalize_range(&mut curve, 0, (u0, u1));
                        let p = curve[0];
                        let q = curve[curve.len() - 1];
                        let x = Point2::new(u1, v0);
                        let y = Point2::new(u0, v0);
                        let vec0 = polyline_on_surface(surface, q, y, tol);
                        let vec1 = polyline_on_surface(surface, y, x, tol);
                        let vec2 = polyline_on_surface(surface, x, p, tol);
                        closed.push(connect_edges([vec0, vec1, vec2, curve]));
                    } else if p.y < q.y - TOLERANCE {
                        normalize_range(&mut curve, 1, (v0, v1));
                        let p = curve[0];
                        let q = curve[curve.len() - 1];
                        let x = Point2::new(u0, v0);
                        let y = Point2::new(u0, v1);
                        let vec0 = polyline_on_surface(surface, q, y, tol);
                        let vec1 = polyline_on_surface(surface, y, x, tol);
                        let vec2 = polyline_on_surface(surface, x, p, tol);
                        closed.push(connect_edges([vec0, vec1, vec2, curve]));
                    } else if q.y < p.y - TOLERANCE {
                        normalize_range(&mut curve, 1, (v0, v1));
                        let p = curve[0];
                        let q = curve[curve.len() - 1];
                        let x = Point2::new(u1, v1);
                        let y = Point2::new(u1, v0);
                        let vec0 = polyline_on_surface(surface, q, y, tol);
                        let vec1 = polyline_on_surface(surface, y, x, tol);
                        let vec2 = polyline_on_surface(surface, x, p, tol);
                        closed.push(connect_edges([vec0, vec1, vec2, curve]));
                    }
                }
            }
            2 => {
                let mut curve1 = open.pop().unwrap();
                let mut curve0 = open.pop().unwrap();
                fn end_pts<T: Copy>(vec: &[T]) -> (T, T) { (vec[0], vec[vec.len() - 1]) }
                let ((p0, p1), (q0, q1)) = (end_pts(&curve0), end_pts(&curve1));
                if !p0.x.near(&p1.x) && !q0.x.near(&q1.x) {
                    if let (Some(urange), _) = surface.try_range_tuple() {
                        normalize_range(&mut curve0, 0, urange);
                        normalize_range(&mut curve1, 0, urange);
                    }
                } else if !p0.y.near(&p1.y) && !q0.y.near(&q1.y) {
                    if let (_, Some(vrange)) = surface.try_range_tuple() {
                        normalize_range(&mut curve0, 1, vrange);
                        normalize_range(&mut curve1, 1, vrange);
                    }
                }
                let ((p0, p1), (q0, q1)) = (end_pts(&curve0), end_pts(&curve1));
                let vec0 = polyline_on_surface(surface, p1, q0, tol);
                let vec1 = polyline_on_surface(surface, q1, p0, tol);
                closed.push(connect_edges([curve0, vec0, curve1, vec1]));
            }
            _ => {}
        }
        if !closed.iter().any(|curve| loop_orientation(curve)) {
            if let (Some((u0, u1)), Some((v0, v1))) = surface.try_range_tuple() {
                let p = [
                    Point2::new(u0, v0),
                    Point2::new(u1, v0),
                    Point2::new(u1, v1),
                    Point2::new(u0, v1),
                ];
                let vec0 = polyline_on_surface(surface, p[0], p[1], tol);
                let vec1 = polyline_on_surface(surface, p[1], p[2], tol);
                let vec2 = polyline_on_surface(surface, p[2], p[3], tol);
                let vec3 = polyline_on_surface(surface, p[3], p[0], tol);
                closed.push(connect_edges([vec0, vec1, vec2, vec3]));
            }
        }
        Self(closed)
    }

    /// whether `c` is included in the domain with boundary = `self`.
    fn include(&self, c: Point2) -> bool {
        let t = 2.0 * std::f64::consts::PI * HashGen::hash1(c);
        let r = Vector2::new(f64::cos(t), f64::sin(t));
        self.0
            .iter()
            .flat_map(|vec| vec.iter().circular_tuple_windows())
            .try_fold(0_i32, move |counter, (p0, p1)| {
                let a = p0 - c;
                let b = p1 - c;
                let s0 = r.x * a.y - r.y * a.x; // v times a
                let s1 = r.x * b.y - r.y * b.x; // v times b
                let s2 = a.x * b.y - a.y * b.x; // a times b
                let x = s2 / (s1 - s0);
                if x.so_small() && s0 * s1 < 0.0 {
                    None
                } else if x > 0.0 && s0 <= 0.0 && s1 > 0.0 {
                    Some(counter + 1)
                } else if x > 0.0 && s0 >= 0.0 && s1 < 0.0 {
                    Some(counter - 1)
                } else {
                    Some(counter)
                }
            })
            .map(|counter| counter > 0)
            .unwrap_or(false)
    }

    /// Inserts points and adds constraint into triangulation.
    fn insert_to(&self, triangulation: &mut Cdt) {
        let poly2tri: Vec<_> = self
            .0
            .iter()
            .flatten()
            .map(|pt| {
                let p = [spade_round(pt.x), spade_round(pt.y)];
                triangulation.insert(SPoint2::from(p)).ok()
            })
            .collect();
        let mut prev: Option<usize> = None;
        let mut counter = 0;
        self.0
            .iter()
            .map(Vec::len)
            .flat_map(|len| {
                let range = counter..counter + len;
                counter += len;
                range.circular_tuple_windows()
            })
            .for_each(|(i, j)| {
                let Some(vj) = poly2tri[j] else { return };
                if let Some(p) = prev {
                    let Some(v) = poly2tri[p] else { return };
                    if triangulation.can_add_constraint(v, vj) {
                        triangulation.add_constraint(v, vj);
                        prev = None;
                    }
                } else {
                    let Some(vi) = poly2tri[i] else { return };
                    if triangulation.can_add_constraint(vi, vj) {
                        triangulation.add_constraint(vi, vj);
                    } else {
                        prev = Some(i);
                    }
                }
            });
    }
}

fn spade_round(x: f64) -> f64 {
    match f64::abs(x) < MIN_ALLOWED_VALUE {
        true => 0.0,
        false => x,
    }
}

/// Tessellates one surface trimmed by polyline.
fn trimming_tessellation<S>(surface: &S, polyboundary: &PolyBoundary, tol: f64, cap: usize) -> PolygonMesh
where S: PreMeshableSurface {
    let mut triangulation = Cdt::new();
    polyboundary.insert_to(&mut triangulation);
    insert_surface(&mut triangulation, surface, polyboundary, tol, cap);
    let mut mesh = triangulation_into_polymesh(
        triangulation.vertices(),
        triangulation.inner_faces(),
        surface,
        polyboundary,
    );
    mesh.make_face_compatible_to_normal();
    mesh
}

/// Inserts parameter divisions into triangulation.
fn insert_surface(
    triangulation: &mut Cdt,
    surface: impl PreMeshableSurface,
    polyline: &PolyBoundary,
    tol: f64,
    cap: usize,
) {
    let bdb: BoundingBox<Point2> = polyline.0.iter().flatten().collect();
    let range = ((bdb.min()[0], bdb.max()[0]), (bdb.min()[1], bdb.max()[1]));
    if !(range.0 .0.is_finite() && range.0 .1.is_finite() && range.1 .0.is_finite() && range.1 .1.is_finite()) {
        return;
    }
    let (udiv, vdiv) = bounded_surface_division(&surface, range, tol, cap);
    let (udiv, vdiv) = balanced_grid(&surface, udiv, vdiv, cap);
    let insert_res: Vec<Vec<Option<_>>> = udiv
        .into_iter()
        .map(|u| {
            vdiv.iter()
                .map(|v| match polyline.include(Point2::new(u, *v)) {
                    true => triangulation.insert(SPoint2::new(u, *v)).ok(),
                    false => None,
                })
                .collect()
        })
        .collect();
    insert_res.windows(2).for_each(|vec| {
        vec[0].windows(2).zip(&vec[1]).for_each(|(a, z)| {
            if let Some(x) = a[0] {
                if let Some(y) = a[1] {
                    if triangulation.can_add_constraint(x, y) {
                        triangulation.add_constraint(x, y);
                    }
                }
                if let Some(z) = z {
                    if triangulation.can_add_constraint(x, *z) {
                        triangulation.add_constraint(x, *z);
                    }
                }
            }
        });
        let idx = vec[0].len() - 1;
        if let (Some(x), Some(y)) = (vec[0][idx], vec[1][idx]) {
            if triangulation.can_add_constraint(x, y) {
                triangulation.add_constraint(x, y);
            }
        }
    });
}

/// SolveCraft: most grid points a face gets (keeps hostile or huge faces bounded).
pub(super) const MAX_GRID: usize = 150_000;
/// SolveCraft: grid points shared by all faces of one shell; many faces get fewer each.
pub(super) const SHELL_GRID_BUDGET: usize = 10_000_000;
/// SolveCraft: most points on one edge's polyline.
const MAX_EDGE_POINTS: usize = 20_000;

/// SolveCraft: the grid budget of each face of a shell with `faces` faces.
pub(super) fn face_cap(faces: usize) -> usize {
    (SHELL_GRID_BUDGET / faces.max(1)).clamp(2_000, MAX_GRID)
}

/// SolveCraft: truck's curve division, bounded: at most `MAX_EDGE_POINTS` points and a
/// bisection depth of 24; a midpoint that does not evaluate (NaN) counts as flat (truck's
/// version recursed on it 100 levels deep, which never finishes).
fn bounded_polyline<C: PolylineableCurve>(curve: &C, range: (f64, f64), tol: f64) -> PolylineCurve {
    let mut out = vec![curve.subs(range.0)];
    let mut stack: Vec<(f64, f64, usize)> = vec![(range.0, range.1, 0)];
    while let Some((a, b, depth)) = stack.pop() {
        let (pa, pb) = (curve.subs(a), curve.subs(b));
        let t = a + (b - a) * 0.4729;
        let chord = pa + (pb - pa) * 0.4729;
        let d2 = curve.subs(t).distance2(chord);
        let tm = 0.5 * (a + b);
        let dm = curve.subs(tm).distance2(pa.midpoint(pb));
        let flat = !(d2 >= tol * tol || dm >= tol * tol);
        if flat || depth >= 24 || out.len() + stack.len() >= MAX_EDGE_POINTS {
            out.push(pb);
        } else {
            // Right half after the left one (stack order).
            stack.push((tm, b, depth + 1));
            stack.push((a, tm, depth + 1));
        }
    }
    PolylineCurve(out)
}

/// SolveCraft: truck's surface division, bounded: stops refining when the grid would exceed
/// `cap` points or after 24 rounds; cells that do not evaluate (NaN) are not refined.
fn bounded_surface_division<S: PreMeshableSurface>(surface: &S, (ur, vr): ((f64, f64), (f64, f64)), tol: f64, cap: usize) -> (Vec<f64>, Vec<f64>) {
    let (mut udiv, mut vdiv) = (vec![ur.0, ur.1], vec![vr.0, vr.1]);
    for _ in 0..24 {
        let mut fu = vec![false; udiv.len() - 1];
        let mut fv = vec![false; vdiv.len() - 1];
        for (i, u) in udiv.windows(2).enumerate() {
            for (j, v) in vdiv.windows(2).enumerate() {
                if fu[i] && fv[j] {
                    continue;
                }
                let (p, q) = (0.4729, 0.5271);
                let (u0, v0) = (u[0] * (1.0 - p) + u[1] * p, v[0] * (1.0 - q) + v[1] * q);
                let p0 = surface.subs(u0, v0);
                let pt00 = surface.subs(u[0], v[0]).to_vec();
                let pt01 = surface.subs(u[0], v[1]).to_vec();
                let pt10 = surface.subs(u[1], v[0]).to_vec();
                let pt11 = surface.subs(u[1], v[1]).to_vec();
                let pt = Point3::from_vec(pt00 * (1.0 - p) * (1.0 - q) + pt01 * (1.0 - p) * q + pt10 * p * (1.0 - q) + pt11 * p * q);
                let far = p0.distance2(pt) > tol * tol;
                // Which direction bends: refine only that one (a cylinder needs no division
                // along its rulings).
                let alu = surface.subs(u[0], v0).to_vec() * (1.0 - p) + surface.subs(u[1], v0).to_vec() * p;
                let alv = surface.subs(u0, v[0]).to_vec() * (1.0 - q) + surface.subs(u0, v[1]).to_vec() * q;
                let bend_u = p0.distance2(Point3::from_vec(alu)) > tol * tol;
                let bend_v = p0.distance2(Point3::from_vec(alv)) > tol * tol;
                if bend_u || bend_v {
                    fu[i] |= bend_u;
                    fv[j] |= bend_v;
                } else if far {
                    fu[i] = true;
                    fv[j] = true;
                }
            }
        }
        let nu = udiv.len() + fu.iter().filter(|x| **x).count();
        let nv = vdiv.len() + fv.iter().filter(|x| **x).count();
        if nu == udiv.len() && nv == vdiv.len() {
            break;
        }
        if nu.saturating_mul(nv) > cap {
            break;
        }
        let refine = |d: &[f64], f: &[bool]| {
            let mut n = vec![d[0]];
            for (w, b) in d.windows(2).zip(f) {
                if *b {
                    n.push(0.5 * (w[0] + w[1]));
                }
                n.push(w[1]);
            }
            n
        };
        udiv = refine(&udiv, &fu);
        vdiv = refine(&vdiv, &fv);
    }
    (udiv, vdiv)
}

/// SolveCraft: refine the parameter grid so its cells are not much longer in one direction than
/// the other. A ruled surface (cylinder, cone) needs no division along its rulings, and the
/// triangles that then join a trimming loop to the far ends of the grid cut through the
/// surface (a cylinder with a hole lost volume this way).
fn balanced_grid(surface: &impl PreMeshableSurface, udiv: Vec<f64>, vdiv: Vec<f64>, cap: usize) -> (Vec<f64>, Vec<f64>) {
    let mid = |d: &[f64]| if d.is_empty() { 0.0 } else { d[d.len() / 2] };
    let (um, vm) = (mid(&udiv), mid(&vdiv));
    let step = |d: &[f64], along_u: bool| -> Vec<f64> {
        d.windows(2)
            .map(|w| {
                let (a, b) = (w[0], w[1]);
                if along_u { surface.subs(a, vm).distance(surface.subs(b, vm)) } else { surface.subs(um, a).distance(surface.subs(um, b)) }
            })
            .collect()
    };
    let (lu, lv) = (step(&udiv, true), step(&vdiv, false));
    let typical = |l: &[f64]| {
        let mut v: Vec<f64> = l.iter().copied().filter(|x| x.is_finite() && *x > 0.0).collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        if v.is_empty() { None } else { Some(v[v.len() / 2]) }
    };
    let refine = |d: &[f64], l: &[f64], target: Option<f64>| -> Vec<f64> {
        let Some(t) = target.filter(|t| *t > 0.0) else { return d.to_vec() };
        // At most 24 cells along a ruling: enough for holes and trims to get short triangles.
        let total: f64 = l.iter().filter(|x| x.is_finite()).sum();
        let t = t.max(total / 24.0 / 4.0);
        let mut out = Vec::with_capacity(d.len());
        for (i, w) in d.windows(2).enumerate() {
            let n = l.get(i).map(|x| (x / (4.0 * t)).ceil()).filter(|n| n.is_finite()).unwrap_or(1.0).clamp(1.0, 64.0) as usize;
            for k in 0..n {
                out.push(w[0] + (w[1] - w[0]) * k as f64 / n as f64);
            }
        }
        if let Some(last) = d.last() {
            out.push(*last);
        }
        out
    };
    // Only a direction left undivided (a ruling) needs it; curved directions already follow
    // the tolerance.
    let u2 = if udiv.len() <= 3 { refine(&udiv, &lu, typical(&lv)) } else { udiv.clone() };
    let v2 = if vdiv.len() <= 3 { refine(&vdiv, &lv, typical(&lu)) } else { vdiv.clone() };
    let (u2, v2) = if u2.len().saturating_mul(v2.len()) > cap { (udiv, vdiv) } else { (u2, v2) };
    // Never more than MAX_GRID points: thin out evenly (the mesh gets coarser, not endless).
    let total = u2.len().saturating_mul(v2.len());
    if total <= cap {
        return (u2, v2);
    }
    let f = (total as f64 / cap.max(4) as f64).sqrt();
    let thin = |d: Vec<f64>| -> Vec<f64> {
        let n = ((d.len() as f64 / f).ceil() as usize).max(2);
        if d.len() <= n {
            return d;
        }
        (0..n).map(|i| d[i * (d.len() - 1) / (n - 1)]).collect()
    };
    (thin(u2), thin(v2))
}

/// Converts triangulation into `PolygonMesh`.
fn triangulation_into_polymesh<'a>(
    vertices: VertexIterator<'a, SPoint2, (), CdtEdge<()>, ()>,
    triangles: InnerFaceIterator<'a, SPoint2, (), CdtEdge<()>, ()>,
    surface: &impl ParametricSurface3D,
    polyline: &PolyBoundary,
) -> PolygonMesh {
    let mut positions = Vec::<Point3>::new();
    let mut uv_coords = Vec::<Vector2>::new();
    let mut normals = Vec::<Vector3>::new();
    let vmap: HashMap<_, _> = vertices
        .enumerate()
        .map(|(i, v)| {
            let p = *v.as_ref();
            let uv = Vector2::new(p.x, p.y);
            positions.push(surface.subs(uv[0], uv[1]));
            uv_coords.push(uv);
            normals.push(surface.normal(uv[0], uv[1]));
            (v.fix(), i)
        })
        .collect();
    let tri_faces: Vec<[StandardVertex; 3]> = triangles
        .map(|tri| tri.vertices())
        .filter(|tri| {
            fn sp2cg(p: SPoint2) -> Point2 { Point2::new(p.x, p.y) }
            let tri = array![i => sp2cg(*tri[i].as_ref()); 3];
            let (a, b) = (tri[1] - tri[0], tri[2] - tri[0]);
            let c = tri[0] + (a + b) / 3.0;
            let area = a.x * b.y - a.y * b.x;
            polyline.include(c) && !area.so_small2()
        })
        .map(|tri| {
            let idcs = array![i => vmap[&tri[i].fix()]; 3];
            array![i => [idcs[i], idcs[i], idcs[i]].into(); 3]
        })
        .collect();
    PolygonMesh::debug_new(
        StandardAttributes {
            positions,
            uv_coords,
            normals,
        },
        Faces::from_tri_and_quad_faces(tri_faces, Vec::new()),
    )
}

fn polyline_on_surface(
    surface: impl PreMeshableSurface,
    p: Point2,
    q: Point2,
    tol: f64,
) -> Vec<Point2> {
    use truck_geometry::prelude::*;
    let line = Line(p, q);
    let pcurve = PCurve::new(line, surface);
    let (vec, _) = pcurve.parameter_division(pcurve.range_tuple(), tol);
    vec.into_iter().map(|t| line.subs(t)).collect()
}

#[test]
#[ignore]
#[cfg(not(target_arch = "wasm32"))]
fn par_bench() {
    use std::time::Instant;
    use truck_modeling::*;
    const JSON: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../resources/shape/bottle.json"
    ));
    let solid: Solid = serde_json::from_str(JSON).unwrap();
    let shell = solid.into_boundaries().pop().unwrap();

    let instant = Instant::now();
    (0..100).for_each(|_| {
        let _shell = shell_tessellation(&shell, 0.01, by_search_parameter);
    });
    println!("{}ms", instant.elapsed().as_millis());

    let instant = Instant::now();
    (0..100).for_each(|_| {
        let _shell = shell_tessellation_single_thread(&shell, 0.01, by_search_parameter);
    });
    println!("{}ms", instant.elapsed().as_millis());
}
