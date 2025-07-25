// We'd like to not have this file in `common`, but sadly there are
// things in `common` that require it (currently, `ServerEvent` and
// `Agent`). When possible, this should be moved to the `rtsim`
// module in `server`.

use crate::{
    character::CharacterId,
    comp::{agent::FlightMode, inventory::item::ItemDef},
    util::Dir,
};
use common_i18n::Content;
use rand::{Rng, seq::IteratorRandom};
use serde::{Deserialize, Serialize};
use specs::Component;
use std::{collections::VecDeque, sync::Arc};
use strum::{EnumIter, IntoEnumIterator};
use vek::*;

slotmap::new_key_type! { pub struct NpcId; }

slotmap::new_key_type! { pub struct SiteId; }

slotmap::new_key_type! { pub struct FactionId; }

slotmap::new_key_type! { pub struct ReportId; }

slotmap::new_key_type! { pub struct AirshipRouteId; }

#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub struct RtSimEntity(pub NpcId);

impl Component for RtSimEntity {
    type Storage = specs::VecStorage<Self>;
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Actor {
    Npc(NpcId),
    Character(CharacterId),
}

impl Actor {
    pub fn npc(&self) -> Option<NpcId> {
        match self {
            Actor::Npc(id) => Some(*id),
            Actor::Character(_) => None,
        }
    }
}

impl From<NpcId> for Actor {
    fn from(value: NpcId) -> Self { Actor::Npc(value) }
}

impl From<CharacterId> for Actor {
    fn from(value: CharacterId) -> Self { Actor::Character(value) }
}

#[derive(EnumIter, Clone, Copy)]
pub enum PersonalityTrait {
    Open,
    Adventurous,
    Closed,
    Conscientious,
    Busybody,
    Unconscientious,
    Extroverted,
    Introverted,
    Agreeable,
    Sociable,
    Disagreeable,
    Neurotic,
    Seeker,
    Worried,
    SadLoner,
    Stable,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct Personality {
    openness: u8,
    conscientiousness: u8,
    extraversion: u8,
    agreeableness: u8,
    neuroticism: u8,
}

fn distributed(min: u8, max: u8, rng: &mut impl Rng) -> u8 {
    let l = max - min;
    min + rng.gen_range(0..=l / 3)
        + rng.gen_range(0..=l / 3 + l % 3 % 2)
        + rng.gen_range(0..=l / 3 + l % 3 / 2)
}

impl Personality {
    pub const HIGH_THRESHOLD: u8 = Self::MAX - Self::LOW_THRESHOLD;
    pub const LITTLE_HIGH: u8 = Self::MID + (Self::MAX - Self::MIN) / 20;
    pub const LITTLE_LOW: u8 = Self::MID - (Self::MAX - Self::MIN) / 20;
    pub const LOW_THRESHOLD: u8 = (Self::MAX - Self::MIN) / 5 * 2 + Self::MIN;
    const MAX: u8 = 255;
    pub const MID: u8 = (Self::MAX - Self::MIN) / 2;
    const MIN: u8 = 0;

    fn distributed_value(rng: &mut impl Rng) -> u8 { distributed(Self::MIN, Self::MAX, rng) }

    pub fn random(rng: &mut impl Rng) -> Self {
        Self {
            openness: Self::distributed_value(rng),
            conscientiousness: Self::distributed_value(rng),
            extraversion: Self::distributed_value(rng),
            agreeableness: Self::distributed_value(rng),
            neuroticism: Self::distributed_value(rng),
        }
    }

    pub fn random_evil(rng: &mut impl Rng) -> Self {
        Self {
            openness: Self::distributed_value(rng),
            extraversion: Self::distributed_value(rng),
            neuroticism: Self::distributed_value(rng),
            agreeableness: distributed(0, Self::LOW_THRESHOLD - 1, rng),
            conscientiousness: distributed(0, Self::LOW_THRESHOLD - 1, rng),
        }
    }

    pub fn random_good(rng: &mut impl Rng) -> Self {
        Self {
            openness: Self::distributed_value(rng),
            extraversion: Self::distributed_value(rng),
            neuroticism: Self::distributed_value(rng),
            agreeableness: Self::distributed_value(rng),
            conscientiousness: distributed(Self::LOW_THRESHOLD, Self::MAX, rng),
        }
    }

