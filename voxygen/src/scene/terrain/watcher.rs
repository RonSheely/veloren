use crate::hud::CraftingTab;
use common::{
    terrain::{Block, BlockKind, SpriteKind, sprite},
    vol::ReadVol,
};
use common_base::span;
use rand::prelude::*;
use rand_chacha::ChaCha8Rng;
use vek::*;

#[derive(Clone, Copy, Debug)]
pub enum Interaction {
    /// This covers mining, unlocking, and regular collectable things (e.g.
    /// twigs).
    Collect,
    Craft(CraftingTab),
    Mount,
    Read,
    LightToggle(bool),
}

#[derive(Copy, Clone)]
pub enum FireplaceType {
    House,
    Workshop, // this also includes witch hut
}

pub struct SmokerProperties {
    pub position: Vec3<i32>,
    pub kind: FireplaceType,
}

impl SmokerProperties {
    fn new(position: Vec3<i32>, kind: FireplaceType) -> Self { Self { position, kind } }
}

#[derive(Default)]
pub struct BlocksOfInterest {
    pub leaves: Vec<Vec3<i32>>,
    pub drip: Vec<Vec3<i32>>,
    pub grass: Vec<Vec3<i32>>,
    pub slow_river: Vec<Vec3<i32>>,
    pub fast_river: Vec<Vec3<i32>>,
    pub waterfall: Vec<(Vec3<i32>, Vec3<f32>)>,
    pub lavapool: Vec<Vec3<i32>>,
    pub fires: Vec<Vec3<i32>>,
    pub smokers: Vec<SmokerProperties>,
    pub beehives: Vec<Vec3<i32>>,
    pub reeds: Vec<Vec3<i32>>,
    pub fireflies: Vec<Vec3<i32>>,
    pub flowers: Vec<Vec3<i32>>,
    pub fire_bowls: Vec<Vec3<i32>>,
    pub snow: Vec<Vec3<i32>>,
    pub spores: Vec<Vec3<i32>>,
    //This is so crickets stay in place and don't randomly change sounds
    pub cricket1: Vec<Vec3<i32>>,
    pub cricket2: Vec<Vec3<i32>>,
    pub cricket3: Vec<Vec3<i32>>,
    pub frogs: Vec<Vec3<i32>>,
    pub one_way_walls: Vec<(Vec3<i32>, Vec3<f32>)>,
    // Note: these are only needed for chunks within the iteraction range so this is a potential
    // area for optimization
    pub interactables: Vec<(Vec3<i32>, Interaction)>,
    pub lights: Vec<(Vec3<i32>, u8)>,
    pub train_smokes: Vec<Vec3<i32>>,
    // needed for biome specific smoke variations
    pub temperature: f32,
    pub humidity: f32,
}

