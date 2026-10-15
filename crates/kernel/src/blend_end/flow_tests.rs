//! Trace physical paths through both endpoint fills and the retained cylinder.
//! Checking only boundary tangency missed waves inside otherwise valid patches.

use super::*;
use crate::Body;
use mt::{InnerSpace, ParametricSurface};

#[test]
fn sloped_corner_flow_stays_smooth_from_floor_to_rim() {
    check_flow(&sloped_basin_with_fillets(), true, false);
}

fn sloped_basin_with_fillets() -> Body {
    placed_basin_with_fillets(1.0, Vec3::ZERO, false)
}

fn placed_basin_with_fillets(scale: f64, offset: Vec3, front: bool) -> Body {
    let place = |p: Vec3| p * scale + offset;
    let rim = [Vec3::new(45.0, 1.8, 16.0), Vec3::new(45.0, 45.0, 16.0), Vec3::new(15.0, 23.4, 16.0), Vec3::new(75.0, 23.4, 16.0)];
    let base = crate::transform_matrix(
        &super::tests::shallow_basin(),
        [[scale, 0.0, 0.0, 0.0], [0.0, scale, 0.0, 0.0], [0.0, 0.0, scale, 0.0], [offset.x, offset.y, offset.z, 1.0]],
    )
    .unwrap();
    let mut body = crate::fillet(&base, &rim.map(place), 0.3 * scale).unwrap();
    let inset = 12.0 * 10.0_f64.to_radians().tan();
    for edge in [
        Vec3::new(15.0 + inset, 40.0 - inset, 4.0),
        Vec3::new(75.0 - inset, 40.0 - inset, 4.0),
        Vec3::new(45.0, 45.0 - inset, 4.0),
        Vec3::new(45.0, 35.0 - inset, 4.0),
        Vec3::new(15.0 + inset * 0.5, if front { (1.8 + 35.0 - inset) * 0.5 } else { 45.0 - inset * 0.5 }, 10.0),
    ] {
        body = crate::fillet(&body, &[place(edge)], 1.5 * scale).unwrap();
    }
    assert!(body.history().is_some(), "endpoint fillets must retain topology history");
    body
}

#[test]
fn perpendicular_corner_flow_stays_smooth_at_rim() {
    let slab = crate::box_solid(Vec3::ZERO, Vec3::new(90.0, 48.0, 16.0)).unwrap();
    let tool = crate::box_solid(Vec3::new(15.0, 10.0, 4.0), Vec3::new(75.0, 40.0, 18.0)).unwrap();
    let body = crate::boolean(&slab, &tool, crate::BoolOp::Cut).unwrap().unwrap();
    let rim = [Vec3::new(45.0, 10.0, 16.0), Vec3::new(45.0, 40.0, 16.0), Vec3::new(15.0, 25.0, 16.0), Vec3::new(75.0, 25.0, 16.0)];
    let body = crate::fillet(&body, &rim, 0.3).unwrap();
    let body = crate::fillet(&body, &[Vec3::new(15.0, 40.0, 10.0)], 1.5).unwrap();
    check_flow(&body, false, false);
}

