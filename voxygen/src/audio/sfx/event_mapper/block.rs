/// EventMapper::Block watches the sound emitting blocks within
/// chunk range of the player and emits ambient sfx
use crate::{
    AudioFrontend,
    audio::sfx::{SFX_DIST_LIMIT_SQR, SfxEvent, SfxTriggerItem, SfxTriggers},
    scene::{Camera, Terrain, terrain::BlocksOfInterest},
};

use super::EventMapper;
use client::Client;
use common::{comp::Pos, spiral::Spiral2d, terrain::TerrainChunk, vol::RectRasterableVol};
use common_state::State;
use hashbrown::HashMap;
use rand::{Rng, prelude::*, seq::SliceRandom, thread_rng};
use rand_chacha::ChaCha8Rng;
use std::time::{Duration, Instant};
use vek::*;

#[derive(Clone, PartialEq)]
struct PreviousBlockState {
    event: SfxEvent,
    time: Instant,
}

impl Default for PreviousBlockState {
    fn default() -> Self {
        Self {
            event: SfxEvent::Idle,
            time: Instant::now()
                .checked_add(Duration::from_millis(thread_rng().gen_range(0..500)))
                .unwrap_or_else(Instant::now),
        }
    }
}

pub struct BlockEventMapper {
    history: HashMap<Vec3<i32>, PreviousBlockState>,
}

