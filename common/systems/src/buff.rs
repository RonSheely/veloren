use common::{
    Damage, DamageSource,
    combat::{self, DamageContributor},
    comp::{
        Alignment, Energy, Group, Health, HealthChange, Inventory, LightEmitter, Mass,
        ModifierKind, PhysicsState, Player, Pos, Stats,
        agent::{Sound, SoundKind},
        aura::{Auras, EnteredAuras},
        body::{Body, object},
        buff::{
            Buff, BuffCategory, BuffChange, BuffData, BuffEffect, BuffKey, BuffKind, BuffSource,
            Buffs, DestInfo,
        },
        fluid_dynamics::{Fluid, LiquidKind},
        item::MaterialStatManifest,
    },
    event::{
        BuffEvent, ChangeBodyEvent, ComboChangeEvent, CreateSpriteEvent, EmitExt,
        EnergyChangeEvent, HealthChangeEvent, RemoveLightEmitterEvent, SoundEvent,
    },
    event_emitters,
    outcome::Outcome,
    resources::{DeltaTime, Secs, Time},
    terrain::SpriteKind,
    uid::{IdMaps, Uid},
};
use common_base::prof_span;
use common_ecs::{Job, Origin, ParMode, Phase, System};
use rand::Rng;
use rayon::iter::ParallelIterator;
use specs::{
    Entities, Entity, LendJoin, ParJoin, Read, ReadExpect, ReadStorage, SystemData, WriteStorage,
    shred,
};
use vek::Vec3;

event_emitters! {
    struct Events[EventEmitters] {
        buff: BuffEvent,
        change_body: ChangeBodyEvent,
        remove_light: RemoveLightEmitterEvent,
        health_change: HealthChangeEvent,
        energy_change: EnergyChangeEvent,
        combo_change: ComboChangeEvent,
        sound: SoundEvent,
        create_sprite: CreateSpriteEvent,
        outcome: Outcome,
    }
}

#[derive(SystemData)]
pub struct ReadData<'a> {
    entities: Entities<'a>,
    dt: Read<'a, DeltaTime>,
    events: Events<'a>,
    inventories: ReadStorage<'a, Inventory>,
    healths: ReadStorage<'a, Health>,
    energies: ReadStorage<'a, Energy>,
    physics_states: ReadStorage<'a, PhysicsState>,
    groups: ReadStorage<'a, Group>,
    id_maps: Read<'a, IdMaps>,
    time: Read<'a, Time>,
    msm: ReadExpect<'a, MaterialStatManifest>,
    buffs: ReadStorage<'a, Buffs>,
    auras: ReadStorage<'a, Auras>,
    entered_auras: ReadStorage<'a, EnteredAuras>,
    positions: ReadStorage<'a, Pos>,
    bodies: ReadStorage<'a, Body>,
    light_emitters: ReadStorage<'a, LightEmitter>,
    alignments: ReadStorage<'a, Alignment>,
    players: ReadStorage<'a, Player>,
    masses: ReadStorage<'a, Mass>,
}

#[derive(Default)]
pub struct Sys;
impl<'a> System<'a> for Sys {
    type SystemData = (ReadData<'a>, WriteStorage<'a, Stats>);

    const NAME: &'static str = "buff";
    const ORIGIN: Origin = Origin::Common;
    const PHASE: Phase = Phase::Create;

