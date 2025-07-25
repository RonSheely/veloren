//! This rule is by far the most significant rule in rtsim to date and governs
//! the behaviour of rtsim NPCs. It uses a novel combinator-based API to express
//! long-running NPC actions in a manner that's halfway between [async/coroutine programming](https://en.wikipedia.org/wiki/Coroutine) and traditional
//! [AI decision trees](https://en.wikipedia.org/wiki/Decision_tree).
//!
//! It may feel unintuitive when you first work with it, but trust us:
//! expressing your AI behaviour in this way brings radical advantages and will
//! simplify your code and make debugging exponentially easier.
//!
//! The fundamental abstraction is that of [`Action`]s. [`Action`]s, somewhat
//! like [`core::future::Future`], represent a long-running behaviour performed
//! by an NPC. See [`Action`] for a deeper explanation of actions and the
//! methods that can be used to combine them together.
//!
//! NPC actions act upon the NPC's [`crate::data::npc::Controller`]. This type
//! represent the immediate behavioural intentions of the NPC during simulation,
//! such as by specifying a location to walk to, an action to perform, speech to
//! say, or some persistent state to change (like the NPC's home site).
//!
//! After brain simulation has occurred, the resulting controller state is
//! passed to either rtsim's internal NPC simulation rule
//! ([`crate::rule::simulate_npcs`]) or, if the chunk the NPC is loaded, are
//! passed to the Veloren server's agent system which attempts to act in
//! accordance with it.

mod airship_ai;
pub mod dialogue;
pub mod movement;
pub mod util;

use std::{collections::VecDeque, hash::BuildHasherDefault, sync::Arc};

use crate::{
    RtState, Rule, RuleError,
    ai::{
        Action, NpcCtx, State, casual, choose, finish, important, just, now,
        predicate::{Chance, EveryRange, Predicate, every_range, timeout},
        seq, until,
    },
    data::{
        ReportKind, Sentiment, Sites,
        npc::{Brain, DialogueSession, PathData, SimulationMode},
    },
    event::OnTick,
};
use common::{
    assets::AssetExt,
    astar::{Astar, PathResult},
    comp::{
        self, Content, bird_large,
        compass::{Direction, Distance},
        item::ItemDef,
    },
    path::Path,
    rtsim::{
        Actor, ChunkResource, DialogueKind, NpcInput, PersonalityTrait, Profession, Response, Role,
        SiteId,
    },
    spiral::Spiral2d,
    store::Id,
    terrain::{CoordinateConversions, TerrainChunkSize, sprite},
    time::DayPeriod,
    util::Dir,
};
use core::ops::ControlFlow;
use fxhash::FxHasher64;
use itertools::{Either, Itertools};
use rand::prelude::*;
use rand_chacha::ChaChaRng;
use rayon::iter::{IntoParallelRefMutIterator, ParallelIterator};
use vek::*;
use world::{
    IndexRef, World,
    civ::{self, Track},
    site::{
        self, PlotKind, Site as WorldSite, SiteKind, TileKind,
        plot::{PlotKindMeta, tavern},
    },
    util::NEIGHBORS,
};

use self::{
    movement::{
        follow_actor, goto, goto_2d, goto_2d_flying, goto_actor, travel_to_point, travel_to_site,
    },
    util::do_dialogue,
};

/// How many ticks should pass between running NPC AI.
/// Note that this only applies to simulated NPCs: loaded NPCs have their AI
/// code run every tick. This means that AI code should be broadly
/// DT-independent.
const SIMULATED_TICK_SKIP: u64 = 10;

pub struct NpcAi;

#[derive(Clone)]
struct DefaultState {
    socialize_timer: EveryRange,
    move_home_timer: Chance<EveryRange>,
}

impl Rule for NpcAi {
    fn start(rtstate: &mut RtState) -> Result<Self, RuleError> {
        // Keep track of the last `SIMULATED_TICK_SKIP` ticks, to know the deltatime
        // since the last tick we ran the npc.
        let mut last_ticks: VecDeque<_> = [1.0 / 30.0; SIMULATED_TICK_SKIP as usize]
            .into_iter()
            .collect();

        rtstate.bind::<Self, OnTick>(move |ctx| {
            last_ticks.push_front(ctx.event.dt);
            if last_ticks.len() >= SIMULATED_TICK_SKIP as usize {
                last_ticks.pop_back();
            }
            // Temporarily take the brains of NPCs out of their heads to appease the borrow
            // checker
            let mut npc_data = {
                let mut data = ctx.state.data_mut();
                data.npcs
                    .iter_mut()
                    // Don't run AI for dead NPCs
                    .filter(|(_, npc)| !npc.is_dead() && !matches!(npc.role, Role::Vehicle))
                    // Don't run AI for simulated NPCs every tick
                    .filter(|(_, npc)| matches!(npc.mode, SimulationMode::Loaded) || (npc.seed as u64 + ctx.event.tick) % SIMULATED_TICK_SKIP == 0)
                    .map(|(npc_id, npc)| {
                        let controller = std::mem::take(&mut npc.controller);
                        let inbox = std::mem::take(&mut npc.inbox);
                        let sentiments = std::mem::take(&mut npc.sentiments);
                        let known_reports = std::mem::take(&mut npc.known_reports);
                        let brain = npc.brain.take().unwrap_or_else(|| Brain {
                            action: Box::new(think().repeat().with_state(DefaultState {
                                socialize_timer: every_range(15.0..30.0),
                                move_home_timer: every_range(400.0..2000.0).chance(0.5),
                            })),
                        });
                        let npc_dialogue = std::mem::take(&mut npc.npc_dialogue);
                        (npc_id, controller, inbox, sentiments, known_reports, brain, npc_dialogue)
                    })
                    .collect::<Vec<_>>()
            };

            // The sum of the last `SIMULATED_TICK_SKIP` tick deltatimes is the deltatime since
            // simulated npcs ran this tick had their ai ran.
            let simulated_dt = last_ticks.iter().sum::<f32>();

            // Do a little thinking
            {
                let data = &*ctx.state.data();

                npc_data
                    .par_iter_mut()
                    .for_each(|(npc_id, controller, inbox, sentiments, known_reports, brain, npc_dialogue)| {
                        let npc = &data.npcs[*npc_id];

                        controller.reset();

                        brain.action.tick(&mut NpcCtx {
                            state: ctx.state,
                            world: ctx.world,
                            index: ctx.index,
                            time_of_day: ctx.event.time_of_day,
                            time: ctx.event.time,
                            npc,
                            npc_id: *npc_id,
                            controller,
                            npc_dialogue,
                            inbox,
                            known_reports,
                            sentiments,
                            dt: if matches!(npc.mode, SimulationMode::Loaded) {
                                ctx.event.dt
                            } else {
                                simulated_dt
                            },
                            rng: ChaChaRng::from_seed(thread_rng().gen::<[u8; 32]>()),
                            system_data: &*ctx.system_data,
                        }, &mut ());

                        // If an input wasn't processed by the brain, we no longer have a use for it
                        inbox.clear();
                    });
            }

            // Reinsert NPC brains
            let mut data = ctx.state.data_mut();
            let mut to_update = Vec::with_capacity(npc_data.len());
            for (npc_id, controller, inbox, sentiments, known_reports, brain, npc_dialogue) in npc_data {
                to_update.push(npc_id);
                data.npcs[npc_id].controller = controller;
                data.npcs[npc_id].brain = Some(brain);
                data.npcs[npc_id].inbox = inbox;
                data.npcs[npc_id].sentiments = sentiments;
                data.npcs[npc_id].known_reports = known_reports;
                data.npcs[npc_id].npc_dialogue = npc_dialogue;
            }

            for npc_id in to_update {
                let v = std::mem::take(&mut data.npcs[npc_id].controller.npc_actions);
                for (target, action) in v {
                    if let Some(npc) = data.npcs.get_mut(target) {
                        npc.npc_dialogue.push_back((npc_id, action));
                    }
                }
            }
        });

        Ok(Self)
    }
}