impl EventMapper for BlockEventMapper {
    fn maintain(
        &mut self,
        audio: &mut AudioFrontend,
        state: &State,
        player_entity: specs::Entity,
        camera: &Camera,
        triggers: &SfxTriggers,
        terrain: &Terrain<TerrainChunk>,
        client: &Client,
    ) {
        let mut rng = ChaCha8Rng::from_seed(thread_rng().gen());

        // Get the player position and chunk
        if let Some(player_pos) = state.read_component_copied::<Pos>(player_entity) {
            let player_chunk = player_pos.0.xy().map2(TerrainChunk::RECT_SIZE, |e, sz| {
                (e.floor() as i32).div_euclid(sz as i32)
            });

            // For determining if crickets should chirp
            let (terrain_alt, temp) = match client.current_chunk() {
                Some(chunk) => (chunk.meta().alt(), chunk.meta().temp()),
                None => (0.0, 0.0),
            };

            struct BlockSounds<'a> {
                // The function to select the blocks of interest that we should emit from
                blocks: fn(&'a BlocksOfInterest) -> &'a [Vec3<i32>],
                // The range, in chunks, that the particles should be generated in from the player
                range: usize,
                // The sound of the generated particle
                sfx: SfxEvent,
                // The volume of the sfx
                volume: f32,
                // Condition that must be true to play
                cond: fn(&State) -> bool,
            }

            let sounds: &[BlockSounds] = &[
                BlockSounds {
                    blocks: |boi| &boi.leaves,
                    range: 1,
                    sfx: SfxEvent::Birdcall,
                    volume: 1.5,
                    cond: |st| st.get_day_period().is_light(),
                },
                BlockSounds {
                    blocks: |boi| &boi.leaves,
                    range: 1,
                    sfx: SfxEvent::Owl,
                    volume: 1.5,
                    cond: |st| st.get_day_period().is_dark(),
                },
                BlockSounds {
                    blocks: |boi| &boi.slow_river,
                    range: 1,
                    sfx: SfxEvent::RunningWaterSlow,
                    volume: 1.0,
                    cond: |_| true,
                },
                BlockSounds {
                    blocks: |boi| &boi.fast_river,
                    range: 1,
                    sfx: SfxEvent::RunningWaterFast,
                    volume: 1.25,
                    cond: |_| true,
                },
                BlockSounds {
                    blocks: |boi| &boi.lavapool,
                    range: 1,
                    sfx: SfxEvent::Lavapool,
                    volume: 1.8,
                    cond: |_| true,
                },
                //BlockSounds {
                //    blocks: |boi| &boi.embers,
                //    range: 1,
                //    sfx: SfxEvent::Embers,
                //    volume: 0.15,
                //    //volume: 0.05,
                //    cond: |_| true,
                //    //cond: |st| st.get_day_period().is_dark(),
                //},
                BlockSounds {
                    blocks: |boi| &boi.frogs,
                    range: 1,
                    sfx: SfxEvent::Frog,
                    volume: 1.0,
                    cond: |st| st.get_day_period().is_dark(),
                },
                //BlockSounds {
                //    blocks: |boi| &boi.flowers,
                //    range: 4,
                //    sfx: SfxEvent::LevelUp,
                //    volume: 1.0,
                //    cond: |st| st.get_day_period().is_dark(),
                //},
                BlockSounds {
                    blocks: |boi| &boi.cricket1,
                    range: 1,
                    sfx: SfxEvent::Cricket1,
                    volume: 0.5,
                    cond: |st| st.get_day_period().is_dark(),
                },
                BlockSounds {
                    blocks: |boi| &boi.cricket2,
                    range: 1,
                    sfx: SfxEvent::Cricket2,
                    volume: 0.5,
                    cond: |st| st.get_day_period().is_dark(),
                },
                BlockSounds {
                    blocks: |boi| &boi.cricket3,
                    range: 1,
                    sfx: SfxEvent::Cricket3,
                    volume: 0.5,
                    cond: |st| st.get_day_period().is_dark(),
                },
                BlockSounds {
                    blocks: |boi| &boi.beehives,
                    range: 1,
                    sfx: SfxEvent::Bees,
                    volume: 0.5,
                    cond: |st| st.get_day_period().is_light(),
                },
            ];
            // Iterate through each kind of block of interest
            for sounds in sounds.iter() {
                // If the timing condition is false, continue
                // TODO Address bird hack properly. See TODO below
                if !(sounds.cond)(state)
                    || (!(sounds.sfx == SfxEvent::Lavapool) && player_pos.0.z < (terrain_alt - 30.0))
                    || (sounds.sfx == SfxEvent::Birdcall && (rng.gen_bool(0.9925) || client.weather_at_player().rain >= 0.07))
                    || (sounds.sfx == SfxEvent::Owl && (rng.gen_bool(0.997) || client.weather_at_player().rain >= 0.14))
                    || (sounds.sfx == SfxEvent::Frog && rng.gen_bool(0.95))
                    //Crickets will not chirp below 5 Celsius
                    || (sounds.sfx == SfxEvent::Cricket1 && ((temp < -0.33) || client.weather_at_player().rain >= 0.07))
                    || (sounds.sfx == SfxEvent::Cricket2 && ((temp < -0.33) || client.weather_at_player().rain >= 0.07))
                    || (sounds.sfx == SfxEvent::Cricket3 && ((temp < -0.33) || client.weather_at_player().rain >= 0.07))
                {
                    continue;
                }

                // For chunks surrounding the player position
                for offset in Spiral2d::new().take((sounds.range * 2 + 1).pow(2)) {
                    let chunk_pos = player_chunk + offset;

                    // Get all the blocks of interest in this chunk
                    terrain.get(chunk_pos).map(|chunk_data| {
                        // Get the positions of the blocks of type sounds
                        let blocks = (sounds.blocks)(&chunk_data.blocks_of_interest);

                        let absolute_pos: Vec3<i32> =
                            Vec3::from(chunk_pos * TerrainChunk::RECT_SIZE.map(|e| e as i32));

                        // Replace all RunningWater blocks with just one random one per tick
                        let blocks = if sounds.sfx == SfxEvent::RunningWaterSlow
                            || sounds.sfx == SfxEvent::RunningWaterFast
                            || sounds.sfx == SfxEvent::Lavapool
                        {
                            blocks
                                .choose(&mut rng)
                                .map(std::slice::from_ref)
                                .unwrap_or(&[])
                        } else {
                            blocks
                        };

                        // Iterate through each individual block
                        for block in blocks {
                            // TODO Address this hack properly, potentially by making a new
                            // block of interest type which picks fewer leaf blocks
                            // Hack to reduce the number of bird, frog, and water sounds
                            if ((sounds.sfx == SfxEvent::Birdcall || sounds.sfx == SfxEvent::Owl)
                                && rng.gen_bool(0.9995))
                                || (sounds.sfx == SfxEvent::Frog && rng.gen_bool(0.75))
                                || (sounds.sfx == SfxEvent::RunningWaterSlow && rng.gen_bool(0.5))
                                || (sounds.sfx == SfxEvent::Lavapool && rng.gen_bool(0.99))
                            {
                                continue;
                            }
                            let block_pos: Vec3<i32> = absolute_pos + block;
                            let internal_state = self.history.entry(block_pos).or_default();

                            let cam_pos = camera.get_pos_with_focus();

                            let block_pos = block_pos.map(|x| x as f32);

                            if Self::should_emit(
                                internal_state,
                                triggers.get_key_value(&sounds.sfx),
                                temp,
                            ) {
                                // If the camera is within SFX distance
                                if (block_pos.distance_squared(cam_pos)) < SFX_DIST_LIMIT_SQR {
                                    let sfx_trigger_item = triggers.get_key_value(&sounds.sfx);
                                    audio.emit_sfx(
                                        sfx_trigger_item,
                                        block_pos,
                                        Some(sounds.volume),
                                        player_pos.0,
                                    );
                                }
                                internal_state.time = Instant::now();
                                internal_state.event = sounds.sfx.clone();
                            }
                        }
                    });
                }
            }
        }
    }
}

