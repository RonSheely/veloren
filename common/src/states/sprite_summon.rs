use crate::{
    comp::{CharacterState, StateUpdate, character_state::OutputEvents},
    event::{CreateSpriteEvent, LocalEvent},
    outcome::Outcome,
    spiral::Spiral2d,
    states::{
        behavior::{CharacterBehavior, JoinData},
        utils::*,
    },
    terrain::{Block, SpriteKind},
    vol::ReadVol,
};
use rand::{Rng, thread_rng};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use vek::*;

#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub enum SpriteSummonAnchor {
    #[default]
    Summoner,
    Target,
}

/// Separated out to condense update portions of character state
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StaticData {
    /// How long the state builds up for
    pub buildup_duration: Duration,
    /// How long the state is casting for
    pub cast_duration: Duration,
    /// How long the state recovers for
    pub recover_duration: Duration,
    /// What kind of sprite is created by this state
    pub sprite: SpriteKind,
    /// Duration until sprite-delete begins (in sec), randomization-range of
    /// sprite-delete-time (in sec)
    pub del_timeout: Option<(f32, f32)>,
    /// Range that sprites are created relative to the summonner
    pub summon_distance: (f32, f32),
    /// Relative to what should the sprites be summoned?
    pub anchor: SpriteSummonAnchor,
    /// Chance that sprite is not created on a particular square
    pub sparseness: f64,
    /// Angle of total coverage, centered on the forward-facing orientation
    pub angle: f32,
    /// How much we can move
    pub move_efficiency: f32,
    /// Adjusts turning rate during the attack
    pub ori_modifier: f32,
    /// Miscellaneous information about the ability
    pub ability_info: AbilityInfo,
}

#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Data {
    /// Struct containing data that does not change over the course of the
    /// character state
    pub static_data: StaticData,
    /// Timer for each stage
    pub timer: Duration,
    /// What section the character stage is in
    pub stage_section: StageSection,
    /// What radius of sprites have already been summoned
    pub achieved_radius: i32,
}

impl CharacterBehavior for Data {
    fn behavior(&self, data: &JoinData, output_events: &mut OutputEvents) -> StateUpdate {
        let mut update = StateUpdate::from(data);

        handle_orientation(data, &mut update, self.static_data.ori_modifier, None);
        handle_move(data, &mut update, self.static_data.move_efficiency);

        let target_pos = || {
            data.controller
                .queued_inputs
                .get(&self.static_data.ability_info.input)
                .or(self.static_data.ability_info.input_attr.as_ref())
                .and_then(|input| input.select_pos)
        };

        match self.stage_section {
            StageSection::Buildup => {
                if self.timer < self.static_data.buildup_duration {
                    // Build up
                    update.character = CharacterState::SpriteSummon(Data {
                        timer: tick_attack_or_default(data, self.timer, None),
                        ..*self
                    });
                    // Send local event used for frontend shenanigans
                    match self.static_data.sprite {
                        SpriteKind::Empty => output_events.emit_local(LocalEvent::CreateOutcome(
                            Outcome::TerracottaStatueCharge {
                                pos: data.pos.0 + *data.ori.look_dir() * (data.body.max_radius()),
                            },
                        )),
                        SpriteKind::FireBlock => {
                            output_events.emit_local(LocalEvent::CreateOutcome(Outcome::Charge {
                                pos: data.pos.0 + *data.ori.look_dir() * (data.body.max_radius()),
                            }))
                        },
                        _ => {},
                    }
                } else {
                    // Transitions to recover section of stage
                    update.character = CharacterState::SpriteSummon(Data {
                        timer: Duration::default(),
                        stage_section: StageSection::Action,
                        ..*self
                    });
                }
            },
            StageSection::Action => {
                if self.timer < self.static_data.cast_duration {
                    let timer_frac =
                        self.timer.as_secs_f32() / self.static_data.cast_duration.as_secs_f32();

                    let anchor_pos = match self.static_data.anchor {
                        SpriteSummonAnchor::Summoner => data.pos.0,
                        // Use the selected target position, falling back to the
                        // summoner position
                        SpriteSummonAnchor::Target => target_pos().unwrap_or(data.pos.0),
                    };
                    let achieved_radius = create_sprites(
                        data,
                        output_events,
                        self.static_data.sprite,
                        timer_frac,
                        self.static_data.summon_distance,
                        self.achieved_radius,
                        self.static_data.angle,
                        self.static_data.sparseness,
                        anchor_pos,
                        matches!(self.static_data.anchor, SpriteSummonAnchor::Target),
                        self.static_data.del_timeout,
                    );

                    update.character = CharacterState::SpriteSummon(Data {
                        timer: tick_attack_or_default(data, self.timer, None),
                        achieved_radius,
                        ..*self
                    });
                    // Send local event used for frontend shenanigans
                    match self.static_data.sprite {
                        SpriteKind::IceSpike => {
                            let summoner_pos =
                                data.pos.0 + *data.ori.look_dir() * data.body.max_radius();
                            output_events.emit_local(LocalEvent::CreateOutcome(
                                Outcome::IceCrack {
                                    pos: match self.static_data.anchor {
                                        SpriteSummonAnchor::Summoner => summoner_pos,
                                        SpriteSummonAnchor::Target => {
                                            target_pos().unwrap_or(summoner_pos)
                                        },
                                    },
                                },
                            ));
                        },
                        SpriteKind::IronSpike => {
                            output_events.emit_local(LocalEvent::CreateOutcome(Outcome::Whoosh {
                                pos: data.pos.0,
                            }));
                        },
                        SpriteKind::FireBlock => {
                            output_events.emit_local(LocalEvent::CreateOutcome(Outcome::Bleep {
                                pos: data.pos.0 + *data.ori.look_dir() * (data.body.max_radius()),
                            }));
                        },
                        _ => {},
                    }
                } else {
                    // Transitions to recover section of stage
                    update.character = CharacterState::SpriteSummon(Data {
                        timer: Duration::default(),
                        stage_section: StageSection::Recover,
                        ..*self
                    });
                }
            },
            StageSection::Recover => {
                if self.timer < self.static_data.recover_duration {
                    // Recovery
                    update.character = CharacterState::SpriteSummon(Data {
                        timer: tick_attack_or_default(
                            data,
                            self.timer,
                            Some(data.stats.recovery_speed_modifier),
                        ),
                        ..*self
                    });
                } else {
                    // Done
                    end_ability(data, &mut update);
                }
            },
            _ => {
                // If it somehow ends up in an incorrect stage section
                end_ability(data, &mut update);
            },
        }

        update
    }
}

