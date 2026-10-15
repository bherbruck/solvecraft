//! Bounded corner fills with a fixed projected map and directional tangent limits.
use super::*;
use mt::{BoundedSurface, Invertible};

pub(crate) fn fit(
    boundaries: &[Boundary; 4],
    singular: [bool; 2],
    out: Vec3,
    size: f64,
    planes: &[(Vec3, Vec3)],
    density: usize,
) -> Result<mt::Surface> {
    if !size.is_finite()
        || size <= 0.0
        || !out.is_finite()
        || planes.len() > 32
        || planes.iter().any(|(n, p)| !n.is_finite() || !p.is_finite() || (n.len() - 1.0).abs() > 1e-6)
    {
        return Err(unsupported("invalid parametric patch input"));
    }
    if ![20, 32, 40, 56].contains(&density) {
        return Err(unsupported("patch density budget"));
    }
    let guide_weight = if singular.iter().any(|s| *s) { 0.01 } else { 1e-4 };
    let z = out.normalized().ok_or_else(|| unsupported("patch direction"))?;
    let x = z.any_perp();
    let y = z.cross(x);
    let axial = match boundaries[0].normal {
        Normal::Cylinder { axis, .. } | Normal::InwardCylinder { axis, .. } => Some((axis.dot(x), axis.dot(y))),
        _ => None,
    };
    let base = |side: usize, t: f64| -> Result<(Vec3, Vec3)> {
        let b = boundaries.get(side).ok_or_else(|| unsupported("boundary"))?;
        let t = if side == 0 || side == 3 { 1.0 - t } else { t };
        let (lo, hi) = b.curve.range_tuple();
        b.sample(lo + (hi - lo) * t)
    };
    let corners = [base(0, 0.0)?.0, base(0, 1.0)?.0, base(2, 0.0)?.0, base(2, 1.0)?.0];
    let which = if singular[0] { 0 } else { 1 };
    let origin = corners[which + 2];
    // Radial cubic blow-up: one corner has zero first/second derivatives,
    // while the other two incident faces retain distinct limiting normals.
    let map_corner = |which: usize, u: f64, v: f64| {
        let a = if which == 0 { u } else { 1.0 - u };
        let b = 1.0 - v;
        let s = a * (a * a + (1.0 - a * a) * b * b);
        let t = b * (b * b + (1.0 - b * b) * a * a);
        (if which == 0 { s } else { 1.0 - s }, 1.0 - t)
    };
    let map = |mut u: f64, mut v: f64| {
        for (corner, active) in singular.iter().enumerate() {
            if *active {
                (u, v) = map_corner(corner, u, v);
            }
        }
        (u, v)
    };
    let guide = |u: f64, v: f64| -> Result<Vec3> {
        let (u, v) = map(u, v);
        Ok(base(0, u)?.0 * (1.0 - v) + base(2, u)?.0 * v + base(1, v)?.0 * (1.0 - u) + base(3, v)?.0 * u
            - corners[0] * ((1.0 - u) * (1.0 - v))
            - corners[1] * (u * (1.0 - v))
            - corners[2] * ((1.0 - u) * v)
            - corners[3] * (u * v)
            - origin)
    };
    let sample = |side: usize, t: f64| -> Result<(Vec3, Vec3)> {
        let (u, v) = match side {
            0 => (t, 0.0),
            1 => (0.0, t),
            2 => (t, 1.0),
            _ => (1.0, t),
        };
        let (u, v) = map(u, v);
        base(side, if side == 0 || side == 2 { u } else { v })
    };
    let base_der = |side: usize, t: f64| -> Result<Vec3> {
        let b = boundaries.get(side).ok_or_else(|| unsupported("boundary"))?;
        let reversed = side == 0 || side == 3;
        let (lo, hi) = b.curve.range_tuple();
        let d = b.curve.der(lo + (hi - lo) * if reversed { 1.0 - t } else { t });
        Ok(Vec3::new(d.x, d.y, d.z) * ((hi - lo) * if reversed { -1.0 } else { 1.0 }))
    };
    let mut tips = Vec::new();
    for (which, active) in singular.iter().enumerate() {
        if !*active {
            continue;
        }
        let a = base_der(2, which as f64)? * if which == 0 { 1.0 } else { -1.0 };
        let d = -base_der(if which == 0 { 1 } else { 3 }, 1.0)?;
        let end_normal = sample(2, which as f64)?.1;
        let seam_normal = sample(if which == 0 { 1 } else { 3 }, 1.0)?.1;
        // Composition of the two radial maps changes the cubic jets but
        // leaves either vertical boundary's cubic parameterization unchanged.
        let scales = if singular == [true, true] { if which == 0 { [3.0, 1.0, 3.0, 1.0] } else { [27.0, 9.0, 3.0, 1.0] } } else { [1.0; 4] };
        tips.push((which, corners[which + 2] - origin, a, d, end_normal, seam_normal, scales));
    }
    let tol = (size * 1e-6).max(1e-6);
    let n = density;
    {
        let mut knots = vec![0.0; 6];
        knots.extend((1..n - 5).map(|i| i as f64 / (n - 5) as f64));
        knots.extend([1.0; 6]);
        let h = 1.0 / (n - 5) as f64;
        // At a clamped quintic endpoint, uniform B-spline controls for u³
        // and u²v are respectively 3h³/5 and h³/25 times their Taylor terms.
        let pure_third = 3.0 * h.powi(3) / 5.0;
        let mixed_third = h.powi(3) / 25.0;
        let mut jets = std::collections::HashMap::new();
        for (which, corner, a, d, _, _, scales) in &tips {
            for i in 0..3 {
                for j in 0..3 - i {
                    jets.insert((if *which == 0 { i } else { n - 1 - i }) * n + n - 1 - j, *corner);
                }
            }
            for (i, j, delta) in [
                (3, 0, *a * (scales[0] * pure_third)),
                (2, 1, *d * (scales[1] * mixed_third)),
                (1, 2, *a * (scales[2] * mixed_third)),
                (0, 3, *d * (scales[3] * pure_third)),
            ] {
                jets.insert((if *which == 0 { i } else { n - 1 - i }) * n + n - 1 - j, *corner + delta);
            }
        }
        let solve_fixed = |mut rows: Vec<Row>, fixed: &std::collections::HashMap<usize, f64>| -> Result<Vec<f64>> {
            for row in &mut rows {
                let mut rhs = row.rhs;
                row.entries.retain(|(i, w)| {
                    if let Some(value) = fixed.get(i) {
                        rhs -= w * value;
                        false
                    } else {
                        true
                    }
                });
                row.rhs = rhs;
            }
            for (i, value) in fixed {
                rows.push(Row { entries: vec![(*i, 1.0)], rhs: *value });
            }
            solve_qr(&rows, n * n)
        };
        let mut rx = Vec::new();
        let mut ry = Vec::new();
        let mut rz = Vec::new();
        for i in 0..n {
            for j in 0..n {
                let u = knots.iter().skip(i + 1).take(5).sum::<f64>() / 5.0;
                let v = knots.iter().skip(j + 1).take(5).sum::<f64>() / 5.0;
                let (bu, _, _) = basis(&knots, u, n)?;
                let (bv, _, _) = basis(&knots, v, n)?;
                let g = guide(u, v)?;
                rx.push(tensor(&bu, &bv, 1.0, g.dot(x)));
                ry.push(tensor(&bu, &bv, 1.0, g.dot(y)));
                rz.push(tensor(&bu, &bv, 1.0, g.dot(z)));
            }
        }
        let cx = solve_fixed(rx, &jets.iter().map(|(i, p)| (*i, p.dot(x))).collect())?;
        let cy = solve_fixed(ry, &jets.iter().map(|(i, p)| (*i, p.dot(y))).collect())?;
        let cz = solve_fixed(rz, &jets.iter().map(|(i, p)| (*i, p.dot(z))).collect())?;
        let projection: Vec<_> = cx.iter().zip(&cy).map(|(a, b)| x * *a + y * *b).collect();
        let project = |bu: &[f64], bv: &[f64]| project_tensor(&projection, n, bu, bv);
        let mut row_sum = RowSum::new(n * n);
        let mut rows = Vec::new();
        for side in 0..4 {
            for i in 0..=1024 {
                let t = i as f64 / 1024.0;
                let (u, v) = match side {
                    0 => (t, 0.0),
                    1 => (0.0, t),
                    2 => (t, 1.0),
                    _ => (1.0, t),
                };
                let (p, normal) = sample(side, t)?;
                let (bu, du, ddu) = basis(&knots, u, n)?;
                let (bv, dv, ddv) = basis(&knots, v, n)?;
                // Boundary controls remain fixed during the height fit. Reject
                // an insufficient boundary grid before assembling and solving
                // its much larger curvature system; no interior solve can
                // repair this positional error.
                let fixed_height = tensor(&bu, &bv, 1.0, 0.0).entries.iter().map(|(i, w)| cz[*i] * *w).sum::<f64>();
                if (origin + project(&bu, &bv) + z * fixed_height).dist(p) >= tol {
                    return Err(unsupported("parametric boundary grid is too coarse"));
                }
                rows.push(tensor(&bu, &bv, 10000.0, (p - origin).dot(z)));
                if normal != Vec3::ZERO {
                    if !normal.dot(z).is_finite() || normal.dot(z) < 1e-3 {
                        return Err(unsupported("boundary normal folds behind projection"));
                    }
                    let proximity = tips
                        .iter()
                        .map(|(corner, _, _, _, _, _, _)| ((u - *corner as f64).powi(2) + (1.0 - v).powi(2)).sqrt())
                        .fold(1.0_f64, f64::min)
                        .max(0.01);
                    let w = 10000.0 / proximity;
                    rows.push(tensor(&du, &bv, w * normal.dot(z), -project(&du, &bv).dot(normal) / normal.dot(z)));
                    rows.push(tensor(&bu, &dv, w * normal.dot(z), -project(&bu, &dv).dot(normal) / normal.dot(z)));
                }
                // Match the barrel's zero axial curvature softly. The physical
                // Hessian includes derivatives of the non-affine projected map;
                // length-squared scaling keeps this condition invariant to units.
                if side == 0
                    && i > 0
                    && i < 1024
                    && let Some((ax, ay)) = axial
                {
                    let d = [project(&du, &bv), project(&bu, &dv), project(&ddu, &bv), project(&du, &dv), project(&bu, &ddv)]
                        .map(|p| [p.dot(x), p.dot(y)]);
                    let (_, h) = physical_curvature_weights(d).ok_or_else(|| unsupported("singular section map"))?;
                    let weights: [f64; 5] = std::array::from_fn(|k| ax * ax * h[0][k] + 2.0 * ax * ay * h[1][k] + ay * ay * h[2][k]);
                    for ((aa, bb), c) in [(&ddu, &bv), (&du, &dv), (&bu, &ddv), (&du, &bv), (&bu, &dv)].into_iter().zip(weights) {
                        row_sum.add_product(aa, bb, c * size * (corners[1] - corners[0]).len());
                    }
                    rows.push(row_sum.finish());
                }
            }
        }
        // Pull physical guide points back through the fixed projected map.
        // Bounded Newton steps skip points outside this fan sector; they never
        // extrapolate a chart or relax its boundary/certificate checks.
        for p in if singular.iter().any(|s| *s) { radial_guide(boundaries)? } else { Vec::new() } {
            let target = p - origin;
            let (mut u, mut v) = (0.5, 0.5);
            for _ in 0..40 {
                let (bu, du, _) = basis(&knots, u, n)?;
                let (bv, dv, _) = basis(&knots, v, n)?;
                let q = project(&bu, &bv) - target;
                let (pu, pv) = (project(&du, &bv), project(&bu, &dv));
                let (aa, ab, ba, bb) = (pu.dot(x), pv.dot(x), pu.dot(y), pv.dot(y));
                let det = aa * bb - ab * ba;
                if !det.is_finite() || det.abs() < 1e-20 {
                    break;
                }
                let a = (q.dot(x) * bb - q.dot(y) * ab) / det;
                let b = (q.dot(y) * aa - q.dot(x) * ba) / det;
                if !a.is_finite() || !b.is_finite() {
                    break;
                }
                u = (u - a.clamp(-0.15, 0.15)).clamp(1e-8, 1.0 - 1e-8);
                v = (v - b.clamp(-0.15, 0.15)).clamp(1e-8, 1.0 - 1e-8);
                if a.abs() + b.abs() < 1e-10 {
                    break;
                }
            }
            let (bu, _, _) = basis(&knots, u, n)?;
            let (bv, _, _) = basis(&knots, v, n)?;
            let delta = project(&bu, &bv) - target;
            if delta.dot(x).hypot(delta.dot(y)) < size * 1e-8 {
                // Fade the positional guide at constrained boundaries, where
                // the neighboring face already determines position and tangent.
                let fade = (16.0 * u * (1.0 - u) * v * (1.0 - v)).powi(2);
                // These discrete radial samples are only a shape hint. A
                // strong positional fit makes a narrow shoulder oscillate
                // between the samples and its exact tangent boundary.
                rows.push(tensor(&bu, &bv, 0.01 * fade, target.dot(z)));
            }
        }
        // Collapsed corner charts need quadrature inside every knot span.
        // The regular-chart grid misses the ends of their clamped spans,
        // allowing a narrow ridge beside an otherwise tangent boundary. Use
        // four-point Gauss integration within spans and eight at the ends,
        // where the collapsed physical map needs closer sampling.
        let quadrature: Vec<_> = if tips.is_empty() {
            (1..2 * n).map(|i| (i as f64 / (2 * n) as f64, 1.0 / (2 * n) as f64)).collect()
        } else {
            (0..n - 5)
                .flat_map(|span| {
                    let samples: &[(f64, f64)] = if span == 0 || span == n - 6 {
                        &[
                            (-0.9602898564975363, 0.1012285362903763),
                            (-0.7966664774136267, 0.2223810344533745),
                            (-0.525_532_409_916_329, 0.3137066458778873),
                            (-0.1834346424956498, 0.362_683_783_378_362),
                            (0.1834346424956498, 0.362_683_783_378_362),
                            (0.525_532_409_916_329, 0.3137066458778873),
                            (0.7966664774136267, 0.2223810344533745),
                            (0.9602898564975363, 0.1012285362903763),
                        ]
                    } else {
                        &[
                            (-0.8611363115940526, 0.3478548451374538),
                            (-0.3399810435848563, 0.6521451548625461),
                            (0.3399810435848563, 0.6521451548625461),
                            (0.8611363115940526, 0.3478548451374538),
                        ]
                    };
                    samples.iter().map(move |&(t, w)| ((span as f64 + (t + 1.0) * 0.5) * h, w * h * 0.5))
                })
                .collect()
        };
        let quadrature = quadrature.into_iter().map(|(t, w)| Ok((basis(&knots, t, n)?, w))).collect::<Result<Vec<_>>>()?;
        for ((bu, du, ddu), wu) in &quadrature {
            for ((bv, dv, ddv), wv) in &quadrature {
                let derivatives =
                    [project(du, bv), project(bu, dv), project(ddu, bv), project(du, dv), project(bu, ddv)].map(|p| [p.dot(x), p.dot(y)]);
                let (area, weights) = physical_curvature_weights(derivatives).ok_or_else(|| unsupported("singular smoothing map"))?;
                let w = (if singular.iter().any(|s| *s) { 1e-4 } else { 1e-3 }) * size * (area * wu * wv).sqrt();
                if !singular.iter().any(|s| *s)
                    && let Some((ax, ay)) = axial
                {
                    let directional: [f64; 5] =
                        std::array::from_fn(|k| ax * ax * weights[0][k] + 2.0 * ax * ay * weights[1][k] + ay * ay * weights[2][k]);
                    for ((aa, bb), c) in [(ddu, bv), (du, dv), (bu, ddv), (du, bv), (bu, dv)].into_iter().zip(directional) {
                        row_sum.add_product(aa, bb, c * w * 3.0);
                    }
                    rows.push(row_sum.finish());
                }
                // The radial chart collapses at the tip. Penalize curvature in
                // physical projected coordinates, not derivatives of that chart.
                for ([a, b, c, cu, cv], factor) in weights.into_iter().zip([1.0, 2.0_f64.sqrt(), 1.0]) {
                    for (aa, bb, coefficient) in [(ddu, bv, a), (du, dv, b), (bu, ddv, c), (du, bv, cu), (bu, dv, cv)] {
                        row_sum.add_product(aa, bb, coefficient * w * factor);
                    }
                    rows.push(row_sum.finish());
                }
            }
        }
        for i in 0..n {
            for j in 0..n {
                // Regularize toward the fitted guide controls, not its point
                // values at Greville parameters (which are not spline controls).
                // This anchors weak modes without importing control-polygon bias.
                rows.push(Row { entries: vec![(i * n + j, guide_weight)], rhs: cz[i * n + j] * guide_weight });
            }
        }
        let mut fixed: std::collections::HashMap<_, _> = jets.iter().map(|(i, p)| (*i, p.dot(z))).collect();
        // Both sides of a shared seam use exactly the same boundary spline.
        // Tangency constraints affect only the interior control rows.
        for i in 0..n {
            for j in 0..n {
                if i == 0 || i + 1 == n || j == 0 || j + 1 == n {
                    fixed.insert(i * n + j, cz[i * n + j]);
                }
            }
        }
        for (which, corner, _, _, end_normal, seam_normal, _) in &tips {
            for (i, j, normal) in [(2, 1, *end_normal), (1, 2, *seam_normal)] {
                let index = (if *which == 0 { i } else { n - 1 - i }) * n + n - 1 - j;
                let projected_corner = x * corner.dot(x) + y * corner.dot(y);
                fixed.insert(index, corner.dot(z) - (projection[index] - projected_corner).dot(normal) / normal.dot(z));
            }
        }
        let mut heights = solve_fixed(rows.clone(), &fixed)?;
        // Bounds corrections stay local to the already validated unconstrained
        // solution instead of exciting its weakly constrained interior modes.
        for (i, value) in heights.iter().enumerate() {
            rows.push(Row { entries: vec![(i, 100.0)], rhs: value * 100.0 });
        }
        for _ in 0..16 {
            let ctrl = (0..n).map(|i| (0..n).map(|j| p3(origin + projection[i * n + j] + z * heights[i * n + j])).collect()).collect();
            let patch = mt::BSplineSurface::try_new((mt::KnotVec::from(knots.clone()), mt::KnotVec::from(knots.clone())), ctrl)
                .map_err(|e| KernelError::Failed(format!("radial patch: {e}")))?;
            let surface = crate::body::evaluation::Surface(mt::Surface::BSplineSurface(patch.clone()));
            let mut distance = 0.0_f64;
            let mut angle = 0.0_f64;
            for side in 0..4 {
                for i in 0..=1025 {
                    let t = i as f64 / 1025.0;
                    let (u, v) = match side {
                        0 => (t, 0.0),
                        1 => (0.0, t),
                        2 => (t, 1.0),
                        _ => (1.0, t),
                    };
                    let (p, normal) = sample(side, t)?;
                    let evaluated = from_p3(surface.subs(u, v));
                    if !evaluated.is_finite() {
                        return Err(unsupported("non-finite parametric fill"));
                    }
                    distance = distance.max(evaluated.dist(p));
                    if v == 1.0 && ((u == 0.0 && singular[0]) || (u == 1.0 && singular[1])) {
                        continue;
                    }
                    let nn = surface.normal(u, v);
                    let nn = Vec3::new(nn.x, nn.y, nn.z);
                    if !nn.is_finite() {
                        return Err(unsupported("singular radial fill"));
                    }
                    if normal != Vec3::ZERO {
                        angle = angle.max(nn.dot(normal).abs().clamp(-1.0, 1.0).acos().to_degrees());
                    }
                }
            }
            if distance < tol && angle < 0.01 {
                let nn = patch.normal(0.5, 0.5);
                let inverse = Vec3::new(nn.x, nn.y, nn.z).dot(z) < 0.0;
                let local = mt::BSplineSurface::try_new(
                    patch.knot_vecs().clone(),
                    (0..n).map(|i| (0..n).map(|j| mt::Point3::new(cx[i * n + j], cy[i * n + j], heights[i * n + j])).collect()).collect(),
                )
                .map_err(|e| KernelError::Failed(format!("local certificate: {e}")))?;
                let local_planes: Vec<_> = planes
                    .iter()
                    .map(|(n, p)| {
                        let p = *p - origin;
                        (Vec3::new(n.dot(x), n.dot(y), n.dot(z)), Vec3::new(p.dot(x), p.dot(y), p.dot(z)))
                    })
                    .collect();
                let mut violations = Vec::new();
                if let Err(error) = certify_with_correction(&local, if inverse { -Vec3::Z } else { Vec3::Z }, &local_planes, tol, &mut violations) {
                    if violations.is_empty() {
                        return Err(error);
                    }
                    let mut added = 0;
                    let grid: Vec<_> = (1..128).map(|i| basis(&knots, i as f64 / 128.0, n)).collect::<Result<_>>()?;
                    for (i, (bu, _, _)) in grid.iter().enumerate() {
                        for (j, (bv, _, _)) in grid.iter().enumerate() {
                            let p =
                                origin + project(bu, bv) + z * tensor(bu, bv, 1.0, 0.0).entries.iter().map(|(k, w)| heights[*k] * *w).sum::<f64>();
                            if planes.iter().any(|(normal, point)| (p - *point).dot(*normal) > tol * 0.25) {
                                violations.push(((i + 1) as f64 / 128.0, (j + 1) as f64 / 128.0));
                            }
                        }
                    }
                    for (u, v) in violations {
                        let (bu, _, _) = basis(&knots, u, n)?;
                        let (bv, _, _) = basis(&knots, v, n)?;
                        let base = origin + project(&bu, &bv);
                        let current = tensor(&bu, &bv, 1.0, 0.0).entries.iter().map(|(i, w)| heights[*i] * *w).sum::<f64>();
                        for (normal, point) in planes {
                            let den = normal.dot(z);
                            if den.abs() < 1e-8 {
                                continue;
                            }
                            let bound = -(base - *point).dot(*normal) / den;
                            if (current - bound) * den > 0.0 {
                                if rows.len() >= 100_000 {
                                    return Err(unsupported("parametric bounds correction budget"));
                                }
                                rows.push(tensor(&bu, &bv, 10000.0 * den.abs(), bound));
                                added += 1;
                            }
                        }
                    }
                    if added == 0 {
                        return Err(error);
                    }
                    heights = solve_fixed(rows.clone(), &fixed)?;
                    continue;
                }
                return Ok(if inverse { surface.0.inverse() } else { surface.0 });
            }
            break;
        }
    }
    Err(unsupported("cannot fit the radial transition"))
}