impl BlockEventMapper {
    pub fn new() -> Self {
        Self {
            history: HashMap::new(),
        }
    }

    /// Ensures that:
    /// 1. An sfx.ron entry exists for an SFX event
    /// 2. The sfx has not been played since it's timeout threshold has elapsed,
    ///    which prevents firing every tick. Note that with so many blocks to
    ///    choose from and different blocks being selected each time, this is
    ///    not perfect, but does reduce the number of plays from blocks that
    ///    have already emitted sfx and are stored in the BlockEventMapper
    ///    history.
    fn should_emit(
        previous_state: &PreviousBlockState,
        sfx_trigger_item: Option<(&SfxEvent, &SfxTriggerItem)>,
        temp: f32,
    ) -> bool {
        let mut rng = ChaCha8Rng::from_seed(thread_rng().gen());

        if let Some((event, item)) = sfx_trigger_item {
            //The interval between cricket chirps calculated by converting chunk
            // temperature to centigrade (we should create a function for this) and applying
            // the "cricket formula" to it
            let cricket_interval = (25.0 / (3.0 * ((temp * 30.0) + 15.0))).max(0.5);
            if &previous_state.event == event {
                //In case certain sounds need modification to their threshold,
                //use match event
                match event {
                    SfxEvent::Cricket1 => {
                        previous_state.time.elapsed().as_secs_f32()
                            >= cricket_interval + rng.gen_range(-0.1..0.1)
                    },
                    SfxEvent::Cricket2 => {
                        //the length and manner of this sound is quite different
                        if cricket_interval < 0.75 {
                            previous_state.time.elapsed().as_secs_f32() >= 0.75
                        } else {
                            previous_state.time.elapsed().as_secs_f32()
                                >= cricket_interval + rng.gen_range(-0.1..0.1)
                        }
                    },
                    SfxEvent::Cricket3 => {
                        previous_state.time.elapsed().as_secs_f32()
                            >= cricket_interval + rng.gen_range(-0.1..0.1)
                    },
                    //Adds random factor to frogs (probably doesn't do anything most of the time)
                    SfxEvent::Frog => {
                        previous_state.time.elapsed().as_secs_f32() >= rng.gen_range(-2.0..2.0)
                    },
                    _ => previous_state.time.elapsed().as_secs_f32() >= item.threshold,
                }
            } else {
                true
            }
        } else {
            false
        }
    }
}
