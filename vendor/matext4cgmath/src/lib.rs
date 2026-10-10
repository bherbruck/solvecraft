//! Unofficial third-party cgmath extensions for calculate eigenvalues, operator norms and Iwasawa decomposition.

// SolveCraft: compiler warnings no longer fail the build.
#![deny(clippy::all, rust_2018_idioms)]
#![warn(
    missing_docs,
    missing_debug_implementations,
    trivial_casts,
    trivial_numeric_casts,
    unsafe_code,
    unstable_features,
    unused_import_braces,
    unused_qualifications
)]
#![allow(clippy::many_single_char_names)]

pub use cgmath;
use cgmath::*;
use num_complex::Complex;

mod eigens;
mod exp_decomp;
/// solvers for low dimensional algebraic equations.
pub mod solver;

/// extension for eigen values
pub trait EigenValues: VectorSpace {
    /// the type of the array of eigen values
    type EigenValues;
    /// calculate eigen values.
    ///
    /// # Examples
    ///
    /// ```
    /// use cgmath::*;
    /// use matext4cgmath::*;
    /// use num_complex::Complex;
    /// const EPS: f64 = 1.0e-10;
    ///
    /// let mat = Matrix2::new(4.0, -2.0, 3.0, -1.0);
    /// let mut eigens = mat.eigenvalues();
    /// // Even in the case of real solutions, the order in the array is not guaranteed.
    /// eigens.sort_by(|x, y| x.re.partial_cmp(&y.re).unwrap());
    /// assert!(Complex::norm(eigens[0] - 1.0) < EPS);
    /// assert!(Complex::norm(eigens[1] - 2.0) < EPS);
    /// ```
    fn eigenvalues(self) -> Self::EigenValues;
}

#[cfg_attr(doc, katexit::katexit)]
/// [operator norms](https://en.wikipedia.org/wiki/Matrix_norm): $L^1$, $L^2$, and $L^\infty$.
/// # Examples
///
/// ```
/// use cgmath::*;
/// use matext4cgmath::*;
///
/// let mat = Matrix2::new(1.0, 3.0, -2.0, 4.0);
/// // L^1 operator norm is the maximum column absolute sumation.
/// assert_eq!(mat.norm_l1(), 6.0);
/// // L^2 operator norm is the maximum singular value.
/// let ans_norm_l2 = (5.0 + f64::sqrt(5.0)) / f64::sqrt(2.0);
/// assert!(f64::abs(mat.norm_l2() - ans_norm_l2) < 1.0e-10);
/// // L^∞ operator norm is the maximum row absolute sumation.
/// assert_eq!(mat.norm_linf(), 7.0);
/// ```
pub trait OperatorNorm: VectorSpace {
    /// operator norm for absolute value sumation: $L^1$.
    fn norm_l1(self) -> Self::Scalar;
    /// operator norm for Euclidean norm: $L^2$.
    fn norm_l2(self) -> Self::Scalar;
    /// operator norm for absolute value maximum: $L^{\infty}$.
    fn norm_linf(self) -> Self::Scalar;
}

/// calculate exponential value
pub trait Exponential: OperatorNorm + std::ops::AddAssign<Self> + One
where
    Self::Scalar: BaseFloat,
{
    /// calculate exponential
    ///
    /// # Examples
    ///
    /// ```
    /// use cgmath::*;
    /// use matext4cgmath::*;
    ///
    /// let x = Matrix2::new(0.0, 1.0, -1.0, 0.0);
    /// let res = x.exp();
    /// let ans = Matrix2::from_angle(Rad(1.0));
    /// assert!((res - ans).norm_l1() < 1.0e-10);
    /// ```
    fn exp(self) -> Self {
        use num_traits::{Float, NumCast};
        let eps = <Self::Scalar as Float>::epsilon();
        let mut a = self;
        let mut res = Self::one();
        for i in 2_u16..=64 {
            res += a;
            a = a * self / <Self::Scalar as NumCast>::from(i).unwrap();
            if a.norm_linf() < eps * self.norm_linf() {
                break;
            }
        }
        res
    }
}

#[cfg_attr(doc, katexit::katexit)]
/// some decompositions of matrix
pub trait Decomposition: VectorSpace {
    #[cfg_attr(doc, katexit::katexit)]
    /// Returns $(K, A, N)$ of the Iwasawa decomposition: $M = KAN$.
    ///
    /// - $K$: orthonormal matrix
    /// - $A$: diagonal matrix
    /// - $N$: upper-half unipotent matrix
    fn iwasawa_decomposition(self) -> Option<(Self, Self, Self)>;
}
