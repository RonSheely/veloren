use super::{FigureBoneData, Skeleton, make_bone, vek::*};

pub type Body = ();

#[derive(Clone, Default)]
pub struct FixtureSkeleton;

pub struct SkeletonAttr;

impl<Factor> Lerp<Factor> for &FixtureSkeleton {
    type Output = FixtureSkeleton;

    fn lerp_unclamped(_from: Self, _to: Self, _factor: Factor) -> Self::Output { FixtureSkeleton }

    fn lerp_unclamped_precise(_from: Self, _to: Self, _factor: Factor) -> Self::Output {
        FixtureSkeleton
    }
}

impl Skeleton for FixtureSkeleton {
    type Attr = SkeletonAttr;
    type Body = Body;
    type ComputedSkeleton = ();

    const BONE_COUNT: usize = 1;
    #[cfg(feature = "use-dyn-lib")]
    const COMPUTE_FN: &'static [u8] = b"fixture_compute_mats\0";

    #[cfg_attr(feature = "be-dyn-lib", unsafe(export_name = "fixture_compute_mats"))]
    fn compute_matrices_inner(
        &self,
        base_mat: Mat4<f32>,
        buf: &mut [FigureBoneData; super::MAX_BONE_COUNT],
        (): Self::Body,
    ) -> Self::ComputedSkeleton {
        buf[0] = make_bone(base_mat);
    }
}

impl Default for SkeletonAttr {
    fn default() -> Self { Self }
}

impl<'a> From<&'a Body> for SkeletonAttr {
    fn from(_body: &'a Body) -> Self { Self }
}