fn idle<S: State>() -> impl Action<S> + Clone {
    just(|ctx, _| ctx.controller.do_idle()).debug(|| "idle")
}

fn talk_to<S: State>(tgt: Actor) -> impl Action<S> {
    now(move |ctx, _| {
        if ctx.sentiments.toward(tgt).is(Sentiment::ENEMY) {
            just(move |ctx, _| {
                ctx.controller
                    .say(tgt, Content::localized("npc-speech-reject_rival"))
            })
            .boxed()
        } else if matches!(tgt, Actor::Character(_)) {
            do_dialogue(tgt, move |session| dialogue::general(tgt, session)).boxed()
        } else {
            smalltalk_to(tgt).boxed()
        }
    })
}

fn tell_site_content(ctx: &NpcCtx, site: SiteId) -> Option<Content> {
    if let Some(world_site) = ctx.state.data().sites.get(site)
        && let Some(site_name) = util::site_name(ctx, site)
    {
        Some(Content::localized_with_args("npc-speech-tell_site", [
            ("site", Content::Plain(site_name)),
            (
                "dir",
                Direction::from_dir(world_site.wpos.as_() - ctx.npc.wpos.xy()).localize_npc(),
            ),
            (
                "dist",
                Distance::from_length(world_site.wpos.as_().distance(ctx.npc.wpos.xy()) as i32)
                    .localize_npc(),
            ),
        ]))
    } else {
        None
    }
}

fn smalltalk_to<S: State>(tgt: Actor) -> impl Action<S> {
    now(move |ctx, _| {
        if matches!(tgt, Actor::Npc(_)) && ctx.rng.gen_bool(0.2) {
            // Cut off the conversation sometimes to avoid infinite conversations (but only
            // if the target is an NPC!) TODO: Don't special case this, have
            // some sort of 'bored of conversation' system
            idle().boxed()
        } else {
            // Mention nearby sites
            let comment = if ctx.rng.gen_bool(0.3)
                && let Some(current_site) = ctx.npc.current_site
                && let Some(current_site) = ctx.state.data().sites.get(current_site)
                && let Some(mention_site) = current_site.nearby_sites_by_size.choose(&mut ctx.rng)
                && let Some(content) = tell_site_content(ctx, *mention_site)
            {
                content
            // Mention current site
            } else if ctx.rng.gen_bool(0.3)
                && let Some(current_site) = ctx.npc.current_site
                && let Some(current_site_name) = util::site_name(ctx, current_site)
            {
                Content::localized_with_args("npc-speech-site", [(
                    "site",
                    Content::Plain(current_site_name),
                )])

            // Mention nearby monsters
            } else if ctx.rng.gen_bool(0.3)
                && let Some(monster) = ctx
                    .state
                    .data()
                    .npcs
                    .values()
                    .filter(|other| matches!(&other.role, Role::Monster))
                    .min_by_key(|other| other.wpos.xy().distance(ctx.npc.wpos.xy()) as i32)
            {
                Content::localized_with_args("npc-speech-tell_monster", [
                    ("body", monster.body.localize_npc()),
                    (
                        "dir",
                        Direction::from_dir(monster.wpos.xy() - ctx.npc.wpos.xy()).localize_npc(),
                    ),
                    (
                        "dist",
                        Distance::from_length(monster.wpos.xy().distance(ctx.npc.wpos.xy()) as i32)
                            .localize_npc(),
                    ),
                ])
            // Specific night dialog
            } else if ctx.rng.gen_bool(0.6) && DayPeriod::from(ctx.time_of_day.0).is_dark() {
                Content::localized("npc-speech-night")
            } else if ctx.rng.gen_bool(0.3)
                && let Some(profession_comment) = match ctx.npc.profession() {
                    Some(Profession::Pirate(_)) => Some(Content::localized("npc-speech-pirate")),
                    _ => None,
                }
            {
                profession_comment
            } else {
                ctx.npc.personality.get_generic_comment(&mut ctx.rng)
            };
            // TODO: Don't special-case players
            let wait = if matches!(tgt, Actor::Character(_)) {
                0.0
            } else {
                1.5
            };
            idle()
                .repeat()
                .stop_if(timeout(wait))
                .then(just(move |ctx, _| ctx.controller.say(tgt, comment.clone())))
                .boxed()
        }
    })
}

fn socialize() -> impl Action<EveryRange> {
    now(move |ctx, socialize: &mut EveryRange| {
        // Skip most socialising actions if we're not loaded
        if matches!(ctx.npc.mode, SimulationMode::Loaded)
            && socialize.should(ctx)
            && !ctx.npc.personality.is(PersonalityTrait::Introverted)
        {
            // Sometimes dance
            if ctx.rng.gen_bool(0.15) {
                return just(|ctx, _| ctx.controller.do_dance(None))
                    .repeat()
                    .stop_if(timeout(6.0))
                    .debug(|| "dancing")
                    .map(|_, _| ())
                    .l()
                    .l();
            // Talk to nearby NPCs
            } else if let Some(other) = ctx
                .state
                .data()
                .npcs
                .nearby(Some(ctx.npc_id), ctx.npc.wpos, 8.0)
                .choose(&mut ctx.rng)
            {
                return smalltalk_to(other)
                    // After talking, wait for a while
                    .then(idle().repeat().stop_if(timeout(4.0)))
                    .map(|_, _| ())
                    .r().l();
            }
        }
        idle().r()
    })
}