fn check_flow(body: &Body, rounded_floor: bool, front: bool) {
    assert!(body.validity().is_empty());
    assert_eq!(body.unmeshed_faces(), 0);
    let mesh = body.display_mesh_angular(body.size() * 0.001, crate::DISPLAY_ANGLE).unwrap();
    let faces: Vec<_> = body.solid.face_iter().collect();
    let (origin, axis) = faces
        .iter()
        .find_map(|f| match crate::offset::surf_of(f, 1e-6) {
            Some(crate::offset::Surf::Cylinder { o, a, r, .. })
                if (if front { (0.2..0.5).contains(&a.z.abs()) } else { a.z.abs() > 0.9 }) && (r - 1.5).abs() < 1e-6 =>
            {
                Some((o, if a.z > 0.0 { a } else { -a }))
            }
            _ => None,
        })
        .unwrap();
    let regions: Vec<_> = faces
        .iter()
        .map(|f| {
            // Fan fills occupy a complete unit square. Their singular corners
            // can defeat a generic trim inversion, so verify the four sides and
            // use the exact parameter bounds for these surfaces.
            if matches!(f.surface(), mt::Surface::BSplineSurface(s) if s.degrees()==(5,5)) {
                assert_eq!(f.boundaries().len(), 1);
                assert_eq!(f.edge_iter().count(), 4);
                let surface = crate::body::evaluation::Surface(f.oriented_surface());
                for edge in f.edge_iter() {
                    let curve = edge.oriented_curve();
                    let (lo, hi) = curve.range_tuple();
                    for t in [0.25, 0.5, 0.75] {
                        let (u, v) = surface.search_nearest_parameter(curve.subs(lo + (hi - lo) * t), None, 100).unwrap();
                        assert!(u.abs().min(v.abs()).min((1.0 - u).abs()).min((1.0 - v).abs()) < 1e-4);
                    }
                }
                None
            } else {
                Some(crate::blend_trim::FaceRegion::new(f, 1e-4).unwrap())
            }
        })
        .collect();
    let max_distance = if front { 10.0 } else { 4.0 };
    let normal_at = |z: f64, direction: Vec3| {
        let across = Vec3::Z.cross(direction);
        let a = mt::Vector3::unit_z();
        let b = mt::Vector3::new(across.x, across.y, across.z);
        let o = origin + axis * ((z - origin.z) / axis.z);
        let (distance, index) = mesh.raycast(o, direction).expect("the complete corner path must be covered");
        let picked = mesh.tri_face[index] as usize;
        let p = o + direction * if distance < max_distance { distance } else { 1.5 };
        // A triangle can straddle a curved trim by its chord tolerance.
        // Fall back to the other faces, accepting only a forward exact hit
        // inside its trim and within the corner. Extrapolating the first
        // surface invents normal jumps, and a coarse triangle may miss a trim.
        let mut candidates = std::iter::once(picked).chain((0..faces.len()).filter(|j| *j != picked));
        let (n, position) = candidates
            .find_map(|index| {
                let s = crate::body::evaluation::Surface(faces[index].oriented_surface());
                let (mut u, mut v) = s.search_nearest_parameter(p3(p), None, 100)?;
                for _ in 0..20 {
                    let q = s.subs(u, v) - p3(o);
                    let (su, sv) = (s.uder(u, v), s.vder(u, v));
                    let (aa, ab, ba, bb) = (su.dot(a), sv.dot(a), su.dot(b), sv.dot(b));
                    let det = aa * bb - ab * ba;
                    if !det.is_finite() || det.abs() < 1e-14 {
                        return None;
                    }
                    let du = (q.dot(a) * bb - q.dot(b) * ab) / det;
                    let dv = (q.dot(b) * aa - q.dot(a) * ba) / det;
                    u -= du;
                    v -= dv;
                    if du.abs() + dv.abs() < 1e-12 {
                        break;
                    }
                }
                if regions[index].is_none() && (-1e-8..=1.0 + 1e-8).contains(&u) && (-1e-8..=1.0 + 1e-8).contains(&v) {
                    u = u.clamp(0.0, 1.0);
                    v = v.clamp(0.0, 1.0);
                }
                let q = s.subs(u, v) - p3(o);
                (q.dot(a).abs() < 1e-6
                    && q.dot(b).abs() < 1e-6
                    && (0.0..max_distance).contains(&(from_p3(s.subs(u, v)) - o).dot(direction))
                    && regions[index]
                        .as_ref()
                        .map_or((0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v), |region| region.contains(s.subs(u, v), 1e-4)))
                .then(|| (s.normal(u, v), from_p3(s.subs(u, v))))
            })
            .unwrap_or_else(|| panic!("ray {direction:?}, z {z}: no exact hit inside the corner"));
        assert!((n.magnitude() - 1.0).abs() < 1e-9);
        (n, position)
    };
    let radial = (-Vec3::X + Vec3::Y * if front { -1.0 } else { 1.0 }).normalized().unwrap();
    let mut failures = Vec::new();
    // Nine paths span the corner, including its shoulders. Sample cell centers
    // to avoid exactly coplanar ray/edge hits at rational construction heights.
    // Leave the original sharp floor vertex and rim apex out of this regular
    // surface test; their direction-dependent limits have separate coverage.
    for lane in -4..=4 {
        let angle = lane as f64 * 0.15;
        let direction = radial * angle.cos() + Vec3::Z.cross(radial) * angle.sin();
        let radial_normal = (direction - axis * direction.dot(axis)).normalized().unwrap();
        let radial_normal = mt::Vector3::new(radial_normal.x, radial_normal.y, radial_normal.z);
        let corner_axis = mt::Vector3::new(axis.x, axis.y, axis.z);
        let mut lower = Vec::new();
        let mut upper = Vec::new();
        let mut previous: Option<(mt::Vector3, Vec3)> = None;
        for i in (if rounded_floor { 0 } else { 232 })..470 {
            let z = 4.2125 + i as f64 * 0.025;
            let (n, p) = normal_at(z, direction);
            if let Some((previous, old)) = previous {
                let turn = previous.dot(n).clamp(-1.0, 1.0).acos();
                if front {
                    // The steep front corner crosses multiple sloping floor
                    // panels. Its normal need not turn monotonically in Z:
                    // bound turning per physical distance instead.
                    let radius = if z < 10.0 { 1.5 } else { 0.3 };
                    assert!(
                        turn <= p.dist(old) * 4.0 / radius + 1e-5,
                        "front normal curvature at lane {lane}, z {z}: {} radians/mm",
                        turn / p.dist(old)
                    );
                } else {
                    assert!(turn < 10.0_f64.to_radians(), "abrupt normal turn at lane {lane}, z {z}: {} degrees", turn.to_degrees());
                }
            }
            previous = Some((n, p));
            // Project into this radial section: asin(n.axis) would confuse
            // legitimate azimuthal turning with reversal of the axial profile.
            assert!(-n.dot(radial_normal) > 1e-3, "corner must face the cavity");
            let tilt = n.dot(corner_axis).atan2(-n.dot(radial_normal));
            if z < 10.0 {
                lower.push(tilt);
            } else {
                upper.push(tilt);
            }
        }
        for (part, values) in [("floor", lower), ("rim", upper)] {
            if values.is_empty() || front {
                continue;
            }
            // A smooth approach to the axial cylinder normal should not turn
            // back on itself. Allow one degree of total numerical/fit reversal,
            // substantially below the old broad bands (over twenty degrees).
            let travel: f64 = values.windows(2).map(|n| (n[1] - n[0]).abs()).sum();
            let net = (values.last().unwrap() - values[0]).abs();
            let reversal = (travel - net).to_degrees();
            if reversal > 1.0 {
                failures.push(format!("{part}, lane {lane}: normal reverses {reversal} degrees"));
            }
        }
    }
    // A transverse section may have one broad peak at the corner. Extra
    // normal travel beyond that peak reveals small alternating rim ripples.
    let corner_axis = mt::Vector3::new(axis.x, axis.y, axis.z);
    for &z in if front { &[][..] } else { &[15.6, 15.7, 15.8, 15.9, 15.95][..] } {
        let values: Vec<_> = (-32..=32)
            .map(|lane| {
                let angle = lane as f64 * 0.0225;
                let direction = radial * angle.cos() + Vec3::Z.cross(radial) * angle.sin();
                normal_at(z, direction).0.dot(corner_axis).clamp(-1.0, 1.0).asin()
            })
            .collect();
        let travel: f64 = values.windows(2).map(|n| (n[1] - n[0]).abs()).sum();
        let min = values.iter().copied().fold(f64::INFINITY, f64::min);
        let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let ends = values[0] + values.last().unwrap();
        let excess = (travel - (2.0 * max - ends).max(ends - 2.0 * min)).to_degrees();
        if excess > 1.0 {
            failures.push(format!("transverse rim at z {z}: extra normal travel {excess} degrees"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("; "));
}

#[test]
fn sloped_corner_display_normals_follow_the_surface() {
    check_display_normals(&sloped_basin_with_fillets(), 20_000);
}

#[test]
fn full_size_placed_corner_display_normals_follow_the_surface() {
    let scale = 10.0;
    let offset = Vec3::new(0.0, -480.0, 690.0);
    let mut body = placed_basin_with_fillets(scale, offset, false);
    let inset = 12.0 * 10.0_f64.to_radians().tan();
    for edge in [
        Vec3::new(15.0 + inset * 0.5, (1.8 + 35.0 - inset) * 0.5, 10.0),
        Vec3::new(75.0 - inset * 0.5, 45.0 - inset * 0.5, 10.0),
        Vec3::new(75.0 - inset * 0.5, (1.8 + 35.0 - inset) * 0.5, 10.0),
    ] {
        body = crate::fillet(&body, &[edge * scale + offset], 1.5 * scale).unwrap();
    }
    check_display_normals(&body, 100_000);
}

fn check_display_normals(body: &Body, budget: usize) {
    let mesh = body.display_mesh_angular(body.size() * 0.001, crate::DISPLAY_ANGLE).unwrap();
    let surfaces: Vec<_> = body
        .solid
        .face_iter()
        .map(|face| match face.oriented_surface() {
            s @ mt::Surface::BSplineSurface(_) => Some(crate::body::evaluation::Surface(s)),
            _ => None,
        })
        .collect();
    let corners: Vec<_> =
        surfaces.iter().map(|s| s.as_ref().map(|s| [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)].map(|(u, v)| s.subs(u, v)))).collect();
    let mut worst = (0.0_f64, Vec3::ZERO);
    let mut checked = 0;
    for (tri, fi) in mesh.triangles.iter().zip(&mesh.tri_face) {
        let Some(surface) = &surfaces[*fi as usize] else { continue };
        let [a, b, c] = tri.map(|i| mesh.positions[i as usize]);
        // Physical refinement has a minimum edge length. Microscopic chord
        // slivers can span a large parameter interval near a collapsed chart;
        // their normals have no visible footprint. Keep the one-degree bound
        // on every triangle above the shared meshing floor: the requested
        // chord tolerance / 32, followed by the minimum-edge factor of 0.1.
        if a.dist(b).max(b.dist(c)).max(c.dist(a)) < body.size() * 0.001 / 32.0 * 0.1 {
            continue;
        }
        for weights in [[1.0 / 3.0; 3], [0.8, 0.1, 0.1], [0.1, 0.8, 0.1], [0.1, 0.1, 0.8]] {
            let p = tri.iter().zip(weights).map(|(i, w)| mesh.positions[*i as usize] * w).fold(Vec3::ZERO, |a, b| a + b);
            // At an original sharp vertex the limiting normal depends on the
            // approach direction. Outside its subpixel neighborhood, check
            // near each triangle corner too: centroids miss narrow seam ripples.
            if corners[*fi as usize].as_ref().unwrap().iter().any(|q| (*q - p3(p)).magnitude() < body.size() * 1e-4) {
                continue;
            }
            let normal = tri.iter().zip(weights).map(|(i, w)| mesh.normals[*i as usize] * w).fold(Vec3::ZERO, |a, b| a + b).normalized().unwrap();
            let (u, v) = surface.search_nearest_parameter(p3(p), None, 100).unwrap();
            let exact = surface.normal(u, v);
            let angle = normal.dot(Vec3::new(exact.x, exact.y, exact.z)).clamp(-1.0, 1.0).acos().to_degrees();
            assert!(angle.is_finite());
            if angle > worst.0 {
                worst = (angle, p);
            }
            checked += 1;
        }
    }
    assert!(checked > 500, "must cover the full transition meshes");
    eprintln!("display: {} triangles, worst normal error {} degrees", mesh.triangles.len(), worst.0);
    assert!(worst.0 < 1.0, "display normal error {} degrees at {:?}", worst.0, worst.1);
    assert!(mesh.triangles.len() < budget, "small blends must not inflate the whole body's display mesh: {} triangles", mesh.triangles.len());
}

#[test]
fn sloped_front_corner_flow_stays_smooth_from_floor_to_rim() {
    check_flow(&placed_basin_with_fillets(1.0, Vec3::ZERO, true), true, true);
}

#[test]
fn sloped_front_floor_shoulder_has_no_repeated_normal_ripples() {
    let body = placed_basin_with_fillets(1.0, Vec3::ZERO, true);
    let mut checked = 0;
    let mut worst = 0.0_f64;
    for face in body.solid.face_iter() {
        let mt::Surface::BSplineSurface(s) = face.oriented_surface() else { continue };
        if s.degrees() != (5, 5) {
            continue;
        }
        let surface = crate::body::evaluation::Surface(mt::Surface::BSplineSurface(s));
        if surface.subs(0.5, 0.5).z > 8.0 {
            continue;
        }
        // The end boundary on the front floor cylinder has no X component
        // in its normal. Follow strips just inside that boundary, where broad
        // axial corner paths miss alternating fitted-surface ripples.
        for side in 0..4 {
            let uv = |t: f64, d: f64| match side {
                0 => (t, d),
                1 => (d, t),
                2 => (t, 1.0 - d),
                _ => (1.0 - d, t),
            };
            if !(1..10).all(|i| {
                let (u, v) = uv(i as f64 / 10.0, 0.0);
                surface.normal(u, v).x.abs() < 1e-3
            }) {
                continue;
            }
            for d in [0.1, 0.05, 0.02, 0.01] {
                let values: Vec<_> = (0..=400)
                    .map(|i| {
                        let (u, v) = uv(0.1 + 0.8 * i as f64 / 400.0, d);
                        surface.normal(u, v).x.clamp(-1.0, 1.0).asin()
                    })
                    .collect();
                let travel: f64 = values.windows(2).map(|p| (p[1] - p[0]).abs()).sum();
                let min = values.iter().copied().fold(f64::INFINITY, f64::min);
                let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                let ends = values[0] + values.last().unwrap();
                let excess = (travel - (2.0 * max - ends).max(ends - 2.0 * min)).to_degrees();
                worst = worst.max(excess);
                checked += 1;
            }
        }
    }
    assert!(checked >= 4, "must sample the shoulder beside the floor join");
    // Extra travel counts each reversal twice. Limit total reversals to one
    // degree; the former corrugated shoulder accumulated about 26 degrees.
    assert!(worst < 2.0, "front floor shoulder has {worst} degrees of repeated normal turning");
}