    pub fn is(&self, trait_: PersonalityTrait) -> bool {
        match trait_ {
            PersonalityTrait::Open => self.openness > Personality::HIGH_THRESHOLD,
            PersonalityTrait::Adventurous => {
                self.openness > Personality::HIGH_THRESHOLD && self.neuroticism < Personality::MID
            },
            PersonalityTrait::Closed => self.openness < Personality::LOW_THRESHOLD,
            PersonalityTrait::Conscientious => self.conscientiousness > Personality::HIGH_THRESHOLD,
            PersonalityTrait::Busybody => self.agreeableness < Personality::LOW_THRESHOLD,
            PersonalityTrait::Unconscientious => {
                self.conscientiousness < Personality::LOW_THRESHOLD
            },
            PersonalityTrait::Extroverted => self.extraversion > Personality::HIGH_THRESHOLD,
            PersonalityTrait::Introverted => self.extraversion < Personality::LOW_THRESHOLD,
            PersonalityTrait::Agreeable => self.agreeableness > Personality::HIGH_THRESHOLD,
            PersonalityTrait::Sociable => {
                self.agreeableness > Personality::HIGH_THRESHOLD
                    && self.extraversion > Personality::MID
            },
            PersonalityTrait::Disagreeable => self.agreeableness < Personality::LOW_THRESHOLD,
            PersonalityTrait::Neurotic => self.neuroticism > Personality::HIGH_THRESHOLD,
            PersonalityTrait::Seeker => {
                self.neuroticism > Personality::HIGH_THRESHOLD
                    && self.openness > Personality::LITTLE_HIGH
            },
            PersonalityTrait::Worried => {
                self.neuroticism > Personality::HIGH_THRESHOLD
                    && self.agreeableness > Personality::LITTLE_HIGH
            },
            PersonalityTrait::SadLoner => {
                self.neuroticism > Personality::HIGH_THRESHOLD
                    && self.extraversion < Personality::LITTLE_LOW
            },
            PersonalityTrait::Stable => self.neuroticism < Personality::LOW_THRESHOLD,
        }
    }

    pub fn chat_trait(&self, rng: &mut impl Rng) -> Option<PersonalityTrait> {
        PersonalityTrait::iter().filter(|t| self.is(*t)).choose(rng)
    }

    pub fn will_ambush(&self) -> bool {
        self.agreeableness < Self::LOW_THRESHOLD && self.conscientiousness < Self::LOW_THRESHOLD
    }