fn pirate(is_leader: bool) -> impl Action<DefaultState> {
    choose(move |ctx, _| {
        let data = ctx.state.data();
        if is_leader
            && let Some(home) = ctx.npc.home
            && ctx.npc.current_site == Some(home)
            && let Some(site) = data.sites.get(home)
            && let Some(faction) = ctx.npc.faction
            // Approx. once an hour.
            && ctx.chance(1.0 / 3600.0)
            && let Some(site_to_raid) = site
                .nearby_sites_by_size
                .iter()
                .filter(|site| {
                    data.sites.get(**site).is_some_and(|site| {
                        // Don't go further than 10km
                        site.wpos.as_::<f32>().distance_squared(ctx.npc.wpos.xy())
                            < 10000.0f32.powi(2)
                    })
                })
                .choose(&mut ctx.rng)
                .copied()
            && site
                .population
                .iter()
                .filter(|npc_id| {
                    data.npcs.get(**npc_id).is_some_and(|npc| {
                        !npc.is_dead()
                            && npc.current_site == Some(home)
                            && npc.faction == Some(faction)
                            && npc.hiring.is_none()
                            && matches!(npc.role, Role::Civilised(Some(Profession::Pirate(false))))
                    })
                })
                .count()
                > 3
        {
            important(
                now(move |ctx, _| {
                    let data = ctx.state.data();
                    if let Some(site) = data.sites.get(home)
                        && let Some(npc) = site
                            .population
                            .iter()
                            .filter(|npc_id| {
                                data.npcs.get(**npc_id).is_some_and(|npc| {
                                    !npc.is_dead()
                                        && npc.current_site == Some(home)
                                        && npc.faction == Some(faction)
                                        && npc.hiring.is_none()
                                        && matches!(
                                            npc.role,
                                            Role::Civilised(Some(Profession::Pirate(false)))
                                        )
                                })
                            })
                            .choose(&mut ctx.rng)
                    {
                        let npc = *npc;
                        follow_actor(Actor::Npc(npc), 5.0)
                            .stop_if(move |ctx: &mut NpcCtx| {
                                let data = ctx.state.data();
                                let Some(follow_npc) = data.npcs.get(npc) else {
                                    return true;
                                };
                                ctx.npc.wpos.distance_squared(follow_npc.wpos) < 6.0f32.powi(2)
                            })
                            .then(just(move |ctx, _| {
                                let leader = ctx.npc_id;
                                ctx.controller.npc_dialogue(
                                    npc,
                                    Content::localized_with_args("npc-speech-pirate_raid", [(
                                        "site",
                                        util::site_name(ctx, site_to_raid).unwrap_or_default(),
                                    )]),
                                    idle().repeat().stop_if(timeout(2.0)).then(just(
                                        move |ctx, _| {
                                            let target = Actor::Npc(leader);
                                            ctx.controller.say(
                                                target,
                                                Content::localized("npc-response-accept_hire"),
                                            );
                                            ctx.controller.hiring = Some(Some((
                                                target,
                                                common::resources::Time(f64::INFINITY),
                                            )));
                                        },
                                    )),
                                );
                            }))
                            .debug(|| "inviting raid participant")
                            .l()
                    } else {
                        idle().r()
                    }
                })
                .repeat()
                .stop_if(move |ctx: &mut NpcCtx| {
                    let data = ctx.state.data();
                    if let Some(site) = data.sites.get(home) {
                        let hired_count = site
                            .population
                            .iter()
                            .filter(|npc_id| {
                                data.npcs.get(**npc_id).is_some_and(|npc| {
                                    !npc.is_dead()
                                        && npc
                                            .hiring
                                            .is_some_and(|(a, _)| a == Actor::Npc(ctx.npc_id))
                                })
                            })
                            .count();

                        let unhired_count = site
                            .population
                            .iter()
                            .filter(|npc_id| {
                                data.npcs.get(**npc_id).is_some_and(|npc| {
                                    !npc.is_dead()
                                        && npc.current_site == Some(home)
                                        && npc.faction == Some(faction)
                                        && npc.hiring.is_none()
                                        && matches!(
                                            npc.role,
                                            Role::Civilised(Some(Profession::Pirate(false)))
                                        )
                                })
                            })
                            .count();

                        if unhired_count == 0 {
                            return true;
                        }

                        let chance = match hired_count {
                            0..=3 => 0.0,
                            _ => (hired_count - 3) as f64 * 1.0 / 1200.0,
                        } / unhired_count as f64;

                        ctx.chance(chance)
                    } else {
                        true
                    }
                })
                .debug(|| "preparing for raid")
                .then(travel_to_site(site_to_raid, 0.8).debug(|| "travel to raid site"))
                .then(
                    // TODO: Replace this with raiding stuff
                    villager(site_to_raid)
                        .stop_if(timeout(ctx.rng.gen_range(60.0..120.0)))
                        .debug(|| "raiding"),
                )
                .then(travel_to_site(home, 0.6).debug(|| "traveling home from raid"))
                // End hiring of hirlings
                .then(just(|ctx, _| {
                    let data = ctx.state.data();
                    if let Some(site) = ctx.npc.home
                        && let Some(site) = data.sites.get(site)
                    {
                        for &npc_id in site.population.iter() {
                            if let Some(npc) = data.npcs.get(npc_id)
                                && npc
                                    .hiring
                                    .is_some_and(|(actor, _)| actor == Actor::Npc(ctx.npc_id))
                            {
                                ctx.controller
                                    .npc_action(npc_id, just(|ctx, _| ctx.controller.end_hiring()));
                            }
                        }
                    }
                }))
                .map(|_, _| ()),
            )
        } else if let Some((leader, _)) = ctx.npc.hiring {
            important(
                follow_actor(leader, 5.0)
                    .stop_if(move |ctx: &mut NpcCtx| {
                        ctx.npc.hiring.is_none_or(move |(actor, _)| actor != leader)
                    })
                    .map(|_, _| ()),
            )
        } else if let Some(home) = ctx.npc.home {
            casual(now(move |ctx, _| {
                let data = ctx.state.data();
                let pos = data.sites.get(home).and_then(|site| {
                    let ws = ctx.index.sites.get(site.world_site?);
                    let plot = ws
                        .filter_plots(|plot| matches!(plot.kind(), PlotKind::PirateHideout(_)))
                        .choose(&mut ctx.rng)?;
                    let tile = plot.tiles().choose(&mut ctx.rng)?;
                    let wpos = ws.tile_center_wpos(tile);

                    Some(wpos.as_())
                });
                // Choose a plaza in the site we're visiting to walk to
                if let Some(new_pos) = pos {
                    // Walk to a point in the hideout...
                    Either::Left(travel_to_point(new_pos, 0.5)
                        .debug(|| "walk to pirate hideout"))
                } else {
                    // If there is no pirate hideout, unset the home.
                    ctx.controller.set_new_home(None);
                    Either::Right(finish())
                }
                    // ...then socialize for some time before moving on
                    .then(socialize()
                        .repeat()
                        .map_state(|state: &mut DefaultState| &mut state.socialize_timer)
                        .stop_if(timeout(ctx.rng.gen_range(30.0..90.0)))
                        .debug(|| "wait at pirate hideout"))
                    .map(|_, _| ())
            }))
        } else {
            // Find new home
            important(just(move |ctx, _| {
                let data = ctx.state.data();
                if let Some((site, _)) =
                    data.sites
                        .iter()
                        .filter(|(_, site)| {
                            site.world_site.is_some_and(|ws| {
                                ctx.index.sites.get(ws).any_plot(|plot| {
                                    matches!(plot.kind(), PlotKind::PirateHideout(_))
                                })
                            })
                        })
                        .min_by_key(|(_, site)| {
                            site.wpos
                                .as_::<i64>()
                                .distance_squared(ctx.npc.wpos.xy().as_())
                        })
                {
                    ctx.controller.set_new_home(site);
                }
            }))
        }
    })
}

fn adventure() -> impl Action<DefaultState> {
    choose(|ctx, _| {
        // Choose a random site that's fairly close by
        if let Some(tgt_site) = ctx
            .state
            .data()
            .sites
            .iter()
            .filter(|(site_id, site)| {
                site.world_site.is_some_and(|ws| ctx.index.sites.get(ws).any_plot(|plot| matches!(plot.meta(), Some(PlotKindMeta::Workshop { .. })))) && (ctx.npc.current_site != Some(*site_id))
                    && ctx.rng.gen_bool(0.25)
            })
            .min_by_key(|(_, site)| site.wpos.as_().distance(ctx.npc.wpos.xy()) as i32)
            .map(|(site_id, _)| site_id)
        {
            let wait_time = if matches!(ctx.npc.profession(), Some(Profession::Merchant)) {
                60.0 * 15.0
            } else {
                60.0 * 3.0
            };
            let site_name = util::site_name(ctx, tgt_site).unwrap_or_default();
            // Travel to the site
            important(just(move |ctx, _| ctx.controller.say(None, Content::localized_with_args("npc-speech-moving_on", [("site", site_name.clone())])))
                          .then(travel_to_site(tgt_site, 0.6))
                          // Stop for a few minutes
                          .then(villager(tgt_site).repeat().stop_if(timeout(wait_time)))
                          .map(|_, _| ())
                          .boxed(),
            )
        } else {
            casual(finish().boxed())
        }
    })
    .debug(move || "adventure")
}

fn hired<S: State>(tgt: Actor) -> impl Action<S> {
    follow_actor(tgt, 5.0)
        // Stop following if we're no longer hired
        .stop_if(move |ctx: &mut NpcCtx| ctx.npc.hiring.is_none_or(|(a, _)| a != tgt))
        .debug(move|| format!("hired by {tgt:?}"))
        .interrupt_with(move |ctx, _| {
            // End hiring for various reasons
            if let Some((tgt, expires)) = ctx.npc.hiring {
                // Hiring period has expired
                if ctx.time > expires {
                    ctx.controller.end_hiring();
                    // If the actor exists, tell them that the hiring is over
                    if util::actor_exists(ctx, tgt) {
                        return Some(goto_actor(tgt, 2.0)
                            .then(do_dialogue(tgt, |session| {
                                session.say_statement(Content::localized("npc-dialogue-hire_expired"))
                            }))
                            .boxed());
                    }
                }

                if ctx.sentiments.toward(tgt).is(Sentiment::RIVAL) {
                    ctx.controller.end_hiring();
                    // If the actor exists, tell them that the hiring is over
                    if util::actor_exists(ctx, tgt) {
                        return Some(goto_actor(tgt, 2.0)
                            .then(do_dialogue(tgt, |session| {
                                session.say_statement(Content::localized(
                                    "npc-dialogue-hire_cancelled_unhappy",
                                ))
                            }))
                            .boxed());
                    }
                }
            }

            None
        })
        .map(|_, _| ())
}