fn basis(knots: &[f64], t: f64, n: usize) -> Result<(Vec<f64>, Vec<f64>, Vec<f64>)> {
    let degree = 5;
    let mut values = vec![0.0; knots.len() - 1];
    for (i, span) in knots.windows(2).enumerate() {
        if ((span[0] <= t && t < span[1]) || (t == 1.0 && i + 1 == n))
            && let Some(x) = values.get_mut(i)
        {
            *x = 1.0;
        }
    }
    let mut low = Vec::new();
    let mut lower = Vec::new();
    for d in 1..=degree {
        let mut next = vec![0.0; values.len() - 1];
        for (i, v) in next.iter_mut().enumerate() {
            let a = knots[i + d] - knots[i];
            let b = knots[i + d + 1] - knots[i + 1];
            if a > 0.0 {
                *v += (t - knots[i]) / a * values[i];
            }
            if b > 0.0 {
                *v += (knots[i + d + 1] - t) / b * values[i + 1];
            }
        }
        values = next;
        if d == degree - 2 {
            lower = values.clone();
        }
        if d == degree - 1 {
            low = values.clone();
        }
    }
    let mut low_der = vec![0.0; n + 1];
    for (i, v) in low_der.iter_mut().enumerate() {
        let a = knots[i + degree - 1] - knots[i];
        let b = knots[i + degree] - knots[i + 1];
        if a > 0.0 {
            *v += (degree - 1) as f64 / a * lower[i];
        }
        if b > 0.0 {
            *v -= (degree - 1) as f64 / b * lower[i + 1];
        }
    }
    let mut der = vec![0.0; n];
    let mut der2 = vec![0.0; n];
    for i in 0..n {
        let a = knots[i + degree] - knots[i];
        let b = knots[i + degree + 1] - knots[i + 1];
        if a > 0.0 {
            der[i] += degree as f64 / a * low[i];
            der2[i] += degree as f64 / a * low_der[i];
        }
        if b > 0.0 {
            der[i] -= degree as f64 / b * low[i + 1];
            der2[i] -= degree as f64 / b * low_der[i + 1];
        }
    }
    Ok((values, der, der2))
}