impl BlocksOfInterest {
    pub fn from_blocks(
        blocks: impl Iterator<Item = (Vec3<i32>, Block)>,
        river_velocity: Vec3<f32>,
        temperature: f32,
        humidity: f32,
        chunk: &impl ReadVol<Vox = Block>,
    ) -> Self {
        span!(_guard, "from_chunk", "BlocksOfInterest::from_chunk");
        let mut leaves = Vec::new();
        let mut drip = Vec::new();
        let mut grass = Vec::new();
        let mut slow_river = Vec::new();
        let mut fast_river = Vec::new();
        let mut waterfall = Vec::new();
        let mut lavapool = Vec::new();
        let mut fires = Vec::new();
        let mut smokers = Vec::new();
        let mut beehives = Vec::new();
        let mut reeds = Vec::new();
        let mut fireflies = Vec::new();
        let mut flowers = Vec::new();
        let mut interactables = Vec::new();
        let mut lights = Vec::new();
        // Lights that can be omitted at random if we have too many and need to cull
        // some of them
        let mut minor_lights = Vec::new();
        let mut fire_bowls = Vec::new();
        let mut snow = Vec::new();
        let mut cricket1 = Vec::new();
        let mut cricket2 = Vec::new();
        let mut cricket3 = Vec::new();
        let mut frogs = Vec::new();
        let mut one_way_walls = Vec::new();
        let mut spores = Vec::new();
        let mut train_smokes = Vec::new();

        let mut rng = ChaCha8Rng::from_seed(thread_rng().gen());

        blocks.for_each(|(pos, block)| {
            match block.kind() {
                BlockKind::Leaves
                    if rng.gen_range(0..16) == 0
                        && chunk
                            .get(pos - Vec3::unit_z())
                            .map_or(true, |b| !b.is_filled()) =>
                {
                    leaves.push(pos)
                },
                BlockKind::WeakRock if rng.gen_range(0..6) == 0 => drip.push(pos),
                BlockKind::Grass => {
                    if rng.gen_range(0..16) == 0 {
                        grass.push(pos);
                    }
                    match rng.gen_range(0..8192) {
                        1 => cricket1.push(pos),
                        2 => cricket2.push(pos),
                        3 => cricket3.push(pos),
                        _ => {},
                    }
                },
                BlockKind::Water => {
                    let is_waterfall = chunk
                        .get(pos + vek::Vec3::unit_z())
                        .is_ok_and(|b| b.is_air())
                        && [
                            vek::Vec2::new(0, 1),
                            vek::Vec2::new(1, 0),
                            vek::Vec2::new(0, -1),
                            vek::Vec2::new(-1, 0),
                        ]
                        .iter()
                        .map(|p| {
                            (1..=2)
                                .take_while(|i| {
                                    chunk.get(pos + p.with_z(*i)).is_ok_and(|b| b.is_liquid())
                                })
                                .count()
                        })
                        .any(|s| s >= 2);

                    if is_waterfall {
                        waterfall.push((pos, river_velocity));
                    }

                    let river_speed_sq = river_velocity.magnitude_squared();
                    // Assign a river speed to water blocks depending on river velocity
                    if is_waterfall || river_speed_sq > 0.9_f32.powi(2) {
                        fast_river.push(pos)
                    } else if river_speed_sq > 0.3_f32.powi(2) {
                        slow_river.push(pos)
                    }
                },
                BlockKind::Snow if rng.gen_range(0..16) == 0 => snow.push(pos),
                BlockKind::Lava
                    if chunk
                        .get(pos + Vec3::unit_z())
                        .map_or(true, |b| !b.is_filled()) =>
                {
                    if rng.gen_range(0..5) == 0 {
                        fires.push(pos + Vec3::unit_z())
                    }
                    if rng.gen_range(0..16) == 0 {
                        lavapool.push(pos)
                    }
                },
                BlockKind::GlowingMushroom if rng.gen_range(0..8) == 0 => spores.push(pos),
                BlockKind::Snow | BlockKind::Ice if rng.gen_range(0..16) == 0 => snow.push(pos),
                _ => {
                    if let Some(sprite) = block.get_sprite() {
                        if sprite.category() == sprite::Category::Lamp {
                            if let Ok(sprite::LightEnabled(enabled)) = block.get_attr() {
                                interactables.push((pos, Interaction::LightToggle(!enabled)));
                            }
                        }

                        if block.is_mountable() {
                            interactables.push((pos, Interaction::Mount));
                        }

                        match sprite {
                            SpriteKind::Ember => {
                                fires.push(pos);
                                smokers.push(SmokerProperties::new(pos, FireplaceType::House));
                            },
                            SpriteKind::TrainSmoke => {
                                train_smokes.push(pos);
                            },
                            SpriteKind::FireBlock => {
                                fire_bowls.push(pos);
                            },
                            // Offset positions to account for block height.
                            // TODO: Is this a good idea?
                            SpriteKind::StreetLamp => fire_bowls.push(pos + Vec3::unit_z() * 2),
                            SpriteKind::FireBowlGround => fire_bowls.push(pos + Vec3::unit_z()),
                            SpriteKind::StreetLampTall => fire_bowls.push(pos + Vec3::unit_z() * 4),
                            SpriteKind::WallSconce => fire_bowls.push(pos + Vec3::unit_z()),
                            SpriteKind::Beehive => beehives.push(pos),
                            SpriteKind::Reed => {
                                reeds.push(pos);
                                fireflies.push(pos);
                                if rng.gen_range(0..12) == 0 {
                                    frogs.push(pos);
                                }
                            },
                            SpriteKind::CaveMushroom => fireflies.push(pos),
                            SpriteKind::PinkFlower => flowers.push(pos),
                            SpriteKind::PurpleFlower => flowers.push(pos),
                            SpriteKind::RedFlower => flowers.push(pos),
                            SpriteKind::WhiteFlower => flowers.push(pos),
                            SpriteKind::YellowFlower => flowers.push(pos),
                            SpriteKind::Sunflower => flowers.push(pos),
                            SpriteKind::CraftingBench => {
                                interactables.push((pos, Interaction::Craft(CraftingTab::All)))
                            },
                            SpriteKind::SmokeDummy => {
                                smokers.push(SmokerProperties::new(pos, FireplaceType::Workshop));
                            },
                            SpriteKind::Forge => interactables
                                .push((pos, Interaction::Craft(CraftingTab::ProcessedMaterial))),
                            SpriteKind::TanningRack => interactables
                                .push((pos, Interaction::Craft(CraftingTab::ProcessedMaterial))),
                            SpriteKind::SpinningWheel => {
                                interactables.push((pos, Interaction::Craft(CraftingTab::All)))
                            },
                            SpriteKind::Loom => {
                                interactables.push((pos, Interaction::Craft(CraftingTab::All)))
                            },
                            SpriteKind::Cauldron => {
                                fires.push(pos);
                                interactables.push((pos, Interaction::Craft(CraftingTab::Potion)))
                            },
                            SpriteKind::Anvil => {
                                interactables.push((pos, Interaction::Craft(CraftingTab::Weapon)))
                            },
                            SpriteKind::CookingPot => {
                                fires.push(pos);
                                interactables.push((pos, Interaction::Craft(CraftingTab::Food)))
                            },
                            SpriteKind::DismantlingBench => {
                                fires.push(pos);
                                interactables
                                    .push((pos, Interaction::Craft(CraftingTab::Dismantle)))
                            },
                            SpriteKind::RepairBench => {
                                interactables.push((pos, Interaction::Craft(CraftingTab::All)))
                            },
                            SpriteKind::OneWayWall => one_way_walls.push((
                                pos,
                                Vec2::unit_y()
                                    .rotated_z(
                                        std::f32::consts::PI
                                            * 0.25
                                            * block
                                                .get_attr::<sprite::Ori>()
                                                .unwrap_or(sprite::Ori(0))
                                                .0
                                                as f32,
                                    )
                                    .with_z(0.0),
                            )),
                            SpriteKind::Sign | SpriteKind::HangingSign => {
                                interactables.push((pos, Interaction::Read))
                            },
                            SpriteKind::MycelBlue => spores.push(pos),
                            SpriteKind::Mold => spores.push(pos),
                            _ => {},
                        }
                    }
                },
            }
            // NOTE: we don't care whether it requires mine-tool or not here
            if block.default_tool().is_some() {
                interactables.push((pos, Interaction::Collect));
            }
            if let Some(glow) = block.get_glow() {
                // Currently, we count filled blocks as 'minor' lights, and sprites as
                // non-minor.
                if block.get_sprite().is_none() {
                    minor_lights.push((pos, glow));
                } else {
                    lights.push((pos, glow));
                }
            }
        });

        // TODO: Come up with a better way to prune many light sources: grouping them
        // into larger lights with k-means clustering, perhaps?
        const MAX_MINOR_LIGHTS: usize = 64;
        lights.extend(
            minor_lights
                .choose_multiple(&mut rng, MAX_MINOR_LIGHTS)
                .copied(),
        );

        Self {
            leaves,
            drip,
            grass,
            slow_river,
            fast_river,
            waterfall,
            lavapool,
            fires,
            smokers,
            beehives,
            reeds,
            fireflies,
            flowers,
            fire_bowls,
            snow,
            spores,
            cricket1,
            cricket2,
            cricket3,
            frogs,
            one_way_walls,
            interactables,
            lights,
            temperature,
            humidity,
            train_smokes,
        }
    }
}