fn gather_ingredients<S: State>() -> impl Action<S> {
    just(|ctx, _| {
        ctx.controller.do_gather(
            &[
                ChunkResource::Fruit,
                ChunkResource::Mushroom,
                ChunkResource::Plant,
            ][..],
        )
    })
    .debug(|| "gather ingredients")
}

fn hunt_animals<S: State>() -> impl Action<S> {
    just(|ctx, _| ctx.controller.do_hunt_animals()).debug(|| "hunt_animals")
}

fn find_forest(ctx: &mut NpcCtx) -> Option<Vec2<f32>> {
    let chunk_pos = ctx.npc.wpos.xy().as_().wpos_to_cpos();
    Spiral2d::new()
        .skip(ctx.rng.gen_range(1..=64))
        .take(24)
        .map(|rpos| chunk_pos + rpos)
        .find(|cpos| {
            ctx.world
                .sim()
                .get(*cpos)
                .is_some_and(|c| c.tree_density > 0.75 && c.surface_veg > 0.5)
        })
        .map(|chunk| TerrainChunkSize::center_wpos(chunk).as_())
}

fn find_farm(ctx: &mut NpcCtx, site: SiteId) -> Option<Vec2<f32>> {
    ctx.state.data().sites.get(site).and_then(|site| {
        let site = ctx.index.sites.get(site.world_site?);
        let farm = site
            .filter_plots(|p| matches!(p.kind(), PlotKind::FarmField(_)))
            .choose(&mut ctx.rng)?;

        Some(site.tile_center_wpos(farm.root_tile()).as_())
    })
}

fn choose_plaza(ctx: &mut NpcCtx, site: SiteId) -> Option<Vec2<f32>> {
    ctx.state.data().sites.get(site).and_then(|site| {
        let site = ctx.index.sites.get(site.world_site?);
        let plaza = &site.plots[site.plazas().choose(&mut ctx.rng)?];
        let tile = plaza
            .tiles()
            .choose(&mut ctx.rng)
            .unwrap_or_else(|| plaza.root_tile());
        Some(site.tile_center_wpos(tile).as_())
    })
}

const WALKING_SPEED: f32 = 0.35;

