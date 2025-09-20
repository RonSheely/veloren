use crate::{
    comp::{
        Density, Mass, Ori, ThrownItem,
        item::{
            Item, ItemKind, Utility,
            armor::ArmorKind,
            tool::{Tool, ToolKind},
        },
    },
    consts::WATER_DENSITY,
    util::Dir,
};
use common_base::enum_iter;
use rand::prelude::*;
use serde::{Deserialize, Serialize};
use std::f32::consts::PI;
use strum::IntoEnumIterator;
use vek::Vec3;

enum_iter! {
    #[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
    #[repr(u32)]
    pub enum ItemArmorKind {
        Shoulder = 0,
        Chest = 1,
        Belt = 2,
        Hand = 3,
        Pants = 4,
        Foot = 5,
        Back = 6,
        Ring = 7,
        Neck = 8,
        Head = 9,
        Tabard = 10,
        Bag = 11,
    }
}

enum_iter! {
    #[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
    #[repr(u32)]
    pub enum Body {
        Tool(ToolKind) = 0,
        ModularComponent = 1,
        Lantern = 2,
        Glider = 3,
        Armor(ItemArmorKind) = 4,
        Utility = 5,
        Consumable = 6,
        Throwable = 7,
        Ingredient = 8,
        Coins = 9,
        CoinPouch = 10,
        Empty = 11,
        Thrown(ToolKind) = 12,
    }
}

impl From<Body> for super::Body {
    fn from(body: Body) -> Self { super::Body::Item(body) }
}

impl From<&Item> for Body {
    fn from(item: &Item) -> Self {
        match &*item.kind() {
            ItemKind::Tool(Tool { kind, .. }) => Body::Tool(*kind),
            ItemKind::ModularComponent(_) => Body::ModularComponent,
            ItemKind::Lantern(_) => Body::Lantern,
            ItemKind::Glider => Body::Glider,
            ItemKind::Armor(armor) => match armor.kind {
                ArmorKind::Shoulder => Body::Armor(ItemArmorKind::Shoulder),
                ArmorKind::Chest => Body::Armor(ItemArmorKind::Chest),
                ArmorKind::Belt => Body::Armor(ItemArmorKind::Belt),
                ArmorKind::Hand => Body::Armor(ItemArmorKind::Hand),
                ArmorKind::Pants => Body::Armor(ItemArmorKind::Pants),
                ArmorKind::Foot => Body::Armor(ItemArmorKind::Foot),
                ArmorKind::Back => Body::Armor(ItemArmorKind::Back),
                ArmorKind::Backpack => Body::Armor(ItemArmorKind::Back),
                ArmorKind::Ring => Body::Armor(ItemArmorKind::Ring),
                ArmorKind::Neck => Body::Armor(ItemArmorKind::Neck),
                ArmorKind::Head => Body::Armor(ItemArmorKind::Head),
                ArmorKind::Tabard => Body::Armor(ItemArmorKind::Tabard),
                ArmorKind::Bag => Body::Armor(ItemArmorKind::Bag),
            },
            ItemKind::Utility { kind, .. } => match kind {
                Utility::Coins => {
                    if item.amount() > 100 {
                        Body::CoinPouch
                    } else {
                        Body::Coins
                    }
                },
                _ => Body::Utility,
            },
            ItemKind::Consumable { .. } => Body::Consumable,
            ItemKind::Ingredient { .. } => Body::Ingredient,
            _ => Body::Empty,
        }
    }
}

impl From<&ThrownItem> for Body {
    fn from(thrown_item: &ThrownItem) -> Self {
        match &*thrown_item.0.kind() {
            ItemKind::Tool(Tool { kind, .. }) => Body::Thrown(*kind),
            _ => Body::Empty,
        }
    }
}

impl Body {
    pub fn to_string(self) -> &'static str {
        match self {
            Body::Tool(_) => "tool",
            Body::ModularComponent => "modular_component",
            Body::Lantern => "lantern",
            Body::Glider => "glider",
            Body::Armor(_) => "armor",
            Body::Utility => "utility",
            Body::Consumable => "consumable",
            Body::Throwable => "throwable",
            Body::Ingredient => "ingredient",
            Body::Coins => "coins",
            Body::CoinPouch => "coin_pouch",
            Body::Empty => "empty",
            Body::Thrown(_) => "thrown",
        }
    }

    pub fn density(&self) -> Density { Density(1.1 * WATER_DENSITY) }

    pub fn mass(&self) -> Mass { Mass(2.0) }

    pub fn dimensions(&self) -> Vec3<f32> { Vec3::new(0.0, 0.1, 0.0) }

    pub fn orientation(&self, rng: &mut impl Rng) -> Ori {
        let random = rng.random_range(-1.0..1.0f32);
        let default = Ori::default();
        match self {
            Body::Tool(_) | Body::Thrown(_) => default
                .pitched_down(PI / 2.0)
                .yawed_left(PI / 2.0)
                .pitched_towards(
                    Dir::from_unnormalized(Vec3::new(
                        random,
                        rng.random_range(-1.0..1.0f32),
                        rng.random_range(-1.0..1.0f32),
                    ))
                    .unwrap_or_default(),
                ),

            Body::Armor(ItemArmorKind::Neck | ItemArmorKind::Back | ItemArmorKind::Tabard) => {
                default.yawed_left(random).pitched_down(PI / 2.0)
            },
            _ => default.yawed_left(random),
        }
    }
}
