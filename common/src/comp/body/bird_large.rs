use common_base::{enum_iter, struct_iter};
use rand::{prelude::IndexedRandom, rng};
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

struct_iter! {
    #[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
    pub struct Body {
        pub species: Species,
        pub body_type: BodyType,
    }
}

impl Body {
    pub fn random() -> Self {
        let mut rng = rng();
        let species = *ALL_SPECIES.choose(&mut rng).unwrap();
        Self::random_with(&mut rng, &species)
    }

    #[inline]
    pub fn random_with(rng: &mut impl rand::Rng, &species: &Species) -> Self {
        let body_type = *ALL_BODY_TYPES.choose(rng).unwrap();
        Self { species, body_type }
    }
}

impl From<Body> for super::Body {
    fn from(body: Body) -> Self { super::Body::BirdLarge(body) }
}

enum_iter! {
    ~const_array(ALL)
    #[derive(
        Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
    #[repr(u32)]
    pub enum Species {
        Phoenix = 0,
        Cockatrice = 1,
        Roc = 2,
        FlameWyvern = 3,
        CloudWyvern = 4,
        FrostWyvern = 5,
        SeaWyvern = 6,
        WealdWyvern = 7,
    }
}

/// Data representing per-species generic data.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AllSpecies<SpeciesMeta> {
    pub phoenix: SpeciesMeta,
    pub cockatrice: SpeciesMeta,
    pub roc: SpeciesMeta,
    pub wyvern_flame: SpeciesMeta,
    pub wyvern_cloud: SpeciesMeta,
    pub wyvern_frost: SpeciesMeta,
    pub wyvern_sea: SpeciesMeta,
    pub wyvern_weald: SpeciesMeta,
}

impl<'a, SpeciesMeta> core::ops::Index<&'a Species> for AllSpecies<SpeciesMeta> {
    type Output = SpeciesMeta;

    #[inline]
    fn index(&self, &index: &'a Species) -> &Self::Output {
        match index {
            Species::Phoenix => &self.phoenix,
            Species::Cockatrice => &self.cockatrice,
            Species::Roc => &self.roc,
            Species::FlameWyvern => &self.wyvern_flame,
            Species::CloudWyvern => &self.wyvern_cloud,
            Species::FrostWyvern => &self.wyvern_frost,
            Species::SeaWyvern => &self.wyvern_sea,
            Species::WealdWyvern => &self.wyvern_weald,
        }
    }
}

pub const ALL_SPECIES: [Species; Species::NUM_KINDS] = Species::ALL;

impl<'a, SpeciesMeta: 'a> IntoIterator for &'a AllSpecies<SpeciesMeta> {
    type IntoIter = std::iter::Copied<std::slice::Iter<'static, Self::Item>>;
    type Item = Species;

    fn into_iter(self) -> Self::IntoIter { ALL_SPECIES.iter().copied() }
}

enum_iter! {
    ~const_array(ALL)
    #[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, EnumString, Display)]
    #[repr(u32)]
    pub enum BodyType {
        Female = 0,
        Male = 1,
    }
}
pub const ALL_BODY_TYPES: [BodyType; BodyType::NUM_KINDS] = BodyType::ALL;