fn villager(visiting_site: SiteId) -> impl Action<DefaultState> {
    choose(move |ctx, state: &mut DefaultState| {
        // Consider moving home if the home site gets too full
        if state.move_home_timer.should(ctx)
            && let Some(home) = ctx.npc.home
            && Some(home) == ctx.npc.current_site
            && let Some(home_pop_ratio) = ctx.state.data().sites.get(home)
                .and_then(|site| Some((site, ctx.index.sites.get(site.world_site?))))
                .and_then(|(site, world_site)| { let houses = world_site.filter_plots(|p| matches!(p.meta(), Some(PlotKindMeta::House { .. }))).count(); if houses == 0 { return None } Some(site.population.len() as f32 / houses as f32) } )
                // Only consider moving if the population is more than 1.5x the number of homes
                .filter(|pop_ratio| *pop_ratio > 1.5)
            && let Some(new_home) = ctx
                .state
                .data()
                .sites
                .iter()
                // Don't try to move to the site that's currently our home
                .filter(|(site_id, _)| Some(*site_id) != ctx.npc.home)
                // Only consider towns as potential homes
                .filter_map(|(site_id, site)| {
                    let world_site = site.world_site.map(|ws| ctx.index.sites.get(ws))?;
                    let house_count = world_site.filter_plots(|p| matches!(p.meta(), Some(PlotKindMeta::House { .. }))).count();

                    if house_count == 0 {
                        return None;
                    }
                    Some((site_id, site, house_count))
                })
                // Only select sites that are less densely populated than our own
                .filter(|(_, site, houses)| (site.population.len() as f32 / *houses as f32) < home_pop_ratio)
                // Find the closest of the candidate sites
                .min_by_key(|(_, site, _)| site.wpos.as_().distance(ctx.npc.wpos.xy()) as i32)
                .map(|(site_id, _, _)| site_id)
        {
            let site_name = util::site_name(ctx, new_home);
            return important(just(move |ctx, _| {
                if let Some(site_name) = &site_name {
                    ctx.controller.say(None, Content::localized_with_args("npc-speech-migrating", [("site", site_name.clone())]))
                }
            })
                .then(travel_to_site(new_home, 0.5))
                .then(just(move |ctx, _| ctx.controller.set_new_home(new_home))));
        }
        let day_period = DayPeriod::from(ctx.time_of_day.0);
        let is_weekend = ctx.time_of_day.day() as u64 % 6 == 0;
        let is_evening = day_period == DayPeriod::Evening;

        let is_free_time = is_weekend || is_evening;

        let is_raining = ctx.system_data.weather_grid.is_raining(ctx.npc.wpos.xy());

        // Go to a house if it's dark
        if day_period.is_dark()
            && !matches!(ctx.npc.profession(), Some(Profession::Guard))
        {
            return important(
                now(move |ctx, _| {
                    if let Some(house_wpos) = ctx
                        .state
                        .data()
                        .sites
                        .get(visiting_site)
                        .and_then(|site| Some(ctx.index.sites.get(site.world_site?)))
                        .and_then(|site| {
                            // Find a house in the site we're visiting
                            let house = site
                                .plots()
                                .filter(|p| matches!(p.kind().meta(), Some(PlotKindMeta::House { .. })))
                                .choose(&mut ctx.rng)?;
                            Some(site.tile_center_wpos(house.root_tile()).as_())
                        })
                    {
                        just(|ctx, _| {
                            ctx.controller
                                .say(None, Content::localized("npc-speech-night_time"))
                        })
                        .then(travel_to_point(house_wpos, 0.65))
                        .debug(|| "walk to house")
                        .then(socialize().repeat().map_state(|state: &mut DefaultState| &mut state.socialize_timer).debug(|| "wait in house"))
                        .stop_if(|ctx: &mut NpcCtx| DayPeriod::from(ctx.time_of_day.0).is_light())
                        .then(just(|ctx, _| {
                            ctx.controller
                                .say(None, Content::localized("npc-speech-day_time"))
                        }))
                        .map(|_, _| ())
                        .boxed()
                    } else {
                        finish().boxed()
                    }
                })
                .debug(|| "find somewhere to sleep"),
            );
        }
        // Go to a house if its raining
        else if is_raining
            && !matches!(ctx.npc.profession(), Some(Profession::Guard))
        {
            return important(
                now(move |ctx, _| {
                    if let Some(house_wpos) = ctx
                        .state
                        .data()
                        .sites
                        .get(visiting_site)
                        .and_then(|site| Some(ctx.index.sites.get(site.world_site?)))
                        .and_then(|site| {
                            // Find a house in the site we're visiting
                            let house = site
                                .plots()
                                .filter(|p| matches!(p.kind().meta(), Some(PlotKindMeta::House { .. })))
                                .choose(&mut ctx.rng)?;
                            Some(site.tile_center_wpos(house.root_tile()).as_())
                        })
                    {
                        just(|ctx, _| {
                                ctx.controller.say(None, Content::localized("npc-speech-seeking_shelter_rain"))
                        })
                        .then(travel_to_point(house_wpos, 0.65))
                        .debug(|| "walk to house (rain)")
                        .then(socialize().repeat().map_state(|state: &mut DefaultState| &mut state.socialize_timer).debug(|| "wait in house (rain)"))
                        .stop_if(|ctx: &mut NpcCtx| {
                                    let is_raining = ctx.system_data.weather_grid.is_raining(ctx.npc.wpos.xy());
                                    !is_raining
                    })
                        .then(just(|ctx, _| {
                                ctx.controller.say(None, Content::localized("npc-speech-rain_stopped"))
                        }))
                        .map(|_, _| ())
                        .boxed()
                        } else {
                        finish().boxed()
                    }
                })
                .debug(|| "find somewhere to wait (rain)"),
            );
        }
        // Go do something fun on evenings and holidays, or on random days.
        else if
            // Ain't no rest for the wicked
            !matches!(ctx.npc.profession(), Some(Profession::Guard | Profession::Chef))
            && (matches!(day_period, DayPeriod::Evening) || is_free_time || ctx.rng.gen_bool(0.05)) {
            let mut fun_activities = Vec::new();

            if let Some(ws_id) = ctx.state.data().sites[visiting_site].world_site {
                let ws = ctx.index.sites.get(ws_id);
                if let Some(arena) = ws.plots().find_map(|p| match p.kind() { PlotKind::DesertCityArena(a) => Some(a), _ => None}) {
                    let wait_time = ctx.rng.gen_range(100.0..300.0);
                    // We don't use Z coordinates for seats because they are complicated to calculate from the Ramp procedural generation
                    // and using goto_2d seems to work just fine. However it also means that NPC will never go seat on the stands
                    // on the first floor of the arena. This is a compromise that was made because in the current arena procedural generation
                    // there is also no pathways to the stands on the first floor for NPCs.
                    let arena_center = Vec3::new(arena.center.x, arena.center.y, arena.base).as_::<f32>();
                    let stand_dist = arena.stand_dist as f32;
                    let seat_var_width = ctx.rng.gen_range(0..arena.stand_width) as f32;
                    let seat_var_length = ctx.rng.gen_range(-arena.stand_length..arena.stand_length) as f32;
                    // Select a seat on one of the 4 arena stands
                    let seat = match ctx.rng.gen_range(0..4) {
                        0 => Vec3::new(arena_center.x - stand_dist + seat_var_width, arena_center.y + seat_var_length, arena_center.z),
                        1 => Vec3::new(arena_center.x + stand_dist - seat_var_width, arena_center.y + seat_var_length, arena_center.z),
                        2 => Vec3::new(arena_center.x + seat_var_length, arena_center.y - stand_dist + seat_var_width, arena_center.z),
                        _ => Vec3::new(arena_center.x + seat_var_length, arena_center.y + stand_dist - seat_var_width, arena_center.z),
                    };
                    let look_dir = Dir::from_unnormalized(arena_center - seat);
                    // Walk to an arena seat, cheer, sit and dance
                    let action = casual(just(move |ctx, _| ctx.controller.say(None, Content::localized("npc-speech-arena")))
                            .then(goto_2d(seat.xy(), 0.6, 1.0).debug(|| "go to arena"))
                            // Turn toward the centre of the arena and watch the action!
                            .then(choose(move |ctx, _| if ctx.rng.gen_bool(0.3) {
                                casual(just(move |ctx,_| ctx.controller.do_cheer(look_dir)).repeat().stop_if(timeout(5.0)))
                            } else if ctx.rng.gen_bool(0.15) {
                                casual(just(move |ctx,_| ctx.controller.do_dance(look_dir)).repeat().stop_if(timeout(5.0)))
                            } else {
                                casual(just(move |ctx,_| ctx.controller.do_sit(look_dir, None)).repeat().stop_if(timeout(15.0)))
                            })
                                .repeat()
                                .stop_if(timeout(wait_time)))
                            .map(|_, _| ())
                            .boxed());
                    fun_activities.push(action);
                }
                if let Some(tavern) = ws.plots().filter_map(|p| match p.kind() {  PlotKind::Tavern(a) => Some(a), _ => None }).choose(&mut ctx.rng) {
                    let tavern_name = tavern.name.clone();
                    let wait_time = ctx.rng.gen_range(100.0..300.0);

                    let (stage_aabr, stage_z) = tavern.rooms.values().flat_map(|room| {
                        room.details.iter().filter_map(|detail| match detail {
                            tavern::Detail::Stage { aabr } => Some((*aabr, room.bounds.min.z + 1)),
                            _ => None,
                        })
                    }).choose(&mut ctx.rng).unwrap_or((tavern.bounds, tavern.door_wpos.z));

                    let bar_pos = tavern.rooms.values().flat_map(|room|
                        room.details.iter().filter_map(|detail| match detail {
                            tavern::Detail::Bar { aabr } => {
                                let side = site::util::Dir::from_vec2(room.bounds.center().xy() - aabr.center());
                                let pos = side.select_aabr_with(*aabr, aabr.center()) + side.to_vec2();

                                Some(pos.with_z(room.bounds.min.z))
                            }
                            _ => None,
                        })
                    ).choose(&mut ctx.rng).unwrap_or(stage_aabr.center().with_z(stage_z));

                    // Pick a chair that is theirs for the stay
                    let chair_pos = tavern.rooms.values().flat_map(|room| {
                        let z = room.bounds.min.z;
                        room.details.iter().filter_map(move |detail| match detail {
                            tavern::Detail::Table { pos, chairs } => Some(chairs.into_iter().map(move |dir| pos.with_z(z) + dir.to_vec2())),
                            _ => None,
                        })
                        .flatten()
                    }
                    ).choose(&mut ctx.rng)
                    // This path is possible, but highly unlikely.
                    .unwrap_or(bar_pos);

                    let stage_aabr = stage_aabr.as_::<f32>();
                    let stage_z = stage_z as f32;

                    let action = casual(travel_to_point(tavern.door_wpos.xy().as_() + 0.5, 0.8).then(choose(move |ctx, (last_action, _)| {
                            let action = [0, 1, 2].into_iter().filter(|i| *last_action != Some(*i)).choose(&mut ctx.rng).expect("We have at least 2 elements");
                            let socialize_repeat = || socialize().map_state(|(_, timer)| timer).repeat();
                            match action {
                                // Go and dance on a stage.
                                0 => {
                                    casual(
                                        now(move |ctx, (last_action, _)| {
                                            *last_action = Some(action);
                                            goto(stage_aabr.min.map2(stage_aabr.max, |a, b| ctx.rng.gen_range(a..b)).with_z(stage_z), WALKING_SPEED, 1.0)
                                        })
                                        .then(just(move |ctx,_| ctx.controller.do_dance(None)).repeat().stop_if(timeout(ctx.rng.gen_range(20.0..30.0))))
                                        .map(|_, _| ())
                                        .debug(|| "Dancing on the stage")
                                    )
                                },
                                // Go and sit at a table.
                                1 => {
                                    casual(
                                        now(move |ctx, (last_action, _)| {
                                            *last_action = Some(action);
                                            goto(chair_pos.as_() + 0.5, WALKING_SPEED, 1.0)
                                                .then(just(move |ctx, _| ctx.controller.do_sit(None, Some(chair_pos)))
                                                    // .then(socialize().map_state(|(_, timer)| timer))
                                                    .repeat().stop_if(timeout(ctx.rng.gen_range(30.0..60.0)))
                                                )
                                                .map(|_, _| ())
                                        })
                                        .debug(move || format!("Sitting in a chair at {} {} {}", chair_pos.x, chair_pos.y, chair_pos.z))
                                    )
                                },
                                // Go to the bar.
                                _ => {
                                    casual(
                                        now(move |ctx, (last_action, _)| {
                                            *last_action = Some(action);
                                            goto(bar_pos.as_() + 0.5, WALKING_SPEED, 1.0).then(socialize_repeat().stop_if(timeout(ctx.rng.gen_range(10.0..25.0)))).map(|_, _| ())
                                        }).debug(|| "At the bar")
                                    )
                                },
                            }
                        })
                        .with_state((None::<u32>, every_range(5.0..10.0)))
                        .repeat()
                        .stop_if(timeout(wait_time)))
                        .map(|_, _| ())
                        .debug(move || format!("At the tavern '{}'", tavern_name))
                        .boxed()
                    );

                    fun_activities.push(action);
                }
            }


            if !fun_activities.is_empty() {
                let i = ctx.rng.gen_range(0..fun_activities.len());
                return fun_activities.swap_remove(i);
            }
        }
        // Villagers with roles should perform those roles
        else if matches!(ctx.npc.profession(), Some(Profession::Herbalist)) && ctx.rng.gen_bool(0.8)
        {
            if let Some(forest_wpos) = find_forest(ctx) {
                return casual(
                    travel_to_point(forest_wpos, 0.5)
                        .debug(|| "walk to forest")
                        .then({
                            let wait_time = ctx.rng.gen_range(10.0..30.0);
                            gather_ingredients().repeat().stop_if(timeout(wait_time))
                        })
                        .map(|_, _| ()),
                );
            }
        } else if matches!(ctx.npc.profession(), Some(Profession::Farmer)) && ctx.rng.gen_bool(0.8)
        {
            if let Some(farm_wpos) = find_farm(ctx, visiting_site) {
                return casual(
                    travel_to_point(farm_wpos, 0.5)
                        .debug(|| "walk to farm")
                        .then({
                            let wait_time = ctx.rng.gen_range(30.0..120.0);
                            gather_ingredients().repeat().stop_if(timeout(wait_time))
                        })
                        .map(|_, _| ()),
                );
            }
        } else if matches!(ctx.npc.profession(), Some(Profession::Hunter)) && ctx.rng.gen_bool(0.8) {
            if let Some(forest_wpos) = find_forest(ctx) {
                return casual(
                    just(|ctx, _| {
                        ctx.controller
                            .say(None, Content::localized("npc-speech-start_hunting"))
                    })
                    .then(travel_to_point(forest_wpos, 0.75))
                    .debug(|| "walk to forest")
                    .then({
                        let wait_time = ctx.rng.gen_range(30.0..60.0);
                        hunt_animals().repeat().stop_if(timeout(wait_time))
                    })
                    .map(|_, _| ()),
                );
            }
        } else if matches!(ctx.npc.profession(), Some(Profession::Guard)) && ctx.rng.gen_bool(0.7) {
            if let Some(plaza_wpos) = choose_plaza(ctx, visiting_site) {
                return casual(
                    travel_to_point(plaza_wpos, 0.4)
                        .debug(|| "patrol")
                        .interrupt_with(move |ctx, _| {
                            if ctx.rng.gen_bool(0.0003) {
                                Some(just(move |ctx, _| {
                                    ctx.controller
                                        .say(None, Content::localized("npc-speech-guard_thought"))
                                }))
                            } else {
                                None
                            }
                        })
                        .map(|_, _| ()),
                );
            }
        } else if matches!(ctx.npc.profession(), Some(Profession::Merchant)) && ctx.rng.gen_bool(0.8)
        {
            return casual(
                just(|ctx, _| {
                    // Try to direct our speech at nearby actors, if there are any
                    let (target, phrase) = if ctx.rng.gen_bool(0.3) && let Some(other) = ctx
                        .state
                        .data()
                        .npcs
                        .nearby(Some(ctx.npc_id), ctx.npc.wpos, 8.0)
                        .choose(&mut ctx.rng)
                    {
                        (Some(other), "npc-speech-merchant_sell_directed")
                    } else {
                        // Otherwise, resort to generic expressions
                        (None, "npc-speech-merchant_sell_undirected")
                    };

                    ctx.controller.say(target, Content::localized(phrase));
                })
                .then(idle().repeat().stop_if(timeout(8.0)))
                .repeat()
                .stop_if(timeout(60.0))
                .debug(|| "sell wares")
                .map(|_, _| ()),
            );
        } else if matches!(ctx.npc.profession(), Some(Profession::Chef))
            && ctx.rng.gen_bool(0.8)
            && let Some(ws_id) = ctx.state.data().sites[visiting_site].world_site
            && let Some(tavern) = ctx.index.sites.get(ws_id).plots().filter_map(|p| match p.kind() {  PlotKind::Tavern(a) => Some(a), _ => None }).choose(&mut ctx.rng)
            && let Some((bar_pos, room_center)) = tavern.rooms.values().flat_map(|room|
                room.details.iter().filter_map(|detail| match detail {
                    tavern::Detail::Bar { aabr } => {
                        let center = aabr.center();
                        Some((center.with_z(room.bounds.min.z), room.bounds.center().xy()))
                    }
                    _ => None,
                })
            ).choose(&mut ctx.rng) {

            let face_dir = Dir::from_unnormalized((room_center - bar_pos).as_::<f32>().with_z(0.0)).unwrap_or_else(|| Dir::random_2d(&mut ctx.rng));

            return casual(
                travel_to_point(tavern.door_wpos.xy().as_(), 0.5)
                    .then(goto(bar_pos.as_() + Vec2::new(0.5, 0.5), WALKING_SPEED, 2.0))
                    // TODO: Just dance there for now, in the future do other stuff.
                    .then(just(move |ctx, _| ctx.controller.do_dance(Some(face_dir))).repeat().stop_if(timeout(60.0)))
                    .debug(|| "cook food").map(|_, _| ())
            )
        }

        // If nothing else needs doing, walk between plazas and socialize
        casual(now(move |ctx, _| {
            // Choose a plaza in the site we're visiting to walk to
            if let Some(plaza_wpos) = choose_plaza(ctx, visiting_site) {
                // Walk to the plaza...
                Either::Left(travel_to_point(plaza_wpos, 0.5)
                    .debug(|| "walk to plaza"))
            } else {
                // No plazas? :(
                Either::Right(finish())
            }
                // ...then socialize for some time before moving on
                .then(socialize()
                    .repeat()
                    .map_state(|state: &mut DefaultState| &mut state.socialize_timer)
                    .stop_if(timeout(ctx.rng.gen_range(30.0..90.0)))
                    .debug(|| "wait at plaza"))
                .map(|_, _| ())
        }))
    })
    .debug(move || format!("villager at site {:?}", visiting_site))
}