    fn run(job: &mut Job<Self>, (read_data, mut stats): Self::SystemData) {
        let mut emitters = read_data.events.get_emitters();
        let dt = read_data.dt.0;
        // Set to false to avoid spamming server
        stats.set_event_emission(false);

        // Put out underwater campfires. Logically belongs here since this system also
        // removes burning, but campfires don't have healths/stats/energies/buffs, so
        // this needs a separate loop.
        job.cpu_stats.measure(ParMode::Rayon);
        let to_put_out_campfires = (
            &read_data.entities,
            &read_data.bodies,
            &read_data.physics_states,
            &read_data.light_emitters, //to improve iteration speed
        )
            .par_join()
            .map_init(
                || {
                    prof_span!(guard, "buff campfire deactivate");
                    guard
                },
                |_guard, (entity, body, physics_state, _)| {
                    if matches!(*body, Body::Object(object::Body::CampfireLit))
                        && matches!(
                            physics_state.in_fluid,
                            Some(Fluid::Liquid {
                                kind: LiquidKind::Water,
                                ..
                            })
                        )
                    {
                        Some(entity)
                    } else {
                        None
                    }
                },
            )
            .fold(Vec::new, |mut to_put_out_campfires, put_out_campfire| {
                put_out_campfire.map(|put| to_put_out_campfires.push(put));
                to_put_out_campfires
            })
            .reduce(
                Vec::new,
                |mut to_put_out_campfires_a, mut to_put_out_campfires_b| {
                    to_put_out_campfires_a.append(&mut to_put_out_campfires_b);
                    to_put_out_campfires_a
                },
            );
        job.cpu_stats.measure(ParMode::Single);
        {
            prof_span!(_guard, "write deferred campfire deletion");
            // Assume that to_put_out_campfires is near to zero always, so this access isn't
            // slower than parallel checking above
            for e in to_put_out_campfires {
                {
                    emitters.emit(ChangeBodyEvent {
                        entity: e,
                        new_body: Body::Object(object::Body::Campfire),
                        permanent_change: None,
                    });
                    emitters.emit(RemoveLightEmitterEvent { entity: e });
                }
            }
        }

        let mut rng = rand::thread_rng();
        let buff_join = (
            &read_data.entities,
            &read_data.buffs,
            &mut stats,
            &read_data.bodies,
            &read_data.healths,
            &read_data.energies,
            read_data.physics_states.maybe(),
            read_data.masses.maybe(),
        )
            .lend_join();
        buff_join.for_each(|comps| {
            let (entity, buff_comp, mut stat, body, health, energy, physics_state, mass) = comps;
            let dest_info = DestInfo {
                stats: Some(&stat),
                mass,
            };
            // Apply buffs to entity based off of their current physics_state
            if let Some(physics_state) = physics_state {
                // Set nearby entities on fire if burning
                if let Some((_, burning)) = buff_comp.iter_kind(BuffKind::Burning).next() {
                    for t_entity in physics_state.touch_entities.keys().filter_map(|te_uid| {
                        read_data.id_maps.uid_entity(*te_uid).filter(|te| {
                            combat::permit_pvp(
                                &read_data.alignments,
                                &read_data.players,
                                &read_data.entered_auras,
                                &read_data.id_maps,
                                Some(entity),
                                *te,
                            )
                        })
                    }) {
                        let duration = burning.data.duration.map(|d| d * 0.9);
                        if duration.is_none_or(|d| d.0 >= 1.0)
                            && rng
                                .gen_bool((dt * burning.data.strength / 5.0).clamp(0.0, 1.0).into())
                        {
                            // NOTE: setting source as the burned character is
                            // problematic for whole array of reasons.
                            // 1) It would show non-sensical death message.
                            // 2) It makes NPCs hate the victim of fire, which is rather annoying.
                            // 3) It makes NPCs hate other NPCs and attempting to attack them, even
                            //    when they are in the same group.
                            //
                            // We could reference original source, but it might
                            // have some cheesing & griefing potential.
                            //
                            // Yes, with this implementation you could put
                            // yourself on fire, and then harras NPCs but good
                            // luck with that.
                            emitters.emit(BuffEvent {
                                entity: t_entity,
                                buff_change: BuffChange::Add(Buff::new(
                                    BuffKind::Burning,
                                    BuffData::new(burning.data.strength, duration),
                                    vec![BuffCategory::Natural],
                                    BuffSource::World,
                                    *read_data.time,
                                    DestInfo {
                                        // Can't mutably access stats, and for burning debuff stats
                                        // has no effect (for now)
                                        stats: None,
                                        mass: read_data.masses.get(t_entity),
                                    },
                                    mass,
                                )),
                            });
                        }
                    }
                }
                if matches!(
                    physics_state.on_ground.and_then(|b| b.get_sprite()),
                    Some(SpriteKind::EnsnaringVines)
                ) {
                    // If on ensnaring vines, apply partial ensnared debuff
                    emitters.emit(BuffEvent {
                        entity,
                        buff_change: BuffChange::Add(Buff::new(
                            BuffKind::Ensnared,
                            BuffData::new(0.5, Some(Secs(0.1))),
                            Vec::new(),
                            BuffSource::World,
                            *read_data.time,
                            dest_info,
                            None,
                        )),
                    });
                }
                if matches!(
                    physics_state.on_ground.and_then(|b| b.get_sprite()),
                    Some(SpriteKind::EnsnaringWeb)
                ) {
                    // If on ensnaring web, apply ensnared debuff
                    emitters.emit(BuffEvent {
                        entity,
                        buff_change: BuffChange::Add(Buff::new(
                            BuffKind::Ensnared,
                            BuffData::new(1.0, Some(Secs(1.0))),
                            Vec::new(),
                            BuffSource::World,
                            *read_data.time,
                            dest_info,
                            None,
                        )),
                    });
                }
                if matches!(
                    physics_state.on_ground.and_then(|b| b.get_sprite()),
                    Some(SpriteKind::SeaUrchin)
                ) {
                    // If touching Sea Urchin apply Bleeding buff
                    emitters.emit(BuffEvent {
                        entity,
                        buff_change: BuffChange::Add(Buff::new(
                            BuffKind::Bleeding,
                            BuffData::new(1.0, Some(Secs(6.0))),
                            Vec::new(),
                            BuffSource::World,
                            *read_data.time,
                            dest_info,
                            None,
                        )),
                    });
                }
                if matches!(
                    physics_state.on_ground.and_then(|b| b.get_sprite()),
                    Some(SpriteKind::HaniwaTrap)
                ) && !body.immune_to(BuffKind::Bleeding)
                {
                    // TODO: Determine a better place to emit sprite change events
                    if let Some(pos) = read_data.positions.get(entity) {
                        // If touching Trap - change sprite and apply Bleeding buff
                        emitters.emit(CreateSpriteEvent {
                            pos: Vec3::new(pos.0.x as i32, pos.0.y as i32, pos.0.z as i32 - 1),
                            sprite: SpriteKind::HaniwaTrapTriggered,
                            del_timeout: Some((4.0, 1.0)),
                        });
                        emitters.emit(SoundEvent {
                            sound: Sound::new(SoundKind::Trap, pos.0, 12.0, read_data.time.0),
                        });
                        emitters.emit(Outcome::Slash { pos: pos.0 });

                        emitters.emit(BuffEvent {
                            entity,
                            buff_change: BuffChange::Add(Buff::new(
                                BuffKind::Bleeding,
                                BuffData::new(5.0, Some(Secs(3.0))),
                                Vec::new(),
                                BuffSource::World,
                                *read_data.time,
                                dest_info,
                                None,
                            )),
                        });
                    }
                }
                if matches!(
                    physics_state.on_ground.and_then(|b| b.get_sprite()),
                    Some(SpriteKind::IronSpike | SpriteKind::HaniwaTrapTriggered)
                ) {
                    // If touching Iron Spike apply Bleeding buff
                    emitters.emit(BuffEvent {
                        entity,
                        buff_change: BuffChange::Add(Buff::new(
                            BuffKind::Bleeding,
                            BuffData::new(1.0, Some(Secs(4.0))),
                            Vec::new(),
                            BuffSource::World,
                            *read_data.time,
                            dest_info,
                            None,
                        )),
                    });
                }
                if matches!(
                    physics_state.on_ground.and_then(|b| b.get_sprite()),
                    Some(SpriteKind::HotSurface)
                ) {
                    // If touching a hot surface apply Burning buff
                    emitters.emit(BuffEvent {
                        entity,
                        buff_change: BuffChange::Add(Buff::new(
                            BuffKind::Burning,
                            BuffData::new(10.0, None),
                            Vec::new(),
                            BuffSource::World,
                            *read_data.time,
                            dest_info,
                            None,
                        )),
                    });
                }
                if matches!(
                    physics_state.on_ground.and_then(|b| b.get_sprite()),
                    Some(SpriteKind::IceSpike)
                ) {
                    // When standing on IceSpike, apply bleeding
                    emitters.emit(BuffEvent {
                        entity,
                        buff_change: BuffChange::Add(Buff::new(
                            BuffKind::Bleeding,
                            BuffData::new(15.0, Some(Secs(0.1))),
                            Vec::new(),
                            BuffSource::World,
                            *read_data.time,
                            dest_info,
                            None,
                        )),
                    });
                    // When standing on IceSpike also apply Frozen
                    emitters.emit(BuffEvent {
                        entity,
                        buff_change: BuffChange::Add(Buff::new(
                            BuffKind::Frozen,
                            BuffData::new(0.2, Some(Secs(3.0))),
                            Vec::new(),
                            BuffSource::World,
                            *read_data.time,
                            dest_info,
                            None,
                        )),
                    });
                }
                if matches!(
                    physics_state.on_ground.and_then(|b| b.get_sprite()),
                    Some(SpriteKind::FireBlock)
                ) {
                    // If on FireBlock vines, apply burning buff
                    emitters.emit(BuffEvent {
                        entity,
                        buff_change: BuffChange::Add(Buff::new(
                            BuffKind::Burning,
                            BuffData::new(20.0, None),
                            Vec::new(),
                            BuffSource::World,
                            *read_data.time,
                            dest_info,
                            None,
                        )),
                    });
                }
                if matches!(
                    physics_state.in_fluid,
                    Some(Fluid::Liquid {
                        kind: LiquidKind::Lava,
                        ..
                    })
                ) && !body.negates_buff(BuffKind::Burning)
                {
                    // If in lava fluid, apply burning debuff
                    emitters.emit(BuffEvent {
                        entity,
                        buff_change: BuffChange::Add(Buff::new(
                            BuffKind::Burning,
                            BuffData::new(20.0, None),
                            vec![BuffCategory::Natural],
                            BuffSource::World,
                            *read_data.time,
                            dest_info,
                            None,
                        )),
                    });
                } else if matches!(
                    physics_state.in_fluid,
                    Some(Fluid::Liquid {
                        kind: LiquidKind::Water,
                        ..
                    })
                ) && buff_comp.kinds[BuffKind::Burning].is_some()
                {
                    // If in water fluid and currently burning, remove burning debuffs
                    emitters.emit(BuffEvent {
                        entity,
                        buff_change: BuffChange::RemoveByKind(BuffKind::Burning),
                    });
                }
            }

            let mut expired_buffs = Vec::<BuffKey>::new();

            // Replace buffs from an active aura with a normal buff when out of range of the
            // aura or link no longer active
            for (buff_key, buff) in &buff_comp.buffs {
                let keep = buff.cat_ids.iter().all(|cat| match cat {
                    BuffCategory::FromActiveAura(source, key) => {
                        let Some(source_entity) = read_data.id_maps.uid_entity(*source) else {
                            return false;
                        };

                        let Some(aura) = read_data
                            .auras
                            .get(source_entity)
                            .and_then(|aura| aura.auras.get(*key))
                        else {
                            return false;
                        };

                        let (Some(pos), Some(aura_pos)) = (
                            read_data.positions.get(entity),
                            read_data.positions.get(source_entity),
                        ) else {
                            return false;
                        };

                        pos.0.distance_squared(aura_pos.0) <= aura.radius.powi(2)
                    },
                    BuffCategory::FromLink(l) => l.exists(),
                    _ => true,
                });

                if !keep {
                    expired_buffs.push(buff_key);
                    emitters.emit(BuffEvent {
                        entity,
                        buff_change: BuffChange::Add(Buff::new(
                            buff.kind,
                            buff.data,
                            buff.cat_ids
                                .iter()
                                .filter(|cat_id| {
                                    !matches!(
                                        cat_id,
                                        BuffCategory::FromActiveAura(..)
                                            | BuffCategory::FromLink(..)
                                    )
                                })
                                .cloned()
                                .collect::<Vec<_>>(),
                            buff.source,
                            *read_data.time,
                            dest_info,
                            None,
                        )),
                    });
                }
            }

            buff_comp.buffs.iter().for_each(|(buff_key, buff)| {
                if buff.end_time.is_some_and(|end| end.0 < read_data.time.0) {
                    expired_buffs.push(buff_key)
                }
            });

            let infinite_damage_reduction = (Damage::compute_damage_reduction(
                None,
                read_data.inventories.get(entity),
                Some(&stat),
                &read_data.msm,
            ) - 1.0)
                .abs()
                < f32::EPSILON;
            if infinite_damage_reduction {
                for (key, buff) in buff_comp.buffs.iter() {
                    if !buff.kind.is_buff() {
                        expired_buffs.push(key);
                    }
                }
            }

            // Call to reset stats to base values
            stat.reset_temp_modifiers();

            let mut body_override = None;

            // Iterator over the lists of buffs by kind
            let mut buff_kinds = buff_comp
                .kinds
                .iter()
                .filter_map(|(kind, keys)| keys.as_ref().map(|keys| (kind, keys.clone())))
                .collect::<Vec<(BuffKind, (Vec<BuffKey>, Time))>>();
            buff_kinds.sort_by_key(|(kind, _)| !kind.affects_subsequent_buffs());
            for (buff_kind, (buff_keys, kind_start_time)) in buff_kinds.into_iter() {
                let mut active_buff_keys = Vec::new();
                if infinite_damage_reduction && !buff_kind.is_buff() {
                    continue;
                }

                if buff_kind.stacks() {
                    // Process all the buffs of this kind
                    active_buff_keys = buff_keys;
                } else {
                    // Only process the strongest of this buff kind
                    active_buff_keys.push(buff_keys[0]);
                }
                for buff_key in active_buff_keys.into_iter() {
                    if let Some(buff) = buff_comp.buffs.get(buff_key) {
                        // Skip the effect of buffs whose start delay hasn't expired.
                        if buff.start_time.0 > read_data.time.0 {
                            continue;
                        }
                        // Get buff owner?
                        let buff_owner = if let BuffSource::Character { by: owner } = buff.source {
                            Some(owner)
                        } else {
                            None
                        };

                        // Now, execute the buff, based on it's delta
                        for effect in &buff.effects {
                            execute_effect(
                                effect,
                                buff.kind,
                                buff.start_time,
                                kind_start_time,
                                &read_data,
                                &mut stat,
                                body,
                                &mut body_override,
                                health,
                                energy,
                                entity,
                                buff_owner,
                                &mut emitters,
                                dt,
                                *read_data.time,
                                expired_buffs.contains(&buff_key),
                                buff_comp,
                            );
                        }
                    }
                }
            }

            // Update body if needed.
            let new_body = body_override.unwrap_or(stat.original_body);
            if new_body != *body {
                emitters.emit(ChangeBodyEvent {
                    entity,
                    new_body,
                    permanent_change: None,
                });
            }

            // Remove buffs that expire
            if !expired_buffs.is_empty() {
                emitters.emit(BuffEvent {
                    entity,
                    buff_change: BuffChange::RemoveByKey(expired_buffs),
                });
            }

            // Remove buffs that don't persist on death
            if health.is_dead {
                emitters.emit(BuffEvent {
                    entity,
                    buff_change: BuffChange::RemoveByCategory {
                        all_required: vec![],
                        any_required: vec![],
                        none_required: vec![BuffCategory::PersistOnDeath],
                    },
                });
            }
        });
        // Turned back to true
        stats.set_event_emission(true);
    }
}

