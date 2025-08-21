use crate::{terrain::TerrainChunkSize, vol::RectVolSize};
use serde::{Deserialize, Serialize};
use strum::EnumIter;
use vek::*;

// In chunks
pub const ZONE_SIZE: u32 = 32;

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, Serialize, Deserialize)]
    pub struct InstFlags: u8 {
        const SNOW_COVERED  = 0b00000001;
        const GLOW          = 0b00000010;
        /// Rotate half pi radians (90 degrees)
        const ROTATE_HALF_PI = 0b00000100;
        /// Rotate pi radians (180 degrees)
        const ROTATE_PI      = 0b00001000;
    }
}

#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug, Serialize, Deserialize, EnumIter)]
#[repr(u16)]
pub enum ObjectKind {
    GenericTree,
    Pine,
    Dead,
    House,
    GiantTree,
    Mangrove,
    Acacia,
    Birch,
    Redwood,
    Baobab,
    Frostpine,
    Haniwa,
    Desert,
    Palm,
    Arena,
    SavannahHut,
    SavannahAirshipDock,
    TerracottaPalace,
    TerracottaHouse,
    TerracottaYard,
    AirshipDock,
    CoastalHouse,
    CoastalWorkshop,
    CoastalAirshipDock,
    DesertCityAirshipDock,
    CliffTownAirshipDock,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Object {
    pub kind: ObjectKind,
    pub pos: Vec3<i16>,
    pub flags: InstFlags,
    pub color: Rgb<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Zone {
    pub objects: Vec<Object>,
}

pub fn to_wpos(wpos: i32) -> i32 { wpos * (TerrainChunkSize::RECT_SIZE.x * ZONE_SIZE) as i32 }

pub fn from_wpos(zone_pos: i32) -> i32 {
    zone_pos.div_euclid((TerrainChunkSize::RECT_SIZE.x * ZONE_SIZE) as i32)
}