fn pilot<S: State>(ship: common::comp::ship::Body) -> impl Action<S> {
    // Travel between different towns in a straight line
    now(move |ctx, _| {
        let data = &*ctx.state.data();
        let station_wpos = data
            .sites
            .iter()
            .filter(|(id, _)| Some(*id) != ctx.npc.current_site)
            .filter_map(|(_, site)| Some(ctx.index.sites.get(site.world_site?)))
            .flat_map(|site| {
                site.filter_plots(|plot| {
                    matches!(plot.kind().meta(), Some(PlotKindMeta::AirshipDock { .. }))
                })
                .map(|plot| site.tile_center_wpos(plot.root_tile()))
            })
            .choose(&mut ctx.rng);
        if let Some(station_wpos) = station_wpos {
            Either::Right(
                goto_2d_flying(
                    station_wpos.as_(),
                    1.0,
                    50.0,
                    150.0,
                    110.0,
                    ship.flying_height(),
                )
                .then(goto_2d_flying(
                    station_wpos.as_(),
                    1.0,
                    10.0,
                    32.0,
                    16.0,
                    30.0,
                )),
            )
        } else {
            Either::Left(finish())
        }
    })
    .repeat()
    .map(|_, _| ())
}

fn captain<S: State>() -> impl Action<S> {
    // For now just randomly travel the sea
    now(|ctx, _| {
        let chunk = ctx.npc.wpos.xy().as_().wpos_to_cpos();
        if let Some(chunk) = NEIGHBORS
            .into_iter()
            .map(|neighbor| chunk + neighbor)
            .filter(|neighbor| {
                ctx.world
                    .sim()
                    .get(*neighbor)
                    .is_some_and(|c| c.river.river_kind.is_some())
            })
            .choose(&mut ctx.rng)
        {
            let wpos = TerrainChunkSize::center_wpos(chunk);
            let wpos = wpos.as_().with_z(
                ctx.world
                    .sim()
                    .get_interpolated(wpos, |chunk| chunk.water_alt)
                    .unwrap_or(0.0),
            );
            goto(wpos, 0.7, 5.0).boxed()
        } else {
            idle().boxed()
        }
    })
    .repeat()
    .map(|_, _| ())
}

