use crate::comp::skillset::{
    SKILL_GROUP_LOOKUP, SKILL_MAX_LEVEL, SKILL_PREREQUISITES, SkillGroupKind, SkillPrerequisite,
};
use serde::{Deserialize, Serialize};

/// Represents a skill that a player can unlock, that either grants them some
/// kind of active ability, or a passive effect etc. Obviously because this is
/// an enum it doesn't describe what the skill actually -does-, this will be
/// handled by dedicated ECS systems.
// NOTE: if skill does use some constant, add it to corresponding
// SkillTree Modifiers below.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize, Deserialize, Ord, PartialOrd)]
pub enum Skill {
    Sword(SwordSkill),
    Axe(AxeSkill),
    Hammer(HammerSkill),
    Bow(BowSkill),
    Staff(StaffSkill),
    Sceptre(SceptreSkill),
    Climb(ClimbSkill),
    Swim(SwimSkill),
    Pick(MiningSkill),
    UnlockGroup(SkillGroupKind),
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize, Deserialize, Ord, PartialOrd)]
pub enum SwordSkill {
    CrescentSlash,
    FellStrike,
    Skewer,
    Cascade,
    CrossCut,
    Finisher,
    HeavySweep,
    HeavyPommelStrike,
    HeavyFortitude,
    HeavyPillarThrust,
    AgileQuickDraw,
    AgileFeint,
    AgileDancingEdge,
    AgileFlurry,
    DefensiveRiposte,
    DefensiveDisengage,
    DefensiveDeflect,
    DefensiveStalwartSword,
    CripplingGouge,
    CripplingHamstring,
    CripplingBloodyGash,
    CripplingEviscerate,
    CleavingWhirlwindSlice,
    CleavingEarthSplitter,
    CleavingSkySplitter,
    CleavingBladeFever,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize, Deserialize, Ord, PartialOrd)]
pub enum AxeSkill {
    BrutalSwing,
    Berserk,
    RisingTide,
    SavageSense,
    AdrenalineRush,
    Execute,
    Maelstrom,
    Rake,
    Bloodfeast,
    FierceRaze,
    Furor,
    Fracture,
    Lacerate,
    Riptide,
    SkullBash,
    Sunder,
    Plunder,
    Defiance,
    Keelhaul,
    Bulkhead,
    Capsize,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize, Deserialize, Ord, PartialOrd)]
pub enum HammerSkill {
    ScornfulSwipe,
    Tremor,
    VigorousBash,
    Retaliate,
    SpineCracker,
    Breach,
    IronTempest,
    Upheaval,
    Thunderclap,
    SeismicShock,
    HeavyWhorl,
    Intercept,
    PileDriver,
    LungPummel,
    HelmCrusher,
    Rampart,
    Tenacity,
    Earthshaker,
    Judgement,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize, Deserialize, Ord, PartialOrd)]
pub enum BowSkill {
    // Passives
    ProjSpeed,
    // Charged upgrades
    CDamage,
    CRegen,
    CKnockback,
    CSpeed,
    CMove,
    // Repeater upgrades
    RDamage,
    RCost,
    RSpeed,
    // Shotgun upgrades
    UnlockShotgun,
    SDamage,
    SCost,
    SArrows,
    SSpread,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize, Deserialize, Ord, PartialOrd)]
pub enum StaffSkill {
    // Basic ranged upgrades
    BDamage,
    BRegen,
    BRadius,
    // Flamethrower upgrades
    FDamage,
    FRange,
    FDrain,
    FVelocity,
    // Shockwave upgrades
    UnlockShockwave,
    SDamage,
    SKnockback,
    SRange,
    SCost,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize, Deserialize, Ord, PartialOrd)]
pub enum SceptreSkill {
    // Lifesteal beam upgrades
    LDamage,
    LRange,
    LLifesteal,
    LRegen,
    // Healing aura upgrades
    HHeal,
    HRange,
    HDuration,
    HCost,
    // Warding aura upgrades
    UnlockAura,
    AStrength,
    ADuration,
    ARange,
    ACost,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize, Deserialize, Ord, PartialOrd)]
pub enum ClimbSkill {
    Cost,
    Speed,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize, Deserialize, Ord, PartialOrd)]
pub enum SwimSkill {
    Speed,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize, Deserialize, Ord, PartialOrd)]
pub enum MiningSkill {
    Speed,
    OreGain,
    GemGain,
}

impl Skill {
    /// Is unable to detect cyclic dependencies, so ensure that there are no
    /// cycles if you modify the prerequisite map.
    pub fn prerequisite_skills(&self) -> Option<&SkillPrerequisite> {
        SKILL_PREREQUISITES.get(self)
    }

    /// Returns the cost in skill points of unlocking a particular skill
    pub fn skill_cost(&self, level: u16) -> u16 { level }

    /// Returns the maximum level a skill can reach, returns None if the skill
    /// doesn't level
    pub fn max_level(&self) -> u16 { SKILL_MAX_LEVEL.get(self).copied().unwrap_or(1) }

    /// Returns the skill group type for a skill from the static skill group
    /// definitions.
    pub fn skill_group_kind(&self) -> Option<SkillGroupKind> {
        SKILL_GROUP_LOOKUP.get(self).copied()
    }
}

/// Tree of modifiers that represent how stats are
/// changed per each skill level.
///
/// It's used as bridge between ECS systems
/// and voxygen Diary for skill descriptions and helps to sync them.
///
/// NOTE: Just adding constant does nothing, you need to use it in both
/// ECS systems and Diary.
// TODO: make it lazy_static and move to .ron?
pub const SKILL_MODIFIERS: SkillTreeModifiers = SkillTreeModifiers::get();

