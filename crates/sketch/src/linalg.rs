//! Small dense linear algebra for the solver: row-major matrices, Gaussian elimination with
//! partial pivoting, and reduced row echelon form for rank and null-space analysis.

/// Row-major dense matrix.
#[derive(Clone, Debug)]
pub struct Mat {
    pub rows: usize,
    pub cols: usize,
    pub data: Vec<f64>,
}

impl Mat {
    pub fn zeros(rows: usize, cols: usize) -> Mat {
        Mat { rows, cols, data: vec![0.0; rows.saturating_mul(cols)] }
    }
    pub fn get(&self, r: usize, c: usize) -> f64 {
        self.data.get(r * self.cols + c).copied().unwrap_or(0.0)
    }
    pub fn set(&mut self, r: usize, c: usize, v: f64) {
        if let Some(x) = self.data.get_mut(r * self.cols + c) {
            *x = v;
        }
    }
    pub fn add(&mut self, r: usize, c: usize, v: f64) {
        if let Some(x) = self.data.get_mut(r * self.cols + c) {
            *x += v;
        }
    }
    fn swap_rows(&mut self, a: usize, b: usize) {
        if a == b {
            return;
        }
        for c in 0..self.cols {
            self.data.swap(a * self.cols + c, b * self.cols + c);
        }
    }
}

/// Solve `A x = b` (A square). Returns `None` when singular.
pub fn solve(mut a: Mat, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = a.rows;
    if a.cols != n || b.len() != n {
        return None;
    }
    for k in 0..n {
        let mut piv = k;
        let mut best = a.get(k, k).abs();
        for r in k + 1..n {
            let v = a.get(r, k).abs();
            if v > best {
                best = v;
                piv = r;
            }
        }
        if !(best > 1e-300) || !best.is_finite() {
            return None;
        }
        a.swap_rows(k, piv);
        b.swap(k, piv);
        let d = a.get(k, k);
        for r in k + 1..n {
            let f = a.get(r, k) / d;
            if f == 0.0 {
                continue;
            }
            for c in k..n {
                let v = a.get(k, c);
                a.add(r, c, -f * v);
            }
            let bk = *b.get(k)?;
            *b.get_mut(r)? -= f * bk;
        }
    }
    let mut x = vec![0.0; n];
    for k in (0..n).rev() {
        let mut s = *b.get(k)?;
        for c in k + 1..n {
            s -= a.get(k, c) * x.get(c)?;
        }
        *x.get_mut(k)? = s / a.get(k, k);
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}

/// Reduce to row echelon form in place; returns the pivot column of each pivot row.
pub fn rref(a: &mut Mat, tol: f64) -> Vec<usize> {
    let mut pivots = Vec::new();
    let mut row = 0;
    for col in 0..a.cols {
        if row >= a.rows {
            break;
        }
        let mut piv = row;
        let mut best = a.get(row, col).abs();
        for r in row + 1..a.rows {
            let v = a.get(r, col).abs();
            if v > best {
                best = v;
                piv = r;
            }
        }
        if !(best > tol) {
            continue;
        }
        a.swap_rows(row, piv);
        let d = a.get(row, col);
        for c in 0..a.cols {
            let v = a.get(row, c) / d;
            a.set(row, c, v);
        }
        for r in 0..a.rows {
            if r == row {
                continue;
            }
            let f = a.get(r, col);
            if f == 0.0 {
                continue;
            }
            for c in 0..a.cols {
                let v = a.get(row, c);
                a.add(r, c, -f * v);
            }
        }
        pivots.push(col);
        row += 1;
    }
    pivots
}

/// For each column of `j`: is that variable fixed by the constraints (zero in every null-space
/// vector)? Also returns the rank.
pub fn determined_vars(j: &Mat, tol: f64) -> (Vec<bool>, usize) {
    let mut a = j.clone();
    let pivots = rref(&mut a, tol);
    let rank = pivots.len();
    let free: Vec<usize> = (0..a.cols).filter(|c| !pivots.contains(c)).collect();
    let mut det = vec![true; a.cols];
    for f in &free {
        if let Some(d) = det.get_mut(*f) {
            *d = false;
        }
    }
    // Null vector for free column f: x_f = 1, x_pivot(r) = -a[r][f].
    for (r, pc) in pivots.iter().enumerate() {
        if free.iter().any(|f| a.get(r, *f).abs() > tol)
            && let Some(d) = det.get_mut(*pc)
        {
            *d = false;
        }
    }
    (det, rank)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solves_and_detects_singular() {
        let mut a = Mat::zeros(2, 2);
        a.set(0, 0, 2.0);
        a.set(0, 1, 1.0);
        a.set(1, 0, 1.0);
        a.set(1, 1, 3.0);
        let x = solve(a, vec![3.0, 5.0]).unwrap();
        assert!((x[0] - 0.8).abs() < 1e-12 && (x[1] - 1.4).abs() < 1e-12);
        let s = Mat::zeros(2, 2);
        assert!(solve(s, vec![1.0, 1.0]).is_none());
    }

    #[test]
    fn rank_and_determined() {
        // x0 + x1 = .., x2 = .. ; x0 and x1 not individually determined.
        let mut j = Mat::zeros(2, 3);
        j.set(0, 0, 1.0);
        j.set(0, 1, 1.0);
        j.set(1, 2, 1.0);
        let (det, rank) = determined_vars(&j, 1e-9);
        assert_eq!(rank, 2);
        assert_eq!(det, vec![false, false, true]);
    }
}
