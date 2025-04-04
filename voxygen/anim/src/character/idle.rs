use super::{
    super::{Animation, vek::*},
    CharacterSkeleton, SkeletonAttr,
};
use common::comp::item::{Hands, ToolKind};
use std::ops::Mul;

pub struct IdleAnimation;

impl Animation for IdleAnimation {
    type Dependency<'a> = (
        Option<ToolKind>,
        Option<ToolKind>,
        (Option<Hands>, Option<Hands>),
        f32,
    );
    type Skeleton = CharacterSkeleton;

    #[cfg(feature = "use-dyn-lib")]
    const UPDATE_FN: &'static [u8] = b"character_idle\0";

    #[cfg_attr(feature = "be-dyn-lib", unsafe(export_name = "character_idle"))]
    fn update_skeleton_inner(
        skeleton: &Self::Skeleton,
        (active_tool_kind, second_tool_kind, hands, global_time): Self::Dependency<'_>,
        anim_time: f32,
        _rate: &mut f32,
        s_a: &SkeletonAttr,
    ) -> Self::Skeleton {
        let mut next = (*skeleton).clone();

        let slow = (anim_time * 1.0).sin();
        let head_look = Vec2::new(
            (global_time + anim_time / 12.0).floor().mul(7331.0).sin() * 0.1,
            (global_time + anim_time / 12.0).floor().mul(1337.0).sin() * 0.05,
        );
        next.head.scale = Vec3::one() * s_a.head_scale;
        next.chest.scale = Vec3::one() * 1.01;
        next.hand_l.scale = Vec3::one() * 1.04;
        next.hand_r.scale = Vec3::one() * 1.04;
        next.back.scale = Vec3::one() * 1.02;
        next.hold.scale = Vec3::one() * 0.0;
        next.shoulder_l.scale = Vec3::one() * 1.1;
        next.shoulder_r.scale = Vec3::one() * 1.1;

        next.head.position = Vec3::new(0.0, s_a.head.0, s_a.head.1 + slow * 0.3);
        next.head.orientation =
            Quaternion::rotation_z(head_look.x) * Quaternion::rotation_x(head_look.y.abs());

        next.chest.position = Vec3::new(0.0, s_a.chest.0, s_a.chest.1 + slow * 0.3);
        next.chest.orientation = Quaternion::rotation_z(head_look.x * 0.6);

        next.belt.position = Vec3::new(0.0, s_a.belt.0, s_a.belt.1);
        next.belt.orientation = Quaternion::rotation_z(head_look.x * -0.1);

        next.back.position = Vec3::new(0.0, s_a.back.0, s_a.back.1);

        next.shorts.position = Vec3::new(0.0, s_a.shorts.0, s_a.shorts.1);
        next.shorts.orientation = Quaternion::rotation_z(head_look.x * -0.2);

        next.hand_l.position = Vec3::new(
            -s_a.hand.0,
            s_a.hand.1 + slow * 0.15,
            s_a.hand.2 + slow * 0.5,
        );

        next.hand_l.orientation = Quaternion::rotation_x(slow * -0.06);

        next.hand_r.position = Vec3::new(
            s_a.hand.0,
            s_a.hand.1 + slow * 0.15,
            s_a.hand.2 + slow * 0.5,
        );
        next.hand_r.orientation = Quaternion::rotation_x(slow * -0.06);

        next.foot_l.position = Vec3::new(-s_a.foot.0, s_a.foot.1, s_a.foot.2);

        next.foot_r.position = Vec3::new(s_a.foot.0, s_a.foot.1, s_a.foot.2);

        next.shoulder_l.position = Vec3::new(-s_a.shoulder.0, s_a.shoulder.1, s_a.shoulder.2);

        next.shoulder_r.position = Vec3::new(s_a.shoulder.0, s_a.shoulder.1, s_a.shoulder.2);

        next.glider.position = Vec3::new(0.0, 0.0, 10.0);
        next.glider.scale = Vec3::one() * 0.0;
        next.hold.position = Vec3::new(0.4, -0.3, -5.8);

        next.do_tools_on_back(hands, active_tool_kind, second_tool_kind);

        next.do_hold_lantern(s_a, anim_time, 0.0, 0.0, 0.0, 0.0);

        next.torso.position = Vec3::new(0.0, 0.0, 0.0);

        next
    }
}