// TODO: Globally disable this clippy lint
#[expect(clippy::too_many_arguments)]
fn execute_effect(
    effect: &BuffEffect,
    buff_kind: BuffKind,
    buff_start_time: Time,
    buff_kind_start_time: Time,
    read_data: &ReadData,
    stat: &mut Stats,
    current_body: &Body,
    body_override: &mut Option<Body>,
    health: &Health,
    energy: &Energy,
    entity: Entity,
    buff_owner: Option<Uid>,
    server_emitter: &mut (
             impl EmitExt<HealthChangeEvent>
             + EmitExt<EnergyChangeEvent>
             + EmitExt<ComboChangeEvent>
             + EmitExt<BuffEvent>
         ),
    dt: f32,
    time: Time,
    buff_will_expire: bool,
    buffs_comp: &Buffs,
) {
    let num_ticks = |tick_dur: Secs| {
        let time_passed = time.0 - buff_start_time.0;
        let dt = dt as f64;
        // Number of ticks has 3 parts
        //
        // First part checks if delta time was larger than the tick duration, if it was
        // determines number of ticks in that time
        //
        // Second part checks if delta time has just passed the threshold for a tick
        // ending/starting (and accounts for if that delta time was longer than the tick
        // duration)
        // 0.000001 is to account for floating imprecision so this is not applied on the
        // first tick
        //
        // Third part returns the fraction of the current time passed since the last
        // time a tick duration would have happened, this is ignored (by flooring) when
        // the buff is not ending, but is used if the buff is ending this tick
        let curr_tick = (time_passed / tick_dur.0).floor();
        let prev_tick = ((time_passed - dt).max(0.0) / tick_dur.0).floor();
        let whole_ticks = curr_tick - prev_tick;

        if buff_will_expire {
            // If the buff is ending, include the fraction of progress towards the next
            // tick.
            let fractional_tick = (time_passed % tick_dur.0) / tick_dur.0;
            Some((whole_ticks + fractional_tick) as f32)
        } else if whole_ticks >= 1.0 {
            Some(whole_ticks as f32)
        } else {
            None
        }
    };
    match effect {
        BuffEffect::HealthChangeOverTime {
            rate,
            kind,
            instance,
            tick_dur,
        } => {
            if let Some(num_ticks) = num_ticks(*tick_dur) {
                let amount = *rate * num_ticks * tick_dur.0 as f32;

                let (cause, by) = if amount != 0.0 {
                    (Some(DamageSource::Buff(buff_kind)), buff_owner)
                } else {
                    (None, None)
                };
                let amount = match *kind {
                    ModifierKind::Additive => amount,
                    ModifierKind::Multiplicative => health.maximum() * amount,
                };
                let damage_contributor = by.and_then(|uid| {
                    read_data.id_maps.uid_entity(uid).map(|entity| {
                        DamageContributor::new(uid, read_data.groups.get(entity).cloned())
                    })
                });
                server_emitter.emit(HealthChangeEvent {
                    entity,
                    change: HealthChange {
                        amount,
                        by: damage_contributor,
                        cause,
                        time: *read_data.time,
                        precise: false,
                        instance: *instance,
                    },
                });
            };
        },
        BuffEffect::EnergyChangeOverTime {
            rate,
            kind,
            tick_dur,
            reset_rate_on_tick,
        } => {
            if let Some(num_ticks) = num_ticks(*tick_dur) {
                let amount = *rate * num_ticks * tick_dur.0 as f32;

                let amount = match *kind {
                    ModifierKind::Additive => amount,
                    ModifierKind::Multiplicative => energy.maximum() * amount,
                };
                server_emitter.emit(EnergyChangeEvent {
                    entity,
                    change: amount,
                    reset_rate: *reset_rate_on_tick,
                });
            };
        },
        BuffEffect::ComboChangeOverTime { rate, tick_dur } => {
            if let Some(num_ticks) = num_ticks(*tick_dur) {
                let amount = (*rate * num_ticks * tick_dur.0 as f32) as i32;

                server_emitter.emit(ComboChangeEvent {
                    entity,
                    change: amount,
                });
            };
        },
        BuffEffect::MaxHealthModifier { value, kind } => match kind {
            ModifierKind::Additive => {
                stat.max_health_modifiers.add_mod += *value;
            },
            ModifierKind::Multiplicative => {
                stat.max_health_modifiers.mult_mod *= *value;
            },
        },
        BuffEffect::MaxEnergyModifier { value, kind } => match kind {
            ModifierKind::Additive => {
                stat.max_energy_modifiers.add_mod += *value;
            },
            ModifierKind::Multiplicative => {
                stat.max_energy_modifiers.mult_mod *= *value;
            },
        },
        BuffEffect::DamageReduction(dr) => {
            if *dr > 0.0 {
                stat.damage_reduction.pos_mod = stat.damage_reduction.pos_mod.max(*dr);
            } else {
                stat.damage_reduction.neg_mod += dr;
            }
        },
        BuffEffect::MaxHealthChangeOverTime {
            rate,
            kind,
            target_fraction,
        } => {
            let potential_amount = (time.0 - buff_kind_start_time.0) as f32 * rate;

            // Percentage change that should be applied to max_health
            let potential_fraction = 1.0
                + match kind {
                    ModifierKind::Additive => {
                        // `rate * dt` is amount of health, dividing by base max
                        // creates fraction
                        potential_amount / health.base_max()
                    },
                    ModifierKind::Multiplicative => {
                        // `rate * dt` is the fraction
                        potential_amount
                    },
                };

            // Potential progress towards target fraction, if
            // target_fraction ~ 1.0 then set progress to 1.0 to avoid
            // divide by zero
            let progress = if (1.0 - *target_fraction).abs() > f32::EPSILON {
                (1.0 - potential_fraction) / (1.0 - *target_fraction)
            } else {
                1.0
            };

            // Change achieved_fraction depending on what other buffs have
            // occurred
            let achieved_fraction = if progress > 1.0 {
                // If potential fraction already beyond target fraction,
                // simply multiply max_health_modifier by the target
                // fraction, and set achieved fraction to target_fraction
                *target_fraction
            } else {
                // Else have not achieved target yet, use potential_fraction
                potential_fraction
            };

            // Apply achieved_fraction to max_health_modifier
            stat.max_health_modifiers.mult_mod *= achieved_fraction;
        },
        BuffEffect::MovementSpeed(speed) => {
            stat.move_speed_modifier *= *speed;
        },
        BuffEffect::AttackSpeed(speed) => {
            stat.attack_speed_modifier *= *speed;
        },
        BuffEffect::RecoverySpeed(speed) => {
            stat.recovery_speed_modifier *= *speed;
        },
        BuffEffect::GroundFriction(gf) => {
            stat.friction_modifier *= *gf;
        },
        BuffEffect::PoiseReduction(pr) => {
            if *pr > 0.0 {
                stat.poise_reduction.pos_mod = stat.poise_reduction.pos_mod.max(*pr);
            } else {
                stat.poise_reduction.neg_mod += pr;
            }
        },
        BuffEffect::PoiseDamageFromLostHealth(strength) => {
            stat.poise_damage_modifier *= 1.0 + (1.0 - health.fraction()) * *strength;
        },
        BuffEffect::AttackDamage(dam) => {
            stat.attack_damage_modifier *= *dam;
        },
        BuffEffect::PrecisionOverride(val) => {
            // Use lower of precision multiplier overrides
            stat.precision_multiplier_override = stat
                .precision_multiplier_override
                .map(|mult| mult.min(*val))
                .or(Some(*val));
        },
        BuffEffect::PrecisionVulnerabilityOverride(val) => {
            // Use higher of precision multiplier overrides
            stat.precision_vulnerability_multiplier_override = stat
                .precision_vulnerability_multiplier_override
                .map(|mult| mult.max(*val))
                .or(Some(*val));
        },
        BuffEffect::BodyChange(b) => {
            // For when an entity is under the effects of multiple de/buffs that change the
            // body, to avoid flickering between many bodies only change the body if the
            // override body is not equal to the current body. (If the buff that caused the
            // current body is still active, body override will eventually pick up on it,
            // otherwise this will end up with a new body, though random depending on
            // iteration order)
            if Some(current_body) != body_override.as_ref() {
                *body_override = Some(*b)
            }
        },
        BuffEffect::BuffImmunity(buff_kind) => {
            if buffs_comp.contains(*buff_kind) {
                server_emitter.emit(BuffEvent {
                    entity,
                    buff_change: BuffChange::RemoveByKind(*buff_kind),
                });
            }
        },
        BuffEffect::SwimSpeed(speed) => {
            stat.swim_speed_modifier *= speed;
        },
        BuffEffect::AttackEffect(effect) => stat.effects_on_attack.push(effect.clone()),
        BuffEffect::AttackPoise(p) => {
            stat.poise_damage_modifier *= p;
        },
        BuffEffect::MitigationsPenetration(mp) => {
            stat.mitigations_penetration =
                1.0 - ((1.0 - stat.mitigations_penetration) * (1.0 - *mp));
        },
        BuffEffect::EnergyReward(er) => {
            stat.energy_reward_modifier *= er;
        },
        BuffEffect::DamagedEffect(effect) => stat.effects_on_damaged.push(effect.clone()),
        BuffEffect::DeathEffect(effect) => stat.effects_on_death.push(effect.clone()),
        BuffEffect::DisableAuxiliaryAbilities => stat.disable_auxiliary_abilities = true,
        BuffEffect::CrowdControlResistance(ccr) => {
            stat.crowd_control_resistance += ccr;
        },
        BuffEffect::ItemEffectReduction(ier) => {
            stat.item_effect_reduction *= 1.0 - ier;
        },
    };
}