/// Returns achieved radius
pub fn create_sprites(
    data: &JoinData,
    output_events: &mut OutputEvents,
    sprite: SpriteKind,
    timer_frac: f32,
    summon_distance: (f32, f32),
    achieved_radius: i32,
    angle: f32,
    sparseness: f64,
    anchor_pos: Vec3<f32>,
    stack_sprites: bool,
    del_timeout: Option<(f32, f32)>,
) -> i32 {
    // Determines distance from summoner sprites should be created. Goes outward
    // with time.
    let summon_distance = timer_frac * (summon_distance.1 - summon_distance.0) + summon_distance.0;
    let summon_distance = summon_distance.round() as i32;

    // Only summons sprites if summon distance is greater than achieved radius
    for radius in achieved_radius..=summon_distance {
        // 1 added to make range correct, too lazy to add 1 to both variables above
        let radius = radius + 1;
        // Creates a spiral iterator for the newly achieved radius
        let spiral = Spiral2d::with_edge_radius(radius);
        for point in spiral {
            // If square is in the angle and is not sparse, generate sprite
            if data
                .ori
                .look_vec()
                .xy()
                .angle_between(point.as_())
                .to_degrees()
                <= (angle / 2.0)
                && !thread_rng().gen_bool(sparseness)
            {
                // The coordinates of where the sprite is created
                let sprite_pos = Vec3::new(
                    anchor_pos.x.floor() as i32 + point.x,
                    anchor_pos.y.floor() as i32 + point.y,
                    anchor_pos.z.floor() as i32,
                );

                // Check for collision in z up to 10 blocks up or down
                let (obstacle_z, obstacle_z_result) = data
                    .terrain
                    .ray(
                        sprite_pos.map(|x| x as f32 + 0.5) + Vec3::unit_z() * 10.0,
                        sprite_pos.map(|x| x as f32 + 0.5) - Vec3::unit_z() * 10.0,
                    )
                    .until(|b| {
                        // Until reaching a solid block that is not the created
                        // sprite
                        Block::is_solid(b) && b.get_sprite() != Some(sprite)
                    })
                    .cast();

                let z = match sprite {
                    // z height - 1 to delete sprite layer below caster
                    SpriteKind::Empty => sprite_pos.z + (10.5 - obstacle_z).ceil() as i32 - 1,
                    _ => {
                        sprite_pos.z
                            + if let (true, Ok(None)) = (stack_sprites, obstacle_z_result) {
                                0
                            } else {
                                (10.5 - obstacle_z).ceil() as i32
                            }
                    },
                };

                // Location sprite will be created
                let sprite_pos = Vec3::new(sprite_pos.x, sprite_pos.y, z);
                // Layers of sprites
                let layers = match sprite {
                    SpriteKind::SeaUrchin => 2,
                    _ => 1,
                };
                for i in 0..layers {
                    // Send server event to create sprite
                    output_events.emit_server(CreateSpriteEvent {
                        pos: Vec3::new(sprite_pos.x, sprite_pos.y, z + i),
                        sprite,
                        del_timeout,
                    });
                }
            }
        }
    }
    summon_distance
}
