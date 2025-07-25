pub mod alpha;
pub mod combomelee;
pub mod feed;
pub mod idle;
pub mod jump;
pub mod run;
pub mod shockwave;
pub mod stunned;

// Reexports
pub use self::{
    alpha::AlphaAnimation, combomelee::ComboAnimation, feed::FeedAnimation, idle::IdleAnimation,
    jump::JumpAnimation, run::RunAnimation, shockwave::ShockwaveAnimation,
    stunned::StunnedAnimation,
};

use super::{FigureBoneData, Skeleton, vek::*};
use common::comp::{self};
use core::convert::TryFrom;

pub type Body = comp::quadruped_small::Body;

skeleton_impls!(struct QuadrupedSmallSkeleton ComputedQuadrupedSmallSkeleton {
    + head
    + chest
    + leg_fl
    + leg_fr
    + leg_bl
    + leg_br
    + tail
    mount
});

impl Skeleton for QuadrupedSmallSkeleton {
    type Attr = SkeletonAttr;
    type Body = Body;
    type ComputedSkeleton = ComputedQuadrupedSmallSkeleton;

    const BONE_COUNT: usize = ComputedQuadrupedSmallSkeleton::BONE_COUNT;
    #[cfg(feature = "use-dyn-lib")]
    const COMPUTE_FN: &'static [u8] = b"quadruped_small_compute_mats\0";

    #[cfg_attr(
        feature = "be-dyn-lib",
        unsafe(export_name = "quadruped_small_compute_mats")
    )]
    fn compute_matrices_inner(
        &self,
        base_mat: Mat4<f32>,
        buf: &mut [FigureBoneData; super::MAX_BONE_COUNT],
        body: Self::Body,
    ) -> Self::ComputedSkeleton {
        let chest_mat = base_mat
            * Mat4::scaling_3d(SkeletonAttr::from(&body).scaler / 11.0)
            * Mat4::<f32>::from(self.chest);
        let head_mat = chest_mat * Mat4::<f32>::from(self.head);

        let computed_skeleton = ComputedQuadrupedSmallSkeleton {
            head: head_mat,
            chest: chest_mat,
            leg_fl: chest_mat * Mat4::<f32>::from(self.leg_fl),
            leg_fr: chest_mat * Mat4::<f32>::from(self.leg_fr),
            leg_bl: chest_mat * Mat4::<f32>::from(self.leg_bl),
            leg_br: chest_mat * Mat4::<f32>::from(self.leg_br),
            tail: chest_mat * Mat4::<f32>::from(self.tail),
        };

        computed_skeleton.set_figure_bone_data(buf);
        computed_skeleton
    }
}

pub struct SkeletonAttr {
    head: (f32, f32),
    chest: (f32, f32),
    feet_f: (f32, f32, f32),
    feet_b: (f32, f32, f32),
    tail: (f32, f32),
    scaler: f32,
    tempo: f32,
    maximize: f32,
    minimize: f32,
    spring: f32,
    feed: f32,
    lateral: f32,
}
impl<'a> TryFrom<&'a comp::Body> for SkeletonAttr {
    type Error = ();

    fn try_from(body: &'a comp::Body) -> Result<Self, Self::Error> {
        match body {
            comp::Body::QuadrupedSmall(body) => Ok(SkeletonAttr::from(body)),
            _ => Err(()),
        }
    }
}

impl Default for SkeletonAttr {
    fn default() -> Self {
        Self {
            head: (0.0, 0.0),
            chest: (0.0, 0.0),
            feet_f: (0.0, 0.0, 0.0),
            feet_b: (0.0, 0.0, 0.0),
            tail: (0.0, 0.0),
            scaler: 0.0,
            tempo: 0.0,
            maximize: 0.0,
            minimize: 0.0,
            spring: 0.0,
            feed: 0.0,
            lateral: 0.0,
        }
    }
}