fn check_inbox<S: State>(ctx: &mut NpcCtx) -> Option<impl Action<S> + use<S>> {
    let mut action = None;
    ctx.inbox.retain(|input| {
        match input {
            NpcInput::Report(report_id) if !ctx.known_reports.contains(report_id) => {
                let data = ctx.state.data();
                let Some(report) = data.reports.get(*report_id) else {
                    return false;
                };

                const REPORT_RESPONSE_TIME: f64 = 60.0 * 5.0;

                match report.kind {
                    ReportKind::Death { killer, actor, .. }
                        if matches!(&ctx.npc.role, Role::Civilised(_)) =>
                    {
                        // TODO: Don't report self
                        let phrase = if let Some(killer) = killer {
                            // TODO: For now, we don't make sentiment changes if the killer was an
                            // NPC in some cases because some NPCs can't hurt one-another.
                            // This should be changed in the future.
                            let can_damage_killer = if let Actor::Npc(killer) = killer {
                                data.npcs.get(killer).is_some_and(|killer| {
                                    match (&ctx.npc.role, &killer.role) {
                                        (Role::Vehicle, _) | (_, Role::Vehicle) => false,
                                        (Role::Civilised(prof_a), Role::Civilised(prof_b)) => {
                                            match (prof_a, prof_b) {
                                                (
                                                    Some(
                                                        Profession::Pirate(_) | Profession::Cultist,
                                                    ),
                                                    Some(
                                                        Profession::Pirate(_) | Profession::Cultist,
                                                    ),
                                                ) => false,
                                                (
                                                    Some(
                                                        Profession::Pirate(_) | Profession::Cultist,
                                                    ),
                                                    _,
                                                )
                                                | (
                                                    _,
                                                    Some(
                                                        Profession::Pirate(_) | Profession::Cultist,
                                                    ),
                                                ) => true,

                                                _ => false,
                                            }
                                        },
                                        (Role::Civilised(_), _) => true,
                                        (Role::Wild, Role::Wild) => false,
                                        (Role::Wild, _) => true,
                                        (Role::Monster, Role::Monster) => false,
                                        (Role::Monster, _) => true,
                                    }
                                })
                            } else {
                                true
                            };

                            // TODO: Roles themselves are kind of a hack, and so is this. This is
                            // mostly a fix for npcs getting angry if you kill for example an ogre.
                            let is_victim_inherent_enemy = if let Actor::Npc(victim) = actor {
                                data.npcs.get(victim).is_some_and(|victim| {
                                    match (&ctx.npc.role, &victim.role) {
                                        (Role::Civilised(prof), Role::Civilised(victim_prof)) => {
                                            match (prof, victim_prof) {
                                                (
                                                    Some(
                                                        Profession::Pirate(_) | Profession::Cultist,
                                                    ),
                                                    Some(
                                                        Profession::Pirate(_) | Profession::Cultist,
                                                    ),
                                                ) => false,
                                                (
                                                    Some(
                                                        Profession::Pirate(_) | Profession::Cultist,
                                                    ),
                                                    _,
                                                )
                                                | (
                                                    _,
                                                    Some(
                                                        Profession::Pirate(_) | Profession::Cultist,
                                                    ),
                                                ) => true,

                                                _ => false,
                                            }
                                        },

                                        (Role::Civilised(_), Role::Monster) => true,
                                        _ => false,
                                    }
                                })
                            } else {
                                false
                            };

                            let is_victim_enemy = is_victim_inherent_enemy
                                || ctx.sentiments.toward(actor).is(Sentiment::ENEMY);

                            if can_damage_killer {
                                // TODO: Don't hard-code sentiment change
                                let change = if is_victim_enemy {
                                    // Like the killer if we have negative sentiment towards the
                                    // killed.
                                    0.25
                                } else {
                                    -0.75
                                };
                                ctx.sentiments
                                    .toward_mut(killer)
                                    .change_by(change, Sentiment::VILLAIN);
                            }

                            // This is a murder of a player. Feel bad for the player and stop
                            // attacking them.
                            if let Actor::Character(_) = actor {
                                ctx.sentiments
                                    .toward_mut(actor)
                                    .limit_below(Sentiment::ENEMY)
                            }

                            if is_victim_enemy {
                                "npc-speech-witness_enemy_murder"
                            } else {
                                "npc-speech-witness_murder"
                            }
                        } else {
                            "npc-speech-witness_death"
                        };
                        ctx.known_reports.insert(*report_id);

                        if ctx.time_of_day.0 - report.at_tod.0 < REPORT_RESPONSE_TIME {
                            action = Some(
                                just(move |ctx, _| {
                                    ctx.controller.say(killer, Content::localized(phrase))
                                })
                                .l()
                                .l(),
                            );
                        }
                        false
                    },
                    ReportKind::Theft {
                        thief,
                        site,
                        sprite,
                    } => {
                        // Check if this happened at home, where we know what belongs to who
                        if let Some(site) = site
                            && ctx.npc.home == Some(site)
                        {
                            // TODO: Don't hardcode sentiment change.
                            ctx.sentiments
                                .toward_mut(thief)
                                .change_by(-0.2, Sentiment::ENEMY);
                            ctx.known_reports.insert(*report_id);

                            let phrase = if matches!(ctx.npc.profession(), Some(Profession::Farmer))
                                && matches!(sprite.category(), sprite::Category::Plant)
                            {
                                "npc-speech-witness_theft_owned"
                            } else {
                                "npc-speech-witness_theft"
                            };

                            if ctx.time_of_day.0 - report.at_tod.0 < REPORT_RESPONSE_TIME {
                                action = Some(
                                    just(move |ctx, _| {
                                        ctx.controller.say(thief, Content::localized(phrase))
                                    })
                                    .r()
                                    .l(),
                                );
                            }
                        }
                        false
                    },
                    // We don't care about deaths of non-civilians
                    ReportKind::Death { .. } => false,
                }
            },
            NpcInput::Report(_) => false, // Reports we already know of are ignored
            NpcInput::Interaction(by) => {
                action = Some(talk_to(*by).r());
                false
            },
            // Dialogue inputs get retained because they're handled by specific conversation actions
            // later
            NpcInput::Dialogue(_, _) => true,
        }
    });

    action
}

fn check_for_enemies<S: State>(ctx: &mut NpcCtx) -> Option<impl Action<S> + use<S>> {
    // TODO: Instead of checking all nearby actors every tick, it would be more
    // effective to have the actor grid generate a per-tick diff so that we only
    // need to check new actors in the local area. Be careful though:
    // implementing this means accounting for changes in sentiment (that could
    // suddenly make a nearby actor an enemy) as well as variable NPC tick
    // rates!
    ctx.state
        .data()
        .npcs
        .nearby(Some(ctx.npc_id), ctx.npc.wpos, 24.0)
        .find(|actor| ctx.sentiments.toward(*actor).is(Sentiment::ENEMY))
        .map(|enemy| just(move |ctx, _| ctx.controller.attack(enemy)))
}

fn react_to_events<S: State>(ctx: &mut NpcCtx, _: &mut S) -> Option<impl Action<S> + use<S>> {
    check_inbox::<S>(ctx)
        .map(|action| action.boxed())
        .or_else(|| check_for_enemies(ctx).map(|action| action.boxed()))
}

fn humanoid() -> impl Action<DefaultState> {
    choose(|ctx, _| {
        if let Some(riding) = &ctx.state.data().npcs.mounts.get_mount_link(ctx.npc_id) {
            if riding.is_steering {
                if let Some(vehicle) = ctx.state.data().npcs.get(riding.mount) {
                    match vehicle.body {
                        comp::Body::Ship(body @ comp::ship::Body::AirBalloon) => {
                            important(pilot(body))
                        },
                        comp::Body::Ship(comp::ship::Body::DefaultAirship) => {
                            important(airship_ai::pilot_airship())
                        },
                        comp::Body::Ship(
                            comp::ship::Body::SailBoat | comp::ship::Body::Galleon,
                        ) => important(captain()),
                        _ => casual(idle()),
                    }
                } else {
                    casual(finish())
                }
            } else {
                important(
                    socialize().map_state(|state: &mut DefaultState| &mut state.socialize_timer),
                )
            }
        } else if let Some((tgt, _)) = ctx.npc.hiring
            && util::actor_exists(ctx, tgt)
        {
            important(hired(tgt).interrupt_with(react_to_events))
        } else {
            let action = match ctx.npc.profession() {
                Some(Profession::Adventurer(_) | Profession::Merchant) => adventure().l().l(),
                Some(Profession::Pirate(is_leader)) => pirate(is_leader).l().r(),
                _ => {
                    if let Some(home) = ctx.npc.home {
                        villager(home).r().l()
                    } else {
                        idle().r().r() // Homeless
                    }
                },
            };

            casual(action.interrupt_with(react_to_events))
        }
    })
}

