use crate::*;

impl<F: BaseFloat> Exponential for Matrix2<F> {}
impl<F: BaseFloat> Exponential for Matrix3<F> {}
impl<F: BaseFloat> Exponential for Matrix4<F> {}

impl<F: BaseFloat> Decomposition for Matrix2<F> {
    fn iwasawa_decomposition(self) -> Option<(Self, Self, Self)> {
        let v0 = self[0];
        let mag0 = dot(v0, v0);
        if mag0 <= F::zero() {
            return None;
        }
        let n0 = dot(self[1], v0) / mag0;
        let v1 = self[1] - v0 * n0;
        let mag1 = dot(v1, v1);
        if mag1 <= F::zero() {
            return None;
        }
        let (a0, a1) = (F::sqrt(mag0), F::sqrt(mag1));
        Some((
            Matrix2::from_cols(v0 / a0, v1 / a1),
            Matrix2::from_diagonal(Vector2::new(a0, a1)),
            Matrix2::new(F::one(), F::zero(), n0, F::one()),
        ))
    }
}

impl<F: BaseFloat> Decomposition for Matrix3<F> {
    fn iwasawa_decomposition(self) -> Option<(Self, Self, Self)> {
        let v0 = self[0];
        let mag0 = dot(v0, v0);
        if mag0 <= F::zero() {
            return None;
        }
        let n0 = dot(self[1], v0) / mag0;
        let v1 = self[1] - v0 * n0;
        let mag1 = dot(v1, v1);
        if mag1 <= F::zero() {
            return None;
        }
        let n1 = dot(self[2], v0) / mag0;
        let n2 = dot(self[2], v1) / mag1;
        let v2 = self[2] - v0 * n1 - v1 * n2;
        let mag2 = dot(v2, v2);
        if mag2 <= F::zero() {
            return None;
        }
        let (a0, a1, a2) = (F::sqrt(mag0), F::sqrt(mag1), F::sqrt(mag2));
        #[rustfmt::skip]
        let res = Some((
            Matrix3::from_cols(v0 / a0, v1 / a1, v2 / a2),
            Matrix3::from_diagonal(Vector3::new(a0, a1, a2)),
            Matrix3::new(
                F::one(), F::zero(), F::zero(),
                n0, F::one(), F::zero(),
                n1, n2, F::one(),
            ),
        ));
        res
    }
}

impl<F: BaseFloat> Decomposition for Matrix4<F> {
    fn iwasawa_decomposition(self) -> Option<(Self, Self, Self)> {
        let v0 = self[0];
        let mag0 = dot(v0, v0);
        if mag0 <= F::zero() {
            return None;
        }
        let n0 = dot(self[1], v0) / mag0;
        let v1 = self[1] - v0 * n0;
        let mag1 = dot(v1, v1);
        if mag1 <= F::zero() {
            return None;
        }
        let n1 = dot(self[2], v0) / mag0;
        let n2 = dot(self[2], v1) / mag1;
        let v2 = self[2] - v0 * n1 - v1 * n2;
        let mag2 = dot(v2, v2);
        if mag2 <= F::zero() {
            return None;
        }
        let n3 = dot(self[3], v0) / mag0;
        let n4 = dot(self[3], v1) / mag1;
        let n5 = dot(self[3], v2) / mag2;
        let v3 = self[3] - v0 * n3 - v1 * n4 - v2 * n5;
        let mag3 = dot(v3, v3);
        if mag3 <= F::zero() {
            return None;
        }
        let (a0, a1, a2, a3) = (F::sqrt(mag0), F::sqrt(mag1), F::sqrt(mag2), F::sqrt(mag3));
        #[rustfmt::skip]
        let res = Some((
            Matrix4::from_cols(v0 / a0, v1 / a1, v2 / a2, v3 / a3),
            Matrix4::from_diagonal(Vector4::new(a0, a1, a2, a3)),
            Matrix4::new(
                F::one(), F::zero(), F::zero(), F::zero(),
                n0, F::one(), F::zero(), F::zero(),
                n1, n2, F::one(), F::zero(),
                n3, n4, n5, F::one(),
            ),
        ));
        res
    }
}