impl<'a> From<&'a Body> for SkeletonAttr {
    fn from(body: &'a Body) -> Self {
        use comp::quadruped_small::{BodyType::*, Species::*};
        Self {
            head: match (body.species, body.body_type) {
                (Pig, _) => (5.0, 2.0),
                (Fox, _) => (4.0, 3.0),
                (Sheep, _) => (4.0, 4.0),
                (Boar, _) => (7.0, 0.0),
                (Jackalope, _) => (3.0, 2.0),
                (Skunk, _) => (5.0, 1.5),
                (Cat, _) => (4.0, 3.0),
                (Batfox, _) => (5.0, 1.0),
                (Raccoon, _) => (5.0, 2.0),
                (Quokka, _) => (6.0, 2.0),
                (Holladon, _) => (7.0, 1.0),
                (Hyena, _) => (7.5, 2.0),
                (Rabbit, _) => (4.0, 3.0),
                (Truffler, _) => (7.5, -9.0),
                (Frog, _) => (4.0, 2.0),
                (Rat, _) => (5.0, -1.0),
                (Axolotl, _) => (3.0, 2.0),
                (Gecko, _) => (4.0, 2.0),
                (Turtle, _) => (5.0, -2.0),
                (Squirrel, _) => (3.5, 1.0),
                (Fungome, _) => (1.5, -1.5),
                (Porcupine, _) => (6.0, 1.0),
                (Beaver, _) => (5.5, 0.0),
                (Hare, Male) => (3.0, 2.0),
                (Hare, Female) => (2.5, 3.0),
                (Dog, _) => (3.0, 4.5),
                (Goat, _) => (3.5, 4.0),
                (Seal, _) => (4.0, 2.5),
                (TreantSapling, _) => (5.0, -2.0),
                (MossySnail, _) => (2.0, 2.0),
            },
            chest: match (body.species, body.body_type) {
                (Pig, _) => (0.0, 6.0),
                (Fox, _) => (0.0, 8.0),
                (Sheep, _) => (2.0, 7.0),
                (Boar, _) => (0.0, 9.5),
                (Jackalope, _) => (-2.0, 6.0),
                (Skunk, _) => (0.0, 6.0),
                (Cat, _) => (0.0, 6.0),
                (Batfox, _) => (-2.0, 6.0),
                (Raccoon, _) => (0.0, 5.5),
                (Quokka, _) => (2.0, 6.5),
                (Holladon, _) => (-2.0, 9.0),
                (Hyena, _) => (-2.0, 9.0),
                (Rabbit, _) => (-2.0, 6.0),
                (Truffler, _) => (-2.0, 16.0),
                (Frog, _) => (-2.0, 4.5),
                (Rat, _) => (6.0, 5.0),
                (Axolotl, _) => (3.0, 5.0),
                (Gecko, _) => (7.5, 4.0),
                (Turtle, _) => (1.0, 6.0),
                (Squirrel, _) => (4.0, 5.0),
                (Fungome, _) => (4.0, 4.0),
                (Porcupine, _) => (2.0, 11.0),
                (Beaver, _) => (2.0, 6.0),
                (Hare, Male) => (-2.0, 7.0),
                (Hare, Female) => (-2.0, 6.0),
                (Dog, _) => (-2.0, 8.5),
                (Goat, _) => (2.0, 7.5),
                (Seal, _) => (-2.0, 4.0),
                (TreantSapling, _) => (1.0, 13.0),
                (MossySnail, _) => (-2.0, 4.5),
            },
            feet_f: match (body.species, body.body_type) {
                (Pig, _) => (4.5, 3.5, -1.0),
                (Fox, _) => (3.0, 5.0, -5.5),
                (Sheep, _) => (3.5, 2.0, -2.0),
                (Boar, _) => (3.5, 6.0, -5.5),
                (Jackalope, _) => (3.0, 4.0, -2.0),
                (Skunk, _) => (3.5, 4.0, -1.0),
                (Cat, _) => (2.0, 4.0, -1.0),
                (Batfox, _) => (3.0, 4.0, -0.5),
                (Raccoon, _) => (4.0, 4.0, -0.0),
                (Quokka, _) => (3.0, 4.0, -1.0),
                (Holladon, _) => (5.0, 4.0, -2.5),
                (Hyena, _) => (2.5, 5.0, -4.0),
                (Rabbit, _) => (3.0, 3.0, -3.0),
                (Truffler, _) => (2.5, 5.0, -9.0),
                (Frog, _) => (4.5, 6.5, 0.0),
                (Rat, _) => (3.0, 2.5, -1.0),
                (Axolotl, _) => (2.0, 2.0, -2.0),
                (Gecko, _) => (2.0, 4.0, -0.5),
                (Turtle, _) => (5.0, 4.0, -2.0),
                (Squirrel, _) => (3.5, 3.0, -1.0),
                (Fungome, _) => (3.0, 2.0, -1.0),
                (Porcupine, _) => (4.0, 6.5, -9.0),
                (Beaver, _) => (4.5, 4.5, -4.0),
                (Hare, Male) => (3.0, 1.0, -3.0),
                (Hare, Female) => (3.0, 0.5, -4.0),
                (Dog, _) => (3.5, 3.0, -2.5),
                (Goat, _) => (3.0, 2.5, -3.5),
                (Seal, _) => (6.5, 3.0, -2.0),
                (TreantSapling, _) => (5.0, 4.0, -10.0),
                (MossySnail, _) => (4.5, 6.5, 0.0),
            },
            feet_b: match (body.species, body.body_type) {
                (Pig, _) => (3.5, -2.0, 0.0),
                (Fox, _) => (3.0, -3.0, -3.0),
                (Sheep, _) => (3.5, -3.5, -2.0),
                (Boar, _) => (3.0, -3.0, -2.5),
                (Jackalope, _) => (3.5, -2.0, 0.0),
                (Skunk, _) => (3.5, -4.0, -1.5),
                (Cat, _) => (2.0, -3.5, -1.0),
                (Batfox, _) => (3.5, -2.0, -0.5),
                (Raccoon, _) => (4.5, -3.0, 0.5),
                (Quokka, _) => (4.0, -4.0, -1.0),
                (Holladon, _) => (4.0, -2.0, -3.0),
                (Hyena, _) => (3.0, -5.0, -2.5),
                (Rabbit, _) => (3.5, -2.0, -1.0),
                (Truffler, _) => (3.0, -5.0, -9.5),
                (Frog, _) => (5.0, -3.5, 0.0),
                (Rat, _) => (3.0, -2.0, 1.0),
                (Axolotl, _) => (2.0, -3.0, -2.0),
                (Gecko, _) => (1.5, -2.0, -0.5),
                (Turtle, _) => (5.5, -2.5, -2.0),
                (Squirrel, _) => (3.5, -3.0, 0.0),
                (Fungome, _) => (3.0, -3.5, -1.0),
                (Porcupine, _) => (4.5, -1.0, -8.0),
                (Beaver, _) => (4.0, -2.5, -3.0),
                (Hare, Male) => (3.5, -1.0, -2.0),
                (Hare, Female) => (3.5, -3.0, -2.0),
                (Dog, _) => (3.0, -3.5, -2.5),
                (Goat, _) => (3.0, -4.0, -2.0),
                (Seal, _) => (4.5, -6.0, -0.5),
                (TreantSapling, _) => (5.5, -4.0, -10.0),
                (MossySnail, _) => (5.0, -3.5, 0.0),
            },
            tail: match (body.species, body.body_type) {
                (Pig, _) => (-4.5, 2.5),
                (Fox, _) => (-4.5, 2.0),
                (Sheep, _) => (-5.0, 0.0),
                (Boar, _) => (-6.0, 0.0),
                (Jackalope, _) => (-4.0, 2.0),
                (Skunk, _) => (-4.0, 0.5),
                (Cat, _) => (-3.5, 2.0),
                (Batfox, _) => (0.0, 5.0),
                (Raccoon, _) => (-4.0, 1.0),
                (Quokka, _) => (-6.0, 1.0),
                (Holladon, _) => (-1.0, 4.0),
                (Hyena, _) => (-7.0, 0.0),
                (Rabbit, _) => (-4.0, -0.0),
                (Truffler, _) => (0.0, 0.0),
                (Frog, _) => (0.0, -0.0),
                (Rat, _) => (-3.0, 0.0),
                (Axolotl, _) => (-4.0, -1.0),
                (Gecko, _) => (-4.0, 0.0),
                (Turtle, _) => (-6.0, -2.0),
                (Squirrel, _) => (-4.0, 0.0),
                (Fungome, _) => (-4.0, -2.0),
                (Porcupine, _) => (-6.0, 1.0),
                (Beaver, _) => (-6.5, -1.0),
                (Hare, Male) => (-4.0, -1.0),
                (Hare, Female) => (-4.0, 2.0),
                (Dog, _) => (-5.0, 0.5),
                (Goat, _) => (-7.0, 0.0),
                (Seal, _) => (-1.0, 4.0),
                (TreantSapling, _) => (-6.0, -2.0),
                (MossySnail, _) => (0.0, -0.0),
            },
            scaler: match (body.species, body.body_type) {
                (Pig, _) => 0.72,
                (Fox, _) => 0.72,
                (Boar, _) => 0.95,
                (Jackalope, _) => 0.67,
                (Skunk, _) => 0.72,
                (Cat, _) => 0.67,
                (Batfox, _) => 0.9,
                (Holladon, _) => 1.12,
                (Rabbit, _) => 0.56,
                (Frog, _) => 0.56,
                (Rat, _) => 0.5,
                (Axolotl, _) => 0.5,
                (Gecko, _) => 0.56,
                (Turtle, _) => 0.67,
                (Squirrel, _) => 0.4,
                (Fungome, _) => 0.72,
                (Porcupine, _) => 0.65,
                (Hare, _) => 0.65,
                (Seal, _) => 0.9,
                (MossySnail, _) => 1.0,
                (Hyena, _) => 0.95,
                _ => 0.8,
            },
            tempo: match (body.species, body.body_type) {
                (Boar, _) => 1.1,
                (Cat, _) => 1.1,
                (Quokka, _) => 1.2,
                (Hyena, _) => 1.1,
                (Rabbit, _) => 1.15,
                (Frog, _) => 1.15,
                (Rat, _) => 1.0,
                (Axolotl, _) => 1.2,
                (Gecko, _) => 1.1,
                (Turtle, _) => 3.0,
                (Squirrel, _) => 1.15,
                (Porcupine, _) => 1.2,
                (Beaver, _) => 1.2,
                (Hare, _) => 1.15,
                (Seal, _) => 2.5,
                (TreantSapling, _) => 3.0,
                (MossySnail, _) => 0.5,
                _ => 1.0,
            },
            maximize: match (body.species, body.body_type) {
                (Fox, _) => 1.3,
                (Sheep, _) => 1.1,
                (Boar, _) => 1.4,
                (Jackalope, _) => 1.2,
                (Hyena, _) => 1.4,
                (Rabbit, _) => 1.3,
                (Frog, _) => 1.3,
                (Axolotl, _) => 0.9,
                (Turtle, _) => 0.8,
                (Fungome, _) => 0.7,
                (Hare, _) => 1.3,
                _ => 1.0,
            },
            minimize: match (body.species, body.body_type) {
                (Pig, _) => 0.6,
                (Fox, _) => 1.3,
                (Sheep, _) => 0.8,
                (Jackalope, _) => 0.8,
                (Skunk, _) => 0.9,
                (Cat, _) => 0.8,
                (Quokka, _) => 0.9,
                (Holladon, _) => 0.7,
                (Hyena, _) => 1.4,
                (Rabbit, _) => 0.8,
                (Frog, _) => 0.8,
                (Turtle, _) => 0.8,
                (Fungome, _) => 0.4,
                (Porcupine, _) => 0.9,
                (Beaver, _) => 0.9,
                (Hare, _) => 0.8,
                (Goat, _) => 0.8,
                (Seal, _) => 0.7,
                (TreantSapling, _) => 0.7,
                _ => 1.0,
            },
            spring: match (body.species, body.body_type) {
                (Sheep, _) => 1.2,
                (Boar, _) => 0.8,
                (Jackalope, _) => 2.2,
                (Cat, _) => 1.4,
                (Batfox, _) => 1.1,
                (Raccoon, _) => 1.1,
                (Quokka, _) => 1.3,
                (Holladon, _) => 0.7,
                (Hyena, _) => 1.4,
                (Rabbit, _) => 2.5,
                (Truffler, _) => 0.8,
                (Frog, _) => 2.5,
                (Axolotl, _) => 0.8,
                (Gecko, _) => 0.6,
                (Turtle, _) => 0.7,
                (Fungome, _) => 0.8,
                (Porcupine, _) => 1.3,
                (Beaver, _) => 1.3,
                (Hare, Male) => 2.2,
                (Hare, Female) => 2.5,
                (Goat, _) => 1.2,
                (Seal, _) => 0.7,
                (TreantSapling, _) => 0.5,
                _ => 1.0,
            },
            feed: match (body.species, body.body_type) {
                (Boar, _) => 0.6,
                (Skunk, _) => 0.8,
                (Batfox, _) => 0.7,
                (Raccoon, _) => 0.8,
                (Rabbit, _) => 1.2,
                (Truffler, _) => 0.6,
                (Frog, _) => 0.7,
                (Axolotl, _) => 0.8,
                (Gecko, _) => 0.8,
                (Turtle, _) => 0.5,
                (Fungome, _) => 0.7,
                (Hare, _) => 1.2,
                _ => 1.0,
            },
            lateral: match (body.species, body.body_type) {
                (Axolotl, _) => 1.0,
                (Gecko, _) => 1.0,
                (Turtle, _) => 1.0,
                (Fungome, _) => 1.0,
                (TreantSapling, _) => 1.0,
                _ => 0.0,
            },
        }
    }
}

