use super::{
    super::{Animation, vek::*},
    CharacterSkeleton, SkeletonAttr,
};
use common::comp::item::ToolKind;
use core::{f32::consts::PI, ops::Mul};

pub struct SneakEquipAnimation;

impl Animation for SneakEquipAnimation {
    type Dependency<'a> = (Option<ToolKind>, Vec3<f32>, Vec3<f32>, Vec3<f32>, f32);
    type Skeleton = CharacterSkeleton;

    #[cfg(feature = "use-dyn-lib")]
    const UPDATE_FN: &'static [u8] = b"character_sneakequip\0";

    #[cfg_attr(feature = "be-dyn-lib", unsafe(export_name = "character_sneakequip"))]
    fn update_skeleton_inner(
        skeleton: &Self::Skeleton,
        (active_tool_kind, velocity, orientation, last_ori, global_time): Self::Dependency<'_>,
        anim_time: f32,
        rate: &mut f32,
        s_a: &SkeletonAttr,
    ) -> Self::Skeleton {
        let mut next = (*skeleton).clone();
        let speed = Vec2::<f32>::from(velocity).magnitude();
        *rate = 1.0;
        let slow = (anim_time * 3.0).sin();
        let breathe = ((anim_time * 0.5).sin()).abs();
        let walkintensity = if speed > 5.0 { 1.0 } else { 0.45 };
        let lower = if speed > 5.0 { 0.0 } else { 1.0 };
        let _snapfoot = if speed > 5.0 { 1.1 } else { 2.0 };
        let lab: f32 = 1.0;
        let foothoril = (anim_time * 7.0 * lab + PI * 1.45).sin();
        let foothorir = (anim_time * 7.0 * lab + PI * (0.45)).sin();

        let footvertl = (anim_time * 7.0 * lab).sin();
        let footvertr = (anim_time * 7.0 * lab + PI).sin();

        let footrotl = ((5.0 / (2.5 + (2.5) * ((anim_time * 7.0 * lab + PI * 1.4).sin()).powi(2)))
            .sqrt())
            * ((anim_time * 7.0 * lab + PI * 1.4).sin());

        let footrotr = ((5.0 / (1.0 + (4.0) * ((anim_time * 7.0 * lab + PI * 0.4).sin()).powi(2)))
            .sqrt())
            * ((anim_time * 7.0 * lab + PI * 0.4).sin());

        let short = (anim_time * lab * 7.0).sin();
        let noisea = (anim_time * 11.0 + PI / 6.0).sin();
        let noiseb = (anim_time * 19.0 + PI / 4.0).sin();

        let shorte = ((5.0 / (4.0 + 1.0 * ((anim_time * lab * 7.0).sin()).powi(2))).sqrt())
            * ((anim_time * lab * 7.0).sin());

        let shortalt = (anim_time * lab * 7.0 + PI / 2.0).sin();

        let head_look = Vec2::new(
            (global_time + anim_time / 18.0).floor().mul(7331.0).sin() * 0.2,
            (global_time + anim_time / 18.0).floor().mul(1337.0).sin() * 0.1,
        );

        let orientation: Vec2<f32> = Vec2::from(orientation);
        let last_ori = Vec2::from(last_ori);
        let tilt = if vek::Vec2::new(orientation, last_ori)
            .map(|o| o.magnitude_squared())
            .map(|m| m > 0.001 && m.is_finite())
            .reduce_and()
            && orientation.angle_between(last_ori).is_finite()
        {
            orientation.angle_between(last_ori).min(0.2)
                * last_ori.determine_side(Vec2::zero(), orientation).signum()
        } else {
            0.0
        } * 1.3;
        next.hold.scale = Vec3::one() * 0.0;

        if speed > 0.5 {
            next.hand_l.position = Vec3::new(1.0 - s_a.hand.0, 4.0 + s_a.hand.1, 1.0 + s_a.hand.2);
            next.hand_l.orientation = Quaternion::rotation_x(1.0);

            next.hand_r.position = Vec3::new(-1.0 + s_a.hand.0, -1.0 + s_a.hand.1, s_a.hand.2);
            next.hand_r.orientation = Quaternion::rotation_x(0.4);
            next.head.position = Vec3::new(0.0, 1.0 + s_a.head.0, -1.0 + s_a.head.1 + short * 0.06);
            next.head.orientation =
                Quaternion::rotation_z(tilt * -2.5 + head_look.x * 0.2 - short * 0.06)
                    * Quaternion::rotation_x(head_look.y + 0.45);

            next.chest.position = Vec3::new(0.0, s_a.chest.0, -1.0 + s_a.chest.1 + shortalt * -0.5);
            next.chest.orientation = Quaternion::rotation_z(0.3 + short * 0.08 + tilt * -0.2)
                * Quaternion::rotation_y(tilt * 0.8)
                * Quaternion::rotation_x(-0.5);

            next.belt.position = Vec3::new(0.0, 0.5 + s_a.belt.0, 0.7 + s_a.belt.1);
            next.belt.orientation = Quaternion::rotation_z(short * 0.1 + tilt * -1.1)
                * Quaternion::rotation_y(tilt * 0.5)
                * Quaternion::rotation_x(0.2);

            next.back.orientation =
                Quaternion::rotation_x(-0.25 + short * 0.1 + noisea * 0.1 + noiseb * 0.1);

            next.shorts.position = Vec3::new(0.0, 1.0 + s_a.shorts.0, 1.0 + s_a.shorts.1);
            next.shorts.orientation = Quaternion::rotation_z(short * 0.16 + tilt * -1.5)
                * Quaternion::rotation_y(tilt * 0.7)
                * Quaternion::rotation_x(0.3);

            next.foot_l.position = Vec3::new(
                -s_a.foot.0,
                s_a.foot.1 + foothoril * -10.5 * walkintensity - lower * 1.0,
                1.0 + s_a.foot.2 + ((footvertl * -1.7).max(-1.0)) * walkintensity,
            );
            next.foot_l.orientation =
                Quaternion::rotation_x(-0.2 + footrotl * -0.8 * walkintensity)
                    * Quaternion::rotation_y(tilt * 1.8);

            next.foot_r.position = Vec3::new(
                s_a.foot.0,
                s_a.foot.1 + foothorir * -10.5 * walkintensity - lower * 1.0,
                1.0 + s_a.foot.2 + ((footvertr * -1.7).max(-1.0)) * walkintensity,
            );
            next.foot_r.orientation =
                Quaternion::rotation_x(-0.2 + footrotr * -0.8 * walkintensity)
                    * Quaternion::rotation_y(tilt * 1.8);

            next.shoulder_l.orientation = Quaternion::rotation_x(short * 0.15 * walkintensity);

            next.shoulder_r.orientation = Quaternion::rotation_x(short * -0.15 * walkintensity);

            next.lantern.orientation =
                Quaternion::rotation_x(shorte * 0.2 + 0.4) * Quaternion::rotation_y(shorte * 0.1);
        } else {
            next.head.position = Vec3::new(
                0.0,
                1.0 + s_a.head.0,
                -2.0 + s_a.head.1 + slow * 0.1 + breathe * -0.05,
            );
            next.head.orientation = Quaternion::rotation_z(head_look.x)
                * Quaternion::rotation_x(0.6 + head_look.y.abs());

            next.chest.position = Vec3::new(0.0, s_a.chest.0, -3.0 + s_a.chest.1 + slow * 0.1);
            next.chest.orientation = Quaternion::rotation_x(-0.7);

            next.belt.position = Vec3::new(0.0, s_a.belt.0, s_a.belt.1);
            next.belt.orientation = Quaternion::rotation_z(0.3 + head_look.x * -0.1);

            next.hand_l.position = Vec3::new(1.0 - s_a.hand.0, 5.0 + s_a.hand.1, 0.0 + s_a.hand.2);
            next.hand_l.orientation = Quaternion::rotation_x(1.35);

            next.hand_r.position = Vec3::new(-1.0 + s_a.hand.0, s_a.hand.1, s_a.hand.2);
            next.hand_r.orientation = Quaternion::rotation_x(0.4);

            next.shorts.position = Vec3::new(0.0, s_a.shorts.0, s_a.shorts.1);
            next.shorts.orientation = Quaternion::rotation_z(0.6 + head_look.x * -0.2);

            next.foot_l.position = Vec3::new(-s_a.foot.0, -6.0 + s_a.foot.1, 1.0 + s_a.foot.2);
            next.foot_l.orientation = Quaternion::rotation_x(-0.5);

            next.foot_r.position = Vec3::new(s_a.foot.0, 4.0 + s_a.foot.1, s_a.foot.2);
        }

        if skeleton.holding_lantern {
            next.hand_r.position = Vec3::new(s_a.hand.0, s_a.hand.1 + 5.0, s_a.hand.2 + 9.0);
            next.hand_r.orientation = Quaternion::rotation_x(2.5);

            next.lantern.position = Vec3::new(0.0, 1.5, -5.5);
            next.lantern.orientation = next.hand_r.orientation.inverse();
        }
        let equip_slow = 1.0 + (anim_time * 12.0 + PI).cos();
        let equip_slowa = 1.0 + (anim_time * 12.0 + PI / 4.0).cos();
        next.hand_l.orientation = Quaternion::rotation_y(-2.3) * Quaternion::rotation_z(PI / 2.0);
        next.hand_r.orientation = Quaternion::rotation_y(-2.3) * Quaternion::rotation_z(PI / 2.0);
        next.control.position = Vec3::new(equip_slowa * -1.5, 0.0, equip_slow * 1.5);

        match active_tool_kind {
            Some(ToolKind::Sword) => {
                next.hand_l.position = Vec3::new(-8.0, -5.0, 17.0);
                next.hand_r.position = Vec3::new(-6.0, -4.5, 14.0);
            },
            Some(ToolKind::Axe) => {
                next.hand_l.position = Vec3::new(-7.0, -5.0, 17.0);
                next.hand_r.position = Vec3::new(-5.0, -4.5, 14.0);
            },
            Some(ToolKind::Hammer | ToolKind::Pick | ToolKind::Shovel) => {
                next.hand_l.position = Vec3::new(-5.0, -5.0, 13.0);
                next.hand_r.position = Vec3::new(-3.0, -4.5, 10.0);
            },
            Some(ToolKind::Staff) | Some(ToolKind::Sceptre) => {
                next.hand_l.position = Vec3::new(-3.0, -5.0, 8.0);
                next.hand_r.position = Vec3::new(-1.75, -4.5, 5.0);
            },
            Some(ToolKind::Bow) => {
                next.hand_l.position = Vec3::new(-3.0, -5.0, 9.0);
                next.hand_r.position = Vec3::new(-1.75, -4.5, 7.0);
            },
            Some(ToolKind::Instrument) => {
                next.hand_l.position = Vec3::new(-3.0, -5.0, 10.0);
            },
            _ => {},
        }
        next
    }
}
