use crate::*;

impl<F: BaseFloat> EigenValues for Matrix2<F> {
    type EigenValues = [Complex<F>; 2];
    fn eigenvalues(self) -> [Complex<F>; 2] {
        solver::solve_quadratic(-self.trace(), self.determinant())
    }
}

impl<F: BaseFloat> EigenValues for Matrix3<F> {
    type EigenValues = [Complex<F>; 3];
    fn eigenvalues(self) -> [Complex<F>; 3] {
        let poly2 = self[0][0] * self[1][1] + self[0][0] * self[2][2] + self[1][1] * self[2][2]
            - self[0][1] * self[1][0]
            - self[1][2] * self[2][1]
            - self[0][2] * self[2][0];
        solver::solve_cubic(-self.trace(), poly2, -self.determinant())
    }
}

impl<F: BaseFloat> EigenValues for Matrix4<F> {
    type EigenValues = [Complex<F>; 4];
    fn eigenvalues(self) -> [Complex<F>; 4] {
        let poly2 = self[0][0] * self[1][1]
            + self[0][0] * self[2][2]
            + self[0][0] * self[3][3]
            + self[1][1] * self[2][2]
            + self[1][1] * self[3][3]
            + self[2][2] * self[3][3]
            - self[0][1] * self[1][0]
            - self[0][2] * self[2][0]
            - self[0][3] * self[3][0]
            - self[1][2] * self[2][1]
            - self[1][3] * self[3][1]
            - self[2][3] * self[3][2];
        let poly3 = (self[0][0] * self[1][1] + self[0][0] * self[2][2] + self[1][1] * self[2][2]
            - self[0][1] * self[1][0]
            - self[0][2] * self[2][0]
            - self[1][2] * self[2][1])
            * self[3][3]
            + (self[2][0] * self[3][2] - self[2][2] * self[3][0] + self[1][0] * self[3][1]
                - self[1][1] * self[3][0])
                * self[0][3]
            + (self[2][1] * self[3][2] - self[2][2] * self[3][1] + self[3][0] * self[0][1]
                - self[3][1] * self[0][0])
                * self[1][3]
            + (self[3][1] * self[1][2] - self[3][2] * self[1][1] + self[3][0] * self[0][2]
                - self[3][2] * self[0][0])
                * self[2][3]
            + Matrix3::from_cols(self[0].truncate(), self[1].truncate(), self[2].truncate())
                .determinant();
        solver::solve_quartic(-self.trace(), poly2, -poly3, self.determinant())
    }
}

impl<F: BaseFloat> OperatorNorm for Matrix2<F> {
    #[inline]
    fn norm_l1(self) -> F {
        F::max(
            F::abs(self[0][0]) + F::abs(self[0][1]),
            F::abs(self[1][0]) + F::abs(self[1][1]),
        )
    }
    #[inline]
    fn norm_linf(self) -> F {
        F::max(
            F::abs(self[0][0]) + F::abs(self[1][0]),
            F::abs(self[0][1]) + F::abs(self[1][1]),
        )
    }
    #[inline]
    fn norm_l2(self) -> F {
        let eigens = (self.transpose() * self).eigenvalues();
        F::sqrt(F::max(eigens[0].re, eigens[1].re))
    }
}

impl<F: BaseFloat> OperatorNorm for Matrix3<F> {
    #[inline]
    fn norm_l1(self) -> F {
        F::max(
            F::max(
                F::abs(self[0][0]) + F::abs(self[0][1]) + F::abs(self[0][2]),
                F::abs(self[1][0]) + F::abs(self[1][1]) + F::abs(self[1][2]),
            ),
            F::abs(self[2][0]) + F::abs(self[2][1]) + F::abs(self[2][2]),
        )
    }
    #[inline]
    fn norm_linf(self) -> F {
        F::max(
            F::max(
                F::abs(self[0][0]) + F::abs(self[1][0]) + F::abs(self[2][0]),
                F::abs(self[0][1]) + F::abs(self[1][1]) + F::abs(self[2][1]),
            ),
            F::abs(self[0][2]) + F::abs(self[1][2]) + F::abs(self[2][2]),
        )
    }
    #[inline]
    fn norm_l2(self) -> F {
        let eigens = (self.transpose() * self).eigenvalues();
        F::sqrt(F::max(F::max(eigens[0].re, eigens[1].re), eigens[2].re))
    }
}

impl<F: BaseFloat> OperatorNorm for Matrix4<F> {
    #[inline]
    fn norm_l1(self) -> F {
        F::max(
            F::max(
                F::max(
                    F::abs(self[0][0])
                        + F::abs(self[0][1])
                        + F::abs(self[0][2])
                        + F::abs(self[0][3]),
                    F::abs(self[1][0])
                        + F::abs(self[1][1])
                        + F::abs(self[1][2])
                        + F::abs(self[1][3]),
                ),
                F::abs(self[2][0]) + F::abs(self[2][1]) + F::abs(self[2][2]) + F::abs(self[2][3]),
            ),
            F::abs(self[3][0]) + F::abs(self[3][1]) + F::abs(self[3][2]) + F::abs(self[3][3]),
        )
    }
    #[inline]
    fn norm_linf(self) -> F {
        F::max(
            F::max(
                F::max(
                    F::abs(self[0][0])
                        + F::abs(self[1][0])
                        + F::abs(self[2][0])
                        + F::abs(self[3][0]),
                    F::abs(self[0][1])
                        + F::abs(self[1][1])
                        + F::abs(self[2][1])
                        + F::abs(self[3][1]),
                ),
                F::abs(self[0][2]) + F::abs(self[1][2]) + F::abs(self[2][2]) + F::abs(self[3][2]),
            ),
            F::abs(self[0][3]) + F::abs(self[1][3]) + F::abs(self[2][3]) + F::abs(self[3][3]),
        )
    }
    #[inline]
    fn norm_l2(self) -> F {
        let eigens = (self.transpose() * self).eigenvalues();
        F::sqrt(F::max(
            F::max(F::max(eigens[0].re, eigens[1].re), eigens[2].re),
            eigens[3].re,
        ))
    }
}