pub struct SkillTreeModifiers {
    pub bow_tree: BowTreeModifiers,
    pub staff_tree: StaffTreeModifiers,
    pub sceptre_tree: SceptreTreeModifiers,
    pub mining_tree: MiningTreeModifiers,
    pub general_tree: GeneralTreeModifiers,
}

impl SkillTreeModifiers {
    const fn get() -> Self {
        Self {
            bow_tree: BowTreeModifiers::get(),
            staff_tree: StaffTreeModifiers::get(),
            sceptre_tree: SceptreTreeModifiers::get(),
            mining_tree: MiningTreeModifiers::get(),
            general_tree: GeneralTreeModifiers::get(),
        }
    }
}

pub struct BowTreeModifiers {
    pub universal: BowUniversalModifiers,
    pub charged: BowChargedModifiers,
    pub repeater: BowRepeaterModifiers,
    pub shotgun: BowShotgunModifiers,
}

pub struct BowUniversalModifiers {
    // TODO: split per abilities?
    pub projectile_speed: f32,
}

pub struct BowChargedModifiers {
    pub damage_scaling: f32,
    pub regen_scaling: f32,
    pub knockback_scaling: f32,
    pub charge_rate: f32,
    pub move_speed: f32,
}

pub struct BowRepeaterModifiers {
    pub power: f32,
    pub energy_cost: f32,
    pub max_speed: f32,
}

pub struct BowShotgunModifiers {
    pub power: f32,
    pub energy_cost: f32,
    pub num_projectiles: u32,
    pub spread: f32,
}

impl BowTreeModifiers {
    const fn get() -> Self {
        Self {
            universal: BowUniversalModifiers {
                projectile_speed: 1.05,
            },
            charged: BowChargedModifiers {
                damage_scaling: 1.05,
                regen_scaling: 1.05,
                knockback_scaling: 1.05,
                charge_rate: 1.05,
                move_speed: 1.05,
            },
            repeater: BowRepeaterModifiers {
                power: 1.05,
                energy_cost: 0.95,
                max_speed: 1.1,
            },
            shotgun: BowShotgunModifiers {
                power: 1.05,
                energy_cost: 0.95,
                num_projectiles: 1,
                spread: 0.95,
            },
        }
    }
}

pub struct StaffTreeModifiers {
    pub fireball: StaffFireballModifiers,
    pub flamethrower: StaffFlamethrowerModifiers,
    pub shockwave: StaffShockwaveModifiers,
}

pub struct StaffFireballModifiers {
    pub power: f32,
    pub regen: f32,
    pub range: f32,
}

pub struct StaffFlamethrowerModifiers {
    pub damage: f32,
    pub range: f32,
    pub energy_drain: f32,
    pub velocity: f32,
}

pub struct StaffShockwaveModifiers {
    pub damage: f32,
    pub knockback: f32,
    pub duration: f32,
    pub energy_cost: f32,
}

impl StaffTreeModifiers {
    const fn get() -> Self {
        Self {
            fireball: StaffFireballModifiers {
                power: 1.05,
                regen: 1.05,
                range: 1.05,
            },
            flamethrower: StaffFlamethrowerModifiers {
                damage: 1.1,
                range: 1.05,
                energy_drain: 0.95,
                velocity: 1.05,
            },
            shockwave: StaffShockwaveModifiers {
                damage: 1.1,
                knockback: 1.05,
                duration: 1.05,
                energy_cost: 0.95,
            },
        }
    }
}

pub struct SceptreTreeModifiers {
    pub beam: SceptreBeamModifiers,
    pub healing_aura: SceptreHealingAuraModifiers,
    pub warding_aura: SceptreWardingAuraModifiers,
}

pub struct SceptreBeamModifiers {
    pub damage: f32,
    pub range: f32,
    pub energy_regen: f32,
    pub lifesteal: f32,
}

pub struct SceptreHealingAuraModifiers {
    pub strength: f32,
    pub duration: f32,
    pub range: f32,
    pub energy_cost: f32,
}

pub struct SceptreWardingAuraModifiers {
    pub strength: f32,
    pub duration: f32,
    pub range: f32,
    pub energy_cost: f32,
}

impl SceptreTreeModifiers {
    const fn get() -> Self {
        Self {
            beam: SceptreBeamModifiers {
                damage: 1.05,
                range: 1.05,
                energy_regen: 1.05,
                lifesteal: 1.05,
            },
            healing_aura: SceptreHealingAuraModifiers {
                strength: 1.05,
                duration: 1.05,
                range: 1.05,
                energy_cost: 0.95,
            },
            warding_aura: SceptreWardingAuraModifiers {
                strength: 1.05,
                duration: 1.05,
                range: 1.05,
                energy_cost: 0.95,
            },
        }
    }
}

pub struct MiningTreeModifiers {
    pub speed: f32,
    pub gem_gain: f32,
    pub ore_gain: f32,
}

impl MiningTreeModifiers {
    const fn get() -> Self {
        Self {
            speed: 1.1,
            gem_gain: 0.1,
            ore_gain: 0.1,
        }
    }
}

pub struct GeneralTreeModifiers {
    pub swim: SwimTreeModifiers,
    pub climb: ClimbTreeModifiers,
}

pub struct SwimTreeModifiers {
    pub speed: f32,
}

pub struct ClimbTreeModifiers {
    pub energy_cost: f32,
    pub speed: f32,
}

impl GeneralTreeModifiers {
    const fn get() -> Self {
        Self {
            swim: SwimTreeModifiers { speed: 1.25 },
            climb: ClimbTreeModifiers {
                energy_cost: 0.8,
                speed: 1.2,
            },
        }
    }
}