fn bird_large() -> impl Action<DefaultState> {
    now(|ctx, bearing: &mut Vec2<f32>| {
        *bearing = bearing
            .map(|e| e + ctx.rng.gen_range(-0.1..0.1))
            .try_normalized()
            .unwrap_or_default();
        let bearing_dist = 15.0;
        let mut pos = ctx.npc.wpos.xy() + *bearing * bearing_dist;
        let is_deep_water =
            matches!(ctx.npc.body, common::comp::Body::BirdLarge(b) if matches!(b.species, bird_large::Species::SeaWyvern))
                || ctx
                .world
                .sim()
                .get(pos.as_().wpos_to_cpos()).is_none_or(|c| {
                    c.alt - c.water_alt < -120.0 && (c.river.is_ocean() || c.river.is_lake())
                });
        if is_deep_water {
            *bearing *= -1.0;
            pos = ctx.npc.wpos.xy() + *bearing * bearing_dist;
        };
        // when high tree_density fly high, otherwise fly low-mid
        let npc_pos = ctx.npc.wpos.xy();
        let trees = ctx
            .world
            .sim()
            .get(npc_pos.as_().wpos_to_cpos()).is_some_and(|c| c.tree_density > 0.1);
        let height_factor = if trees {
            2.0
        } else {
            ctx.rng.gen_range(0.4..0.9)
        };

        let data = ctx.state.data();
        // without destination site fly to next waypoint
        let mut dest_site = pos;
        if let Some(home) = ctx.npc.home {
            let is_home = ctx.npc.current_site == Some(home);
            if is_home {
                if let Some((id, _)) = data
                    .sites
                    .iter()
                    .filter(|(id, site)| {
                        *id != home
                            && site.world_site.is_some_and(|site| {
                            match ctx.npc.body {
                                common::comp::Body::BirdLarge(b) => match b.species {
                                    bird_large::Species::Phoenix => matches!(ctx.index.sites.get(site).kind,
                                    Some(SiteKind::Terracotta
                                    | SiteKind::Haniwa
                                    | SiteKind::Myrmidon
                                    | SiteKind::Adlet
                                    | SiteKind::DwarvenMine
                                    | SiteKind::ChapelSite
                                    | SiteKind::Cultist
                                    | SiteKind::Gnarling
                                    | SiteKind::Sahagin
                                    | SiteKind::VampireCastle)),
                                    bird_large::Species::Cockatrice => matches!(ctx.index.sites.get(site).kind,
                                    Some(SiteKind::GiantTree)),
                                    bird_large::Species::Roc => matches!(ctx.index.sites.get(site).kind,
                                    Some(SiteKind::Haniwa
                                    | SiteKind::Cultist)),
                                    bird_large::Species::FlameWyvern => matches!(ctx.index.sites.get(site).kind,
                                    Some(SiteKind::DwarvenMine
                                    | SiteKind::Terracotta)),
                                    bird_large::Species::CloudWyvern => matches!(ctx.index.sites.get(site).kind,
                                    Some(SiteKind::ChapelSite
                                    | SiteKind::Sahagin)),
                                    bird_large::Species::FrostWyvern => matches!(ctx.index.sites.get(site).kind,
                                    Some(SiteKind::Adlet
                                    | SiteKind::Myrmidon)),
                                    bird_large::Species::SeaWyvern => matches!(ctx.index.sites.get(site).kind,
                                    Some(SiteKind::ChapelSite
                                    | SiteKind::Sahagin)),
                                    bird_large::Species::WealdWyvern => matches!(ctx.index.sites.get(site).kind,
                                    Some(SiteKind::GiantTree
                                    | SiteKind::Gnarling)),
                                },
                                _ => matches!(&ctx.index.sites.get(site).kind, Some(SiteKind::GiantTree)),
                            }
                        })
                    })
                    /*choose closest destination:
                    .min_by_key(|(_, site)| site.wpos.as_().distance(npc_pos) as i32)*/
                //choose random destination:
                .choose(&mut ctx.rng)
                {
                    ctx.controller.set_new_home(id)
                }
            } else if let Some(site) = data.sites.get(home) {
                dest_site = site.wpos.as_::<f32>()
            }
        }
        goto_2d_flying(
            pos,
            0.2,
            bearing_dist,
            8.0,
            8.0,
            ctx.npc.body.flying_height() * height_factor,
        )
            // If we are too far away from our waypoint position we can stop since we aren't going to a specific place.
            // If waypoint position is further away from destination site find a new waypoint
            .stop_if(move |ctx: &mut NpcCtx| {
                ctx.npc.wpos.xy().distance_squared(pos) > (bearing_dist + 5.0).powi(2)
                    || dest_site.distance_squared(pos) > dest_site.distance_squared(npc_pos)
            })
            // If waypoint position wasn't reached within 10 seconds we're probably stuck and need to find a new waypoint.
            .stop_if(timeout(10.0))
            .debug({
                let bearing = *bearing;
                move || format!("Moving with a bearing of {:?}", bearing)
            })
    })
        .repeat()
        .with_state(Vec2::<f32>::zero())
        .map(|_, _| ())
}

fn monster() -> impl Action<DefaultState> {
    now(|ctx, bearing: &mut Vec2<f32>| {
        *bearing = bearing
            .map(|e| e + ctx.rng.gen_range(-0.1..0.1))
            .try_normalized()
            .unwrap_or_default();
        let bearing_dist = 24.0;
        let mut pos = ctx.npc.wpos.xy() + *bearing * bearing_dist;
        let is_deep_water = ctx
            .world
            .sim()
            .get(pos.as_().wpos_to_cpos())
            .is_none_or(|c| {
                c.alt - c.water_alt < -10.0 && (c.river.is_ocean() || c.river.is_lake())
            });
        if !is_deep_water {
            goto_2d(pos, 0.7, 8.0)
        } else {
            *bearing *= -1.0;

            pos = ctx.npc.wpos.xy() + *bearing * 24.0;

            goto_2d(pos, 0.7, 8.0)
        }
        // If we are too far away from our goal position we can stop since we aren't going to a specific place.
        .stop_if(move |ctx: &mut NpcCtx| {
            ctx.npc.wpos.xy().distance_squared(pos) > (bearing_dist + 5.0).powi(2)
        })
        .debug({
            let bearing = *bearing;
            move || format!("Moving with a bearing of {:?}", bearing)
        })
    })
    .repeat()
    .with_state(Vec2::<f32>::zero())
    .map(|_, _| ())
}

fn think() -> impl Action<DefaultState> {
    now(|ctx, _| match ctx.npc.body {
        common::comp::Body::Humanoid(_) => humanoid().l().l().l(),
        common::comp::Body::BirdLarge(_) => bird_large().r().l().l(),
        _ => match &ctx.npc.role {
            Role::Civilised(_) => socialize()
                .map_state(|state: &mut DefaultState| &mut state.socialize_timer)
                .l()
                .r()
                .l(),
            Role::Monster => monster().r().r().l(),
            Role::Wild => idle().r(),
            Role::Vehicle => idle().r(),
        },
    })
    .interrupt_with(|ctx, _| {
        if let Some((_from, action)) = ctx.npc_dialogue.pop_front() {
            Some(action.with_state(()))
        } else {
            None
        }
    })
}