/// Chain rule for the physical height Hessian. Input: projected u, v, uu, uv,
/// vv derivatives. Output rows multiply [height_uu, height_uv, height_vv,
/// height_u, height_v] to obtain height_xx, height_xy and height_yy.
fn physical_curvature_weights(d: [[f64; 2]; 5]) -> Option<(f64, [[f64; 5]; 3])> {
    if d.iter().flatten().any(|x| !x.is_finite()) {
        return None;
    }
    let [[xu, yu], [xv, yv], [xuu, yuu], [xuv, yuv], [xvv, yvv]] = d;
    let det = xu * yv - xv * yu;
    if !det.is_finite() || det.abs() <= 1e-14 * xu.hypot(yu) * xv.hypot(yv) {
        return None;
    }
    let (ux, vx, uy, vy) = (yv / det, -yu / det, -xv / det, xu / det);
    let weights = [[ux * ux, 2.0 * ux * vx, vx * vx], [ux * uy, ux * vy + vx * uy, vx * vy], [uy * uy, 2.0 * uy * vy, vy * vy]].map(|[a, b, c]| {
        let (cx, cy) = (a * xuu + b * xuv + c * xvv, a * yuu + b * yuv + c * yvv);
        [a, b, c, -ux * cx - uy * cy, -vx * cx - vy * cy]
    });
    weights.iter().flatten().all(|x| x.is_finite()).then_some((det.abs(), weights))
}