pub fn mount_mat(
    computed_skeleton: &ComputedQuadrupedSmallSkeleton,
    skeleton: &QuadrupedSmallSkeleton,
) -> (Mat4<f32>, Quaternion<f32>) {
    (computed_skeleton.chest, skeleton.chest.orientation)
}

pub fn mount_transform(
    body: &Body,
    computed_skeleton: &ComputedQuadrupedSmallSkeleton,
    skeleton: &QuadrupedSmallSkeleton,
) -> Transform<f32, f32, f32> {
    use comp::quadruped_small::{BodyType::*, Species::*};

    let mount_point = match (body.species, body.body_type) {
        (Pig, _) => (0.0, 1.0, 4.0),
        (Fox, _) => (0.0, 0.0, 2.5),
        (Sheep, _) => (0.0, -1.0, 3.5),
        (Boar, _) => (0.0, -2.0, 3.5),
        (Jackalope, _) => (0.0, -1.0, 3.5),
        (Skunk, _) => (0.0, -1.0, 3.0),
        (Cat, _) => (0.0, -1.0, 2.0),
        (Batfox, _) => (0.0, 0.0, 3.0),
        (Raccoon, _) => (0.0, 0.0, 3.5),
        (Quokka, _) => (0.0, -1.0, 4.0),
        (Goat, _) => (0.0, 0.0, 2.5),
        (Holladon, _) => (0.0, -2.0, 2.0),
        (Hyena, _) => (0.0, -4.0, 2.5),
        (Rabbit, _) => (0.0, 0.0, 3.0),
        (Truffler, _) => (0.0, -5.5, 10.0),
        (Frog, _) => (0.0, 0.0, 3.0),
        (Rat, _) => (0.0, 0.5, 3.5),
        (Axolotl, _) => (0.0, -1.0, 1.5),
        (Gecko, _) => (0.0, -1.0, 1.5),
        (Turtle, _) => (0.0, -4.0, 3.0),
        (Squirrel, _) => (0.0, 0.0, 2.5),
        (Fungome, _) => (0.0, -4.0, 3.0),
        (Porcupine, _) => (0.0, 7.0, 2.0),
        (Beaver, _) => (0.0, 0.0, 4.0),
        (Hare, Male) => (0.0, -2.0, 3.0),
        (Hare, Female) => (0.0, -2.0, 2.0),
        (Dog, _) => (0.0, -2.0, 2.5),
        (Seal, _) => (0.0, 0.0, 3.0),
        (TreantSapling, _) => (0.0, -4.0, 1.5),
        (MossySnail, _) => (0.0, -2.0, 6.5),
    }
    .into();

    let (mount_mat, orientation) = mount_mat(computed_skeleton, skeleton);
    Transform {
        position: mount_mat.mul_point(mount_point),
        orientation,
        scale: Vec3::one(),
    }
}
