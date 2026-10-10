use crate::*;

#[cfg_attr(doc, katexit::katexit)]
/// Solve equation: $x^2 + ax + b = 0$.
///
/// # Examples
///
/// ```
/// use matext4cgmath::solver;
/// use num_complex::Complex;
/// const EPS: f64 = 1.0e-10;
///
/// let res = solver::solve_quadratic(3.0, -4.0);
/// // Real solutions are always sorted in order of solution size.
/// assert!(Complex::norm(res[0] + 4.0) < EPS);
/// assert!(Complex::norm(res[1] - 1.0) < EPS);
/// ```
pub fn solve_quadratic<F: BaseFloat>(a: F, b: F) -> [Complex<F>; 2] {
    let two = F::one() + F::one();
    let four = two + two;
    let det = a * a - four * b;
    match det >= F::zero() {
        true => {
            let h = F::sqrt(det);
            [
                Complex::new((-a - h) / two, F::zero()),
                Complex::new((-a + h) / two, F::zero()),
            ]
        }
        false => {
            let h = F::sqrt(-det);
            [Complex::new(-a, h) / two, Complex::new(-a, -h) / two]
        }
    }
}

#[cfg_attr(doc, katexit::katexit)]
/// Solve equation: $x^3 + px + q = 0$.
///
/// # Examples
///
/// ```
/// use matext4cgmath::solver;
/// use num_complex::Complex;
/// const EPS: f64 = 1.0e-10;
///
/// let mut res = solver::pre_solve_cubic(-7.0, -6.0);
/// // Even in the case of real solutions, the order in the array is not guaranteed.
/// res.sort_by(|x, y| x.re.partial_cmp(&y.re).unwrap());
/// let ans = [Complex::from(-2.0), Complex::from(-1.0), Complex::from(3.0)];
/// res.iter().zip(ans).for_each(|(x, y)| {
///     assert!(Complex::norm(x - y) < EPS);
/// });
/// ```
pub fn pre_solve_cubic<F: BaseFloat>(p: F, q: F) -> [Complex<F>; 3] {
    let two = F::one() + F::one();
    let three = two + F::one();
    let sqrt3_2 = F::sqrt(three) / two;
    let omega = Complex::new(-F::one() / two, sqrt3_2);
    let omega2 = Complex::new(-F::one() / two, -sqrt3_2);
    let eps_2 = F::sqrt(F::epsilon());

    let p_3 = p / three;
    let q_2 = q / two;
    let alpha2 = q_2 * q_2 + p_3 * p_3 * p_3;
    let (x, y) = match alpha2 >= F::zero() {
        true => {
            let alpha = F::sqrt(alpha2);
            let tmpx = -q_2 - alpha;
            let tmpy = -q_2 + alpha;
            (
                Complex::new(
                    F::signum(tmpx) * F::powf(F::abs(tmpx), F::one() / three),
                    F::zero(),
                ),
                Complex::new(
                    F::signum(tmpy) * F::powf(F::abs(tmpy), F::one() / three),
                    F::zero(),
                ),
            )
        }
        false => {
            let alphai = F::sqrt(-alpha2);
            (
                Complex::powf(Complex::new(-q_2, alphai), F::one() / three),
                Complex::powf(Complex::new(-q_2, -alphai), F::one() / three),
            )
        }
    };
    let mut res = [x + y, omega * x + omega2 * y, omega2 * x + omega * y];
    // precision by Newton method
    res.iter_mut().for_each(|x| {
        let mut f = *x * *x * *x + *x * p + q;
        let mut f_prime = *x * *x * three + p;
        // SolveCraft: at most 100 steps. Near a double root (the eigenvalues of a
        // translation matrix) the iteration can cycle forever without meeting the tolerance.
        let mut steps = 0;
        while f.norm() > eps_2 * f_prime.norm() && steps < 100 {
            steps += 1;
            if f_prime.norm() < eps_2 {
                return;
            }
            *x -= f / f_prime;
            f = *x * *x * *x + *x * p + q;
            f_prime = *x * *x * three + p;
        }
    });
    res
}

#[cfg_attr(doc, katexit::katexit)]
/// solve equation: $x^3 + ax^2 + bx + c = 0$.
///
/// # Examples
///
/// ```
/// use matext4cgmath::solver;
/// use num_complex::Complex;
/// const EPS: f64 = 1.0e-10;
///
/// let mut res = solver::solve_cubic(-3.0, 0.0, 4.0);
/// // Even in the case of real solutions, the order in the array is not guaranteed.
/// res.sort_by(|x, y| x.re.partial_cmp(&y.re).unwrap());
/// let ans = [Complex::from(-1.0), Complex::from(2.0), Complex::from(2.0)];
/// res.iter().zip(ans).for_each(|(x, y)| {
///     assert!(Complex::norm(x - y) < EPS);
/// });
/// ```
pub fn solve_cubic<F: BaseFloat>(a: F, b: F, c: F) -> [Complex<F>; 3] {
    let two = F::one() + F::one();
    let three = two + F::one();
    let twenty_seven = three * three * three;
    let p = b - a * a / three;
    let q = c - a * b / three + two * a * a * a / twenty_seven;
    let mut res = pre_solve_cubic(p, q);
    res.iter_mut().for_each(|x| {
        *x -= a / three;
    });
    res
}

