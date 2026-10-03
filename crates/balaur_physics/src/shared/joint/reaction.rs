//! The impulse a joint's solver rows gave its first body, which is what
//! `break_force`, `break_torque` and `joint_force` measure.

use crate::scalar::Real;

/// One row rapier's solver builds for an impulse joint: its Jacobian on the
/// first body, linear and then angular about that body's centre of mass, its
/// angular part on the second body, the impulse written back for it, and
/// whether later rows of its group are orthogonalised against it.
pub(crate) struct Row<const L: usize, const A: usize> {
    pub(crate) lin: [Real; L],
    pub(crate) ang1: [Real; A],
    pub(crate) ang2: [Real; A],
    pub(crate) impulse: Real,
    pub(crate) locked: bool,
}

/// The solver's inner product between two rows: both bodies' inverse masses,
/// summed per axis, and each body's inverse inertia in world space.
pub(crate) struct Metric<const L: usize, const A: usize> {
    pub(crate) inv_mass: [Real; L],
    pub(crate) inv_inertia1: [[Real; A]; A],
    pub(crate) inv_inertia2: [[Real; A]; A],
}

impl<const L: usize, const A: usize> Metric<L, A> {
    fn dot(&self, a: &Row<L, A>, b: &Row<L, A>) -> Real {
        let quad = |m: &[[Real; A]; A], x: &[Real; A], y: &[Real; A]| -> Real {
            (0..A)
                .map(|i| x[i] * (0..A).map(|j| m[i][j] * y[j]).sum::<Real>())
                .sum()
        };
        let lin: Real = (0..L).map(|i| a.lin[i] * self.inv_mass[i] * b.lin[i]).sum();
        lin + quad(&self.inv_inertia1, &a.ang1, &b.ang1)
            + quad(&self.inv_inertia2, &a.ang2, &b.ang2)
    }
}

/// The impulse a joint's rows gave its first body: linear, and angular about
/// its centre of mass.
///
/// Rapier orthogonalises each group's rows against the unbounded ones before
/// solving, and writes back the impulse along the orthogonalised row
/// (`JointConstraintHelper::finalize_constraints`), so a written-back impulse
/// means nothing alone. The rows are orthogonalised here the same way before
/// they are summed.
pub(crate) fn applied<const L: usize, const A: usize>(
    groups: &mut [Vec<Row<L, A>>],
    metric: &Metric<L, A>,
) -> ([Real; L], [Real; A]) {
    let (mut lin, mut ang) = ([0.0; L], [0.0; A]);
    for rows in groups.iter_mut() {
        for j in 0..rows.len() {
            let own = metric.dot(&rows[j], &rows[j]);
            if !rows[j].locked || own <= 0.0 {
                continue;
            }
            for i in (j + 1)..rows.len() {
                let coeff = metric.dot(&rows[i], &rows[j]) / own;
                let (lin_j, ang1_j, ang2_j) = (rows[j].lin, rows[j].ang1, rows[j].ang2);
                let row = &mut rows[i];
                row.lin
                    .iter_mut()
                    .zip(lin_j)
                    .for_each(|(x, y)| *x -= y * coeff);
                row.ang1
                    .iter_mut()
                    .zip(ang1_j)
                    .for_each(|(x, y)| *x -= y * coeff);
                row.ang2
                    .iter_mut()
                    .zip(ang2_j)
                    .for_each(|(x, y)| *x -= y * coeff);
            }
        }
        for row in rows.iter() {
            lin.iter_mut()
                .zip(row.lin)
                .for_each(|(x, y)| *x += y * row.impulse);
            ang.iter_mut()
                .zip(row.ang1)
                .for_each(|(x, y)| *x += y * row.impulse);
        }
    }
    (lin, ang)
}