/// Positive Bernstein coefficients of the projected Jacobian certify that the
/// rectangular patch cannot fold; control hulls certify the material bounds.
#[cfg(test)]
fn certify(surface: &mt::BSplineSurface<mt::Point3>, out: Vec3, planes: &[(Vec3, Vec3)], tol: f64) -> Result<()> {
    certify_with_correction(surface, out, planes, tol, &mut Vec::new())
}
fn certify_with_correction(
    surface: &mt::BSplineSurface<mt::Point3>,
    out: Vec3,
    planes: &[(Vec3, Vec3)],
    tol: f64,
    violations: &mut Vec<(f64, f64)>,
) -> Result<()> {
    let cuts = |knots: &mt::KnotVec| {
        let mut v: Vec<_> = knots.iter().copied().filter(|t| *t > 0.0 && *t < 1.0).collect();
        v.dedup();
        v
    };
    let mut left = surface.clone();
    let mut strips = Vec::new();
    for u in cuts(surface.uknot_vec()) {
        let right = left.ucut(u);
        strips.push(left);
        left = right;
    }
    strips.push(left);
    let mut stack = Vec::new();
    for mut strip in strips {
        for v in cuts(surface.vknot_vec()) {
            let rest = strip.vcut(v);
            stack.push((strip, 0));
            strip = rest;
        }
        stack.push((strip, 0));
    }
    let choose = |n: usize, k: usize| (0..k.min(n - k)).fold(1.0, |a, j| a * (n - j) as f64 / (j + 1) as f64);
    let mut visited = 0;
    while let Some((mut patch, depth)) = stack.pop() {
        visited += 1;
        if visited > 32768 {
            return Err(unsupported("parametric certificate exceeds its budget"));
        }
        let (p, q) = patch.degrees();
        let c = patch.control_points();
        if c.len() != p + 1 || c.iter().any(|r| r.len() != q + 1) || p > 7 || q > 7 || p < 1 || q < 1 {
            return Err(unsupported("parametric certificate needs bounded Bezier cells"));
        }
        let bounded = c.iter().flatten().all(|point| {
            let point = from_p3(*point);
            point.is_finite() && planes.iter().all(|(n, p)| (point - *p).dot(*n) <= tol)
        });
        let mut coefficients = vec![0.0; 4 * p * q];
        for i in 0..p {
            for j in 0..=q {
                let du = from_p3(c[i + 1][j]) - from_p3(c[i][j]);
                for a in 0..=p {
                    for b in 0..q {
                        let dv = from_p3(c[a][b + 1]) - from_p3(c[a][b]);
                        let weight =
                            choose(p - 1, i) * choose(p, a) / choose(2 * p - 1, i + a) * choose(q, j) * choose(q - 1, b) / choose(2 * q - 1, j + b);
                        coefficients[(i + a) * (2 * q) + j + b] += du.cross(dv).dot(out) * weight;
                    }
                }
            }
        }
        let magnitude = coefficients.iter().copied().map(f64::abs).fold(0.0_f64, f64::max);
        if bounded && magnitude > 0.0 && coefficients.iter().all(|c| c.is_finite() && *c >= -magnitude * 1e-12) {
            continue;
        }
        let (ur, vr) = patch.range_tuple();
        let u = (ur.0 + ur.1) * 0.5;
        let v = (vr.0 + vr.1) * 0.5;
        let point = from_p3(patch.subs(u, v));
        let jac = patch.uder(u, v).cross(patch.vder(u, v));
        if !point.is_finite() || planes.iter().any(|(n, p)| (point - *p).dot(*n) > tol) || Vec3::new(jac.x, jac.y, jac.z).dot(out) < 0.0 {
            if point.is_finite() && Vec3::new(jac.x, jac.y, jac.z).dot(out) >= 0.0 {
                violations.push((u, v));
            }
            return Err(unsupported("parametric transition protrudes or folds"));
        }
        if depth >= 16 {
            if !bounded {
                let mut worst = f64::NEG_INFINITY;
                let mut where_worst = (u, v);
                for (i, row) in c.iter().enumerate() {
                    for (j, point) in row.iter().enumerate() {
                        for (normal, base) in planes {
                            let distance = (from_p3(*point) - *base).dot(*normal);
                            if distance > worst {
                                worst = distance;
                                where_worst = (ur.0 + (ur.1 - ur.0) * i as f64 / p as f64, vr.0 + (vr.1 - vr.0) * j as f64 / q as f64);
                            }
                        }
                    }
                }
                violations.push(where_worst);
            }
            return Err(unsupported("cannot certify the singular corner transition"));
        }
        if depth % 2 == 0 {
            let next = patch.ucut(u);
            stack.push((patch, depth + 1));
            stack.push((next, depth + 1));
        } else {
            let next = patch.vcut(v);
            stack.push((patch, depth + 1));
            stack.push((next, depth + 1));
        }
    }
    Ok(())
}