    pub fn get_generic_comment(&self, rng: &mut impl Rng) -> Content {
        let i18n_key = if let Some(extreme_trait) = self.chat_trait(rng) {
            match extreme_trait {
                PersonalityTrait::Open => "npc-speech-villager_open",
                PersonalityTrait::Adventurous => "npc-speech-villager_adventurous",
                PersonalityTrait::Closed => "npc-speech-villager_closed",
                PersonalityTrait::Conscientious => "npc-speech-villager_conscientious",
                PersonalityTrait::Busybody => "npc-speech-villager_busybody",
                PersonalityTrait::Unconscientious => "npc-speech-villager_unconscientious",
                PersonalityTrait::Extroverted => "npc-speech-villager_extroverted",
                PersonalityTrait::Introverted => "npc-speech-villager_introverted",
                PersonalityTrait::Agreeable => "npc-speech-villager_agreeable",
                PersonalityTrait::Sociable => "npc-speech-villager_sociable",
                PersonalityTrait::Disagreeable => "npc-speech-villager_disagreeable",
                PersonalityTrait::Neurotic => "npc-speech-villager_neurotic",
                PersonalityTrait::Seeker => "npc-speech-villager_seeker",
                PersonalityTrait::SadLoner => "npc-speech-villager_sad_loner",
                PersonalityTrait::Worried => "npc-speech-villager_worried",
                PersonalityTrait::Stable => "npc-speech-villager_stable",
            }
        } else {
            "npc-speech-villager"
        };

        Content::localized(i18n_key)
    }
}

impl Default for Personality {
    fn default() -> Self {
        Self {
            openness: Personality::MID,
            conscientiousness: Personality::MID,
            extraversion: Personality::MID,
            agreeableness: Personality::MID,
            neuroticism: Personality::MID,
        }
    }
}

/// This type is the map route through which the rtsim (real-time simulation)
/// aspect of the game communicates with the rest of the game. It is analagous
/// to `comp::Controller` in that it provides a consistent interface for
/// simulation NPCs to control their actions. Unlike `comp::Controller`, it is
/// very abstract and is intended for consumption by both the agent code and the
/// internal rtsim simulation code (depending on whether the entity is loaded
/// into the game as a physical entity or not). Agent code should attempt to act
/// upon its instructions where reasonable although deviations for various
/// reasons (obstacle avoidance, counter-attacking, etc.) are expected.
#[derive(Clone, Debug, Default)]
pub struct RtSimController {
    pub activity: Option<NpcActivity>,
    pub actions: VecDeque<NpcAction>,
    pub personality: Personality,
    pub heading_to: Option<String>,
    // TODO: Maybe this should allow for looking at a specific entity target?
    pub look_dir: Option<Dir>,
}

impl RtSimController {
    pub fn with_destination(pos: Vec3<f32>) -> Self {
        Self {
            activity: Some(NpcActivity::Goto(pos, 0.5)),
            ..Default::default()
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum NpcActivity {
    /// (travel_to, speed_factor)
    Goto(Vec3<f32>, f32),
    /// (travel_to, speed_factor, height above terrain, direction_override,
    /// flight_mode)
    GotoFlying(Vec3<f32>, f32, Option<f32>, Option<Dir>, FlightMode),
    Gather(&'static [ChunkResource]),
    // TODO: Generalise to other entities? What kinds of animals?
    HuntAnimals,
    Dance(Option<Dir>),
    Cheer(Option<Dir>),
    Sit(Option<Dir>, Option<Vec3<i32>>),
    Talk(Actor),
}

/// Represents event-like actions that rtsim NPCs can perform to interact with
/// the world
#[derive(Clone, Debug)]
pub enum NpcAction {
    /// Speak the given message, with an optional target for that speech.
    // TODO: Use some sort of structured, language-independent value that frontends can translate
    // instead
    Say(Option<Actor>, Content),
    /// Attack the given target
    Attack(Actor),
    Dialogue(Actor, Dialogue),
}

#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct DialogueId(pub u64);

// `IS_VALIDATED` denotes whether the server has validated this dialogue as
// fulfilled. For example, a dialogue could promise to give the receiver items
// from their inventory.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dialogue<const IS_VALIDATED: bool = false> {
    pub id: DialogueId,
    pub kind: DialogueKind,
}
impl<const IS_VALIDATED: bool> core::cmp::Eq for Dialogue<IS_VALIDATED> {}

impl<const IS_VALIDATED: bool> Dialogue<IS_VALIDATED> {
    pub fn message(&self) -> Option<&Content> {
        match &self.kind {
            DialogueKind::Start | DialogueKind::End | DialogueKind::Marker { .. } => None,
            DialogueKind::Statement(msg) | DialogueKind::Question { msg, .. } => Some(msg),
            DialogueKind::Response { response, .. } => Some(&response.msg),
        }
    }
}

impl Dialogue<false> {
    pub fn into_validated_unchecked(self) -> Dialogue<true> { Dialogue { ..self } }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum DialogueKind {
    Start,
    End,
    Statement(Content),
    Question {
        // Used to uniquely track each question/response
        tag: u32,
        msg: Content,
        // Response options for the target (response_id, content)
        responses: Vec<(u16, Response)>,
    },
    Response {
        // Used to uniquely track each question/response
        tag: u32,
        response: Response,
        response_id: u16,
    },
    Marker {
        wpos: Vec2<i32>,
        name: Content,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub msg: Content,
    pub given_item: Option<(Arc<ItemDef>, u32)>,
}

impl From<Content> for Response {
    fn from(msg: Content) -> Self {
        Self {
            msg,
            given_item: None,
        }
    }
}

// Represents a message passed back to rtsim from an agent's brain
#[derive(Clone, Debug)]
pub enum NpcInput {
    Report(ReportId),
    Interaction(Actor),
    Dialogue(Actor, Dialogue<true>),
}

// Note: the `serde(name = "...")` is to minimise the length of field
// identifiers for the sake of rtsim persistence
#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq, enum_map::Enum)]
pub enum ChunkResource {
    #[serde(rename = "0")]
    Grass,
    #[serde(rename = "1")]
    Flower,
    #[serde(rename = "2")]
    Fruit,
    #[serde(rename = "3")]
    Vegetable,
    #[serde(rename = "4")]
    Mushroom,
    #[serde(rename = "5")]
    Loot, // Chests, boxes, potions, etc.
    #[serde(rename = "6")]
    Plant, // Flax, cotton, wheat, corn, etc.
    #[serde(rename = "7")]
    Stone,
    #[serde(rename = "8")]
    Wood, // Twigs, logs, bamboo, etc.
    #[serde(rename = "9")]
    Gem, // Amethyst, diamond, etc.
    #[serde(rename = "a")]
    Ore, // Iron, copper, etc.
}

// Note: the `serde(name = "...")` is to minimise the length of field
// identifiers for the sake of rtsim persistence
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Role {
    #[serde(rename = "0")]
    Civilised(Option<Profession>),
    #[serde(rename = "1")]
    Wild,
    #[serde(rename = "2")]
    Monster,
    #[serde(rename = "3")]
    Vehicle,
}

// Note: the `serde(name = "...")` is to minimise the length of field
// identifiers for the sake of rtsim persistence
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum Profession {
    #[serde(rename = "0")]
    Farmer,
    #[serde(rename = "1")]
    Hunter,
    #[serde(rename = "2")]
    Merchant,
    #[serde(rename = "3")]
    Guard,
    #[serde(rename = "4")]
    Adventurer(u32),
    #[serde(rename = "5")]
    Blacksmith,
    #[serde(rename = "6")]
    Chef,
    #[serde(rename = "7")]
    Alchemist,
    #[serde(rename = "8")]
    /// True if leader
    Pirate(bool),
    #[serde(rename = "9")]
    Cultist,
    #[serde(rename = "10")]
    Herbalist,
    #[serde(rename = "11")]
    Captain,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorldSettings {
    pub start_time: f64,
}

impl Default for WorldSettings {
    fn default() -> Self {
        Self {
            start_time: 9.0 * 3600.0, // 9am
        }
    }
}