#[cfg_attr(doc, katexit::katexit)]
/// solve equation: $x^4 + px^2 + qx + r = 0$.
///
/// # Examples
///
/// ```
/// use matext4cgmath::solver;
/// use num_complex::Complex;
/// const EPS: f64 = 1e-7;
///
/// let mut res = solver::pre_solve_quartic(-5.0, 0.0, 4.0);
/// // Even in the case of real solutions, the order in the array is not guaranteed.
/// res.sort_by(|x, y| x.re.partial_cmp(&y.re).unwrap());
/// let ans = [Complex::from(-2.0), Complex::from(-1.0), Complex::from(1.0), Complex::from(2.0)];
/// res.iter().zip(ans).for_each(|(x, y)| {
///     assert!(Complex::norm(x - y) < EPS);
/// });
/// ```
pub fn pre_solve_quartic<F: BaseFloat>(p: F, q: F, r: F) -> [Complex<F>; 4] {
    let one = F::one();
    let two = one + one;
    let four = two + two;
    let eps_2 = F::sqrt(F::epsilon());

    let a = two * p;
    let b = p * p - four * r;
    let c = -q * q;
    let f = solve_cubic(a, b, c);
    let a = f[0].sqrt() / two;
    let b = f[1].sqrt() / two;
    let c = f[2].sqrt() / two;

    let mut res = (0..8)
        .map(|i| {
            let a = a * F::powi(-F::one(), i % 2);
            let b = b * F::powi(-F::one(), (i / 2) % 2);
            let c = c * F::powi(-F::one(), (i / 4) % 2);
            [-a - b - c, -a + b + c, a - b + c, a + b - c]
        })
        .map(|x| {
            let f = x
                .iter()
                .map(|t| (t * t * t * t + t * t * p + t * q + r).norm_sqr())
                .max_by(|x, y| x.partial_cmp(y).unwrap())
                .unwrap();
            (x, f)
        })
        .min_by(|x, y| x.1.partial_cmp(&y.1).unwrap())
        .unwrap()
        .0;
    // refinement by Newton method
    res.iter_mut().for_each(|x| {
        let mut f = *x * *x * *x * *x + *x * *x * p + *x * q + r;
        let mut f_prime = *x * *x * *x * four + *x * p * two + q;
        // SolveCraft: at most 100 steps. Near a double root (the eigenvalues of a
        // translation matrix) the iteration can cycle forever without meeting the tolerance.
        let mut steps = 0;
        while f.norm() > eps_2 * f_prime.norm() && steps < 100 {
            steps += 1;
            if f_prime.norm() < eps_2 {
                return;
            }
            *x -= f / f_prime;
            f_prime = *x * *x * *x * four + *x * p * two + q;
            f = *x * *x * *x * *x + *x * *x * p + *x * q + r;
        }
    });
    res
}

#[cfg_attr(doc, katexit::katexit)]
/// solve equation: $x^4 + ax^3 + bx^2 + cx + d = 0$.
///
/// # Examples
///
/// ```
/// use matext4cgmath::solver;
/// use num_complex::Complex;
/// const EPS: f64 = 1.0e-10;
///
/// let mut res = solver::solve_quartic(1.0, -7.0, -1.0, 6.0);
/// // Even in the case of real solutions, the order in the array is not guaranteed.
/// res.sort_by(|x, y| x.re.partial_cmp(&y.re).unwrap());
/// let ans = [Complex::from(-3.0), Complex::from(-1.0), Complex::from(1.0), Complex::from(2.0)];
/// res.iter().zip(ans).for_each(|(x, y)| {
///     assert!(Complex::norm(x * x * x * x + x * x * x - 7.0 * x * x - x + 6.0) < EPS);
///     assert!(Complex::norm(x - y) < EPS, "{x} {y}");
/// });
/// ```
pub fn solve_quartic<F: BaseFloat>(a: F, b: F, c: F, d: F) -> [Complex<F>; 4] {
    let one = F::one();
    let two = one + one;
    let three = one + two;
    let four = two + two;
    let six = two * three;
    let eight = four + four;

    let a_4 = a / four;
    let p = b - six * a_4 * a_4;
    let q = c - two * b * a_4 + eight * a_4 * a_4 * a_4;
    let r = d - c * a_4 + b * a_4 * a_4 - three * a_4 * a_4 * a_4 * a_4;
    let mut res = pre_solve_quartic(p, q, r);
    res.iter_mut().for_each(|x| *x -= a_4);
    res
}