/// A quintic basis has at most six active entries. Trim its zero tails once,
/// rather than scanning the entire second basis for every active first entry.
/// Keep the original threshold and iteration order, including interior zeros.
fn basis_support(values: &[f64]) -> (usize, &[f64]) {
    let Some(first) = values.iter().position(|x| x.abs() > 1e-14) else { return (0, &[]) };
    let end = values.iter().rposition(|x| x.abs() > 1e-14).unwrap_or(first) + 1;
    (first, &values[first..end])
}

fn project_tensor(projection: &[Vec3], stride: usize, bu: &[f64], bv: &[f64]) -> Vec3 {
    let (iu, bu) = basis_support(bu);
    let (iv, bv) = basis_support(bv);
    let mut p = Vec3::ZERO;
    for (i, a) in bu.iter().enumerate().filter(|(_, a)| a.abs() > 1e-14) {
        let row = &projection[(iu + i) * stride + iv..][..bv.len()];
        for (point, b) in row.iter().zip(bv).filter(|(_, b)| b.abs() > 1e-14) {
            p += *point * (a * b);
        }
    }
    p
}

/// Sum sparse tensor rows without allocating a tree node per coefficient. Preserve
/// both the summation order and explicit zeros, so the QR system is unchanged.
struct RowSum {
    values: Vec<f64>,
    present: Vec<bool>,
    touched: Vec<usize>,
}
impl RowSum {
    fn new(count: usize) -> Self {
        Self { values: vec![0.0; count], present: vec![false; count], touched: Vec::with_capacity(36) }
    }

    fn add_product(&mut self, a: &[f64], b: &[f64], weight: f64) {
        let stride = b.len();
        let (ia, a) = basis_support(a);
        let (ib, b) = basis_support(b);
        for (i, x) in a.iter().enumerate().filter(|(_, x)| x.abs() > 1e-14) {
            for (j, y) in b.iter().enumerate().filter(|(_, y)| y.abs() > 1e-14) {
                let index = (ia + i) * stride + ib + j;
                if !self.present[index] {
                    self.present[index] = true;
                    self.touched.push(index);
                }
                self.values[index] += x * y * weight;
            }
        }
    }

    fn finish(&mut self) -> Row {
        self.touched.sort_unstable();
        let entries = self
            .touched
            .drain(..)
            .map(|i| {
                let value = self.values[i];
                self.values[i] = 0.0;
                self.present[i] = false;
                (i, value)
            })
            .collect();
        Row { entries, rhs: 0.0 }
    }
}

/// Banded QR avoids squaring the condition number of vanishing endpoint jets.
fn solve_qr(rows: &[Row], count: usize) -> Result<Vec<f64>> {
    let width = rows.iter().filter_map(|r| Some(r.entries.last()?.0 - r.entries.first()?.0)).max().unwrap_or(0);
    if count > 8192 || width > 320 || rows.len() > 200000 {
        return Err(unsupported("parametric fit budget"));
    }
    let stride = width + 1;
    let mut scales = vec![0.0_f64; count];
    for r in rows {
        for (i, w) in &r.entries {
            scales[*i] += w * w;
        }
    }
    for s in &mut scales {
        *s = s.sqrt();
        if !s.is_finite() || *s <= 0.0 {
            return Err(unsupported("parametric fit scale"));
        }
    }
    let mut order: Vec<_> = rows.iter().filter(|r| !r.entries.is_empty()).collect();
    order.sort_by_key(|r| r.entries.last().map(|e| e.0));
    let mut matrix = vec![0.0_f64; count * stride];
    let mut rhs = vec![0.0; count];
    let mut work = vec![0.0; count];
    for row in order {
        let first = row.entries.first().ok_or_else(|| unsupported("empty QR row"))?.0;
        let last = row.entries.last().ok_or_else(|| unsupported("empty QR row"))?.0;
        work[first..=last].fill(0.0);
        for (i, w) in &row.entries {
            work[*i] = w / scales[*i];
        }
        let mut value = row.rhs;
        for i in first..=last {
            let a = matrix[i * stride];
            let b = work[i];
            if b == 0.0 {
                continue;
            }
            let radius = a.hypot(b);
            let c = a / radius;
            let s = b / radius;
            // Work on matching contiguous slices. Indexing the band through
            // an enumerated full work vector retained a bounds check on every
            // coefficient in this (billions-of-updates) fitting hot loop.
            let end = last.min(i + width) + 1;
            let band = &mut matrix[i * stride..i * stride + end - i];
            for (slot, work) in band.iter_mut().zip(&mut work[i..end]) {
                let (a, b) = (*slot, *work);
                *slot = c * a + s * b;
                *work = -s * a + c * b;
            }
            let a = rhs[i];
            rhs[i] = c * a + s * value;
            value = -s * a + c * value;
        }
    }
    let mut values = vec![0.0; count];
    for i in (0..count).rev() {
        let mut v = rhs[i];
        for j in i + 1..count.min(i + width + 1) {
            v -= matrix[i * stride + j - i] * values[j];
        }
        let d = matrix[i * stride];
        if !d.is_finite() || d <= 0.0 {
            return Err(unsupported("singular parametric QR"));
        }
        values[i] = v / d;
    }
    for (v, s) in values.iter_mut().zip(scales) {
        *v /= s;
    }
    if values.iter().any(|v| !v.is_finite()) {
        return Err(unsupported("invalid parametric QR result"));
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_support_preserves_projection_and_constraint_coefficients() {
        for n in [20, 32, 40, 56] {
            let mut knots = vec![0.0; 6];
            knots.extend((1..n - 5).map(|i| i as f64 / (n - 5) as f64));
            knots.extend([1.0; 6]);
            let projection: Vec<_> = (0..n * n).map(|i| Vec3::new((i as f64).sin(), i as f64 / 7.0, -(i as f64))).collect();
            for t in [0.0, 1e-10, 0.1, 0.5, 0.9, 1.0 - 1e-10, 1.0] {
                let (b, d, dd) = basis(&knots, t, n).unwrap();
                for a in [&b, &d, &dd] {
                    for b in [&b, &d, &dd] {
                        let expected = tensor(a, b, 1.0, 0.0);
                        let p = expected.entries.iter().fold(Vec3::ZERO, |p, (i, w)| p + projection[*i] * *w);
                        assert_eq!(project_tensor(&projection, n, a, b), p);
                        let mut sum = RowSum::new(n * n);
                        sum.add_product(a, b, 0.75);
                        assert_eq!(sum.finish().entries, tensor(a, b, 0.75, 0.0).entries);
                    }
                }
            }
        }
    }

    #[test]
    fn sparse_row_sum_preserves_cancellation_and_resets() {
        let mut sum = RowSum::new(9);
        sum.add_product(&[0.0, 2.0, 3.0], &[1.0, 0.0, 4.0], 0.5);
        sum.add_product(&[0.0, 1.0, 0.0], &[1.0, 5.0, 0.0], -1.0);
        assert_eq!(sum.finish().entries, vec![(3, 0.0), (4, -5.0), (5, 4.0), (6, 1.5), (8, 6.0)]);
        sum.add_product(&[1.0, 0.0, 0.0], &[0.0, 2.0, 0.0], 3.0);
        assert_eq!(sum.finish().entries, vec![(1, 6.0)]);
        assert!(sum.finish().entries.is_empty());
    }

    #[test]
    fn physical_curvature_is_independent_of_a_cubic_parameter_map() {
        let (u, v) = (0.2_f64, 0.7_f64);
        let (x, y) = (u.powi(3), v.powi(3));
        let (xu, yv, xuu, yvv) = (3.0 * u * u, 3.0 * v * v, 6.0 * u, 6.0 * v);
        let (_, rows) = physical_curvature_weights([[xu, 0.0], [0.0, yv], [xuu, 0.0], [0.0, 0.0], [0.0, yvv]]).unwrap();
        // z=x²+xy+y² has the same physical Hessian at every chart location.
        let (zx, zy) = (2.0 * x + y, x + 2.0 * y);
        let h = [2.0 * xu * xu + zx * xuu, xu * yv, 2.0 * yv * yv + zy * yvv, zx * xu, zy * yv];
        for (row, expected) in rows.iter().zip([2.0, 1.0, 2.0]) {
            assert!((row.iter().zip(h).map(|(a, b)| a * b).sum::<f64>() - expected).abs() < 1e-10);
            let plane = [3.0 * xuu, 0.0, -2.0 * yvv, 3.0 * xu, -2.0 * yv];
            assert!(row.iter().zip(plane).map(|(a, b)| a * b).sum::<f64>().abs() < 1e-10);
        }
        assert!(physical_curvature_weights([[0.0; 2]; 5]).is_none());
        assert!(physical_curvature_weights([[f64::NAN; 2]; 5]).is_none());
    }

    #[test]
    fn both_corner_limits_fit_without_a_fold() {
        let line = |a: Vec3, b: Vec3| mt::Curve::Line(mt::Line(p3(a), p3(b)));
        let left = (Vec3::Z + Vec3::X * 0.1).normalized().unwrap();
        let right = (Vec3::Z - Vec3::X * 0.1).normalized().unwrap();
        let boundaries = [
            Boundary { curve: line(Vec3::X, Vec3::ZERO), normal: Normal::Plane(Vec3::Z) },
            Boundary {
                curve: line(Vec3::ZERO, Vec3::Y),
                normal: Normal::Hermite { start: Vec3::Z, end: left, start_der: Vec3::ZERO, end_der: Vec3::ZERO },
            },
            Boundary { curve: line(Vec3::Y, Vec3::X + Vec3::Y), normal: Normal::Plane(Vec3::Z) },
            Boundary {
                curve: line(Vec3::X + Vec3::Y, Vec3::X),
                normal: Normal::Hermite { start: right, end: Vec3::Z, start_der: Vec3::ZERO, end_der: Vec3::ZERO },
            },
        ];
        let surface = fit(&boundaries, [true, true], Vec3::Z, 1.0, &[(Vec3::Z, Vec3::ZERO), (-Vec3::Z, -Vec3::Z)], 32).unwrap();
        for i in 1..20 {
            for j in 1..20 {
                let p = from_p3(surface.subs(i as f64 / 20.0, j as f64 / 20.0));
                assert!(p.is_finite() && p.z <= 1e-6 && p.z >= -1.0 - 1e-6);
            }
        }
    }

    #[test]
    fn banded_qr_resolves_scaled_inconsistent_constraints() {
        let rows = [
            Row { entries: vec![(0, 1e-8), (1, 1e8)], rhs: 4.0 },
            Row { entries: vec![(0, 1e-8)], rhs: 1.0 },
            Row { entries: vec![(1, 1e8)], rhs: 2.0 },
        ];
        let x = solve_qr(&rows, 2).unwrap();
        assert!((x[0] * 1e-8 - 4.0 / 3.0).abs() < 1e-12);
        assert!((x[1] * 1e8 - 7.0 / 3.0).abs() < 1e-12);
    }

    #[test]
    fn certificate_accepts_an_isolated_singular_corner_and_rejects_a_fold() {
        // x=u(u²+v²), y=v(u²+v²): determinant is 3(u²+v²)².
        // Different cubic height terms permit different limiting tangent planes.
        let mut ctrl = vec![vec![mt::Point3::new(0.0, 0.0, 0.0); 4]; 4];
        let choose = [1.0, 3.0, 3.0, 1.0];
        for (i, row) in ctrl.iter_mut().enumerate() {
            for (j, point) in row.iter_mut().enumerate() {
                let x =
                    (if i == 3 { 1.0 } else { 0.0 }) + if i >= 1 && j >= 2 { (i as f64 / 3.0) * (if j == 3 { 1.0 } else { 1.0 / 3.0 }) } else { 0.0 };
                let y =
                    (if j == 3 { 1.0 } else { 0.0 }) + if j >= 1 && i >= 2 { (j as f64 / 3.0) * (if i == 3 { 1.0 } else { 1.0 / 3.0 }) } else { 0.0 };
                let z = if i >= 2 && j >= 1 { (if i == 3 { 1.0 } else { 1.0 / choose[2] }) * j as f64 / 3.0 } else { 0.0 };
                *point = mt::Point3::new(x, y, z);
            }
        }
        let knots = mt::KnotVec::bezier_knot(3);
        let surface = mt::BSplineSurface::new((knots.clone(), knots.clone()), ctrl.clone());
        certify(&surface, Vec3::Z, &[], 1e-6).unwrap();
        for row in &mut ctrl {
            for p in row {
                p.x = -p.x;
            }
        }
        let folded = mt::BSplineSurface::new((knots.clone(), knots), ctrl);
        assert!(certify(&folded, Vec3::Z, &[], 1e-6).is_err());
        // This map changes orientation inside the domain, rather than merely
        // reversing the whole surface: x=(u-1/2)², y=v has J=2u-1.
        let knots = mt::KnotVec::bezier_knot(2);
        let ctrl = [0.25, -0.25, 0.25].into_iter().map(|x| (0..3).map(|j| mt::Point3::new(x, j as f64 / 2.0, 0.0)).collect()).collect();
        let interior_fold = mt::BSplineSurface::new((knots.clone(), knots), ctrl);
        assert!(certify(&interior_fold, Vec3::Z, &[], 1e-6).is_err());
    }
}
