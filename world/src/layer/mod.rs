pub mod cave;
pub mod rock;
pub mod scatter;
pub mod shrub;
pub mod spot;
pub mod tree;
pub mod wildlife;

pub use self::{
    cave::apply_caves_to, rock::apply_rocks_to, scatter::apply_scatter_to, shrub::apply_shrubs_to,
    spot::apply_spots_to, tree::apply_trees_to,
};

use crate::{
    Canvas, CanvasInfo,
    column::ColumnSample,
    config::CONFIG,
    sim,
    util::{FastNoise, RandomPerm, Sampler},
};
use common::terrain::{Block, BlockKind, SpriteKind};
use hashbrown::HashMap;
use noise::NoiseFn;
use rand::{prelude::*, seq::IndexedRandom};
use serde::Deserialize;
use std::{
    f32,
    ops::{Add, Mul, Range, Sub},
};
use vek::*;

#[derive(Deserialize)]
pub struct Colors {
    pub bridge: (u8, u8, u8),
}

const EMPTY_AIR: Block = Block::empty();

pub struct PathLocals {
    pub riverless_alt: f32,
    pub alt: f32,
    pub water_dist: f32,
    pub bridge_offset: f32,
    pub depth: i32,
}

impl PathLocals {
    pub fn new(info: &CanvasInfo, col: &ColumnSample, path_nearest: Vec2<f32>) -> PathLocals {
        // Try to use the column at the centre of the path for sampling to make them
        // flatter
        let col_pos = -info.wpos().map(|e| e as f32) + path_nearest;
        let col00 = info.col(info.wpos() + col_pos.map(|e| e.floor() as i32) + Vec2::new(0, 0));
        let col10 = info.col(info.wpos() + col_pos.map(|e| e.floor() as i32) + Vec2::new(1, 0));
        let col01 = info.col(info.wpos() + col_pos.map(|e| e.floor() as i32) + Vec2::new(0, 1));
        let col11 = info.col(info.wpos() + col_pos.map(|e| e.floor() as i32) + Vec2::new(1, 1));
        let col_attr = |col: &ColumnSample| {
            Vec3::new(col.riverless_alt, col.alt, col.water_dist.unwrap_or(1000.0))
        };
        let [riverless_alt, alt, water_dist] = match (col00, col10, col01, col11) {
            (Some(col00), Some(col10), Some(col01), Some(col11)) => Lerp::lerp(
                Lerp::lerp(col_attr(col00), col_attr(col10), path_nearest.x.fract()),
                Lerp::lerp(col_attr(col01), col_attr(col11), path_nearest.x.fract()),
                path_nearest.y.fract(),
            ),
            _ => col_attr(col),
        }
        .into_array();
        let (bridge_offset, depth) = (
            ((water_dist.max(0.0) * 0.2).min(f32::consts::PI).cos() + 1.0) * 5.0,
            ((1.0 - ((water_dist + 2.0) * 0.3).min(0.0).cos().abs())
                * (riverless_alt + 5.0 - alt).max(0.0)
                * 1.75
                + 3.0) as i32,
        );
        PathLocals {
            riverless_alt,
            alt,
            water_dist,
            bridge_offset,
            depth,
        }
    }
}

pub fn apply_paths_to(canvas: &mut Canvas) {
    canvas.foreach_col(|canvas, wpos2d, col| {
        if let Some((path_dist, path_nearest, path, _)) =
            col.path.filter(|(dist, _, path, _)| *dist < path.width)
        {
            let inset = 0;

            let PathLocals {
                riverless_alt,
                alt: _,
                water_dist: _,
                bridge_offset: _,
                depth: _,
            } = PathLocals::new(&canvas.info(), col, path_nearest);

            let depth = 4;
            let surface_z = riverless_alt.floor() as i32;

            for z in inset - depth..inset {
                let wpos = Vec3::new(wpos2d.x, wpos2d.y, surface_z + z);
                let path_color =
                    path.surface_color(col.sub_surface_color.map(|e| (e * 255.0) as u8), wpos);
                canvas.set(wpos, Block::new(BlockKind::Earth, path_color));
            }
            let head_space = path.head_space(path_dist);
            for z in inset..inset + head_space {
                let pos = Vec3::new(wpos2d.x, wpos2d.y, surface_z + z);
                if canvas.get(pos).kind() != BlockKind::Water {
                    canvas.set(pos, EMPTY_AIR);
                }
            }
        }
    });
}

pub fn apply_trains_to(
    canvas: &mut Canvas,
    sim: &sim::WorldSim,
    sim_chunk: &sim::SimChunk,
    chunk_center_wpos2d: Vec2<i32>,
) {
    let mut splines = Vec::new();
    let g = |v: Vec2<f32>| -> Vec3<f32> {
        let path_nearest = sim
            .get_nearest_path(v.as_::<i32>())
            .map(|x| x.1)
            .unwrap_or(v.as_::<f32>());
        let alt = if let Some(c) = canvas.col_or_gen(v.as_::<i32>()) {
            let pl = PathLocals::new(canvas, &c, path_nearest);
            pl.riverless_alt + pl.bridge_offset + 0.75
        } else {
            sim_chunk.alt
        };
        v.with_z(alt)
    };
    fn hermite_to_bezier(
        p0: Vec3<f32>,
        m0: Vec3<f32>,
        p3: Vec3<f32>,
        m3: Vec3<f32>,
    ) -> CubicBezier3<f32> {
        let hermite = Vec4::new(p0, p3, m0, m3);
        let hermite = hermite.map(|v| v.with_w(0.0));
        let hermite: [[f32; 4]; 4] = hermite.map(|v: Vec4<f32>| v.into_array()).into_array();
        // https://courses.engr.illinois.edu/cs418/sp2009/notes/12-MoreSplines.pdf
        let mut m = Mat4::from_row_arrays([
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
            [-3.0, 3.0, 0.0, 0.0],
            [0.0, 0.0, -3.0, 3.0],
        ]);
        m.invert();
        let bezier = m * Mat4::from_row_arrays(hermite);
        let bezier: Vec4<Vec4<f32>> =
            Vec4::<[f32; 4]>::from(bezier.into_row_arrays()).map(Vec4::from);
        let bezier = bezier.map(Vec3::from);
        CubicBezier3::from(bezier)
    }
    for sim::NearestWaysData { bezier: bez, .. } in
        sim.get_nearest_ways(chunk_center_wpos2d, &|chunk| Some(chunk.path))
    {
        if bez.length_by_discretization(16) < 0.125 {
            continue;
        }
        let a = 0.0;
        let b = 1.0;
        for bez in bez.split((a + b) / 2.0) {
            let p0 = g(bez.evaluate(a));
            let p1 = g(bez.evaluate(a + (b - a) / 3.0));
            let p2 = g(bez.evaluate(a + 2.0 * (b - a) / 3.0));
            let p3 = g(bez.evaluate(b));
            splines.push(hermite_to_bezier(p0, 3.0 * (p1 - p0), p3, 3.0 * (p3 - p2)));
        }
    }
    for spline in splines.into_iter() {
        canvas.chunk.meta_mut().add_track(spline);
    }
}

pub fn apply_coral_to(canvas: &mut Canvas) {
    let info = canvas.info();

    if !info.chunk.river.near_water() {
        return; // Don't bother with coral for a chunk nowhere near water
    }

    canvas.foreach_col(|canvas, wpos2d, col| {
        const CORAL_DEPTH: Range<f32> = 14.0..32.0;
        const CORAL_HEIGHT: f32 = 14.0;
        const CORAL_DEPTH_FADEOUT: f32 = 5.0;
        const CORAL_SCALE: f32 = 10.0;

        let water_depth = col.water_level - col.alt;

        if !CORAL_DEPTH.contains(&water_depth) {
            return; // Avoid coral entirely for this column if we're outside coral depths
        }

        for z in col.alt.floor() as i32..(col.alt + CORAL_HEIGHT) as i32 {
            let wpos = Vec3::new(wpos2d.x, wpos2d.y, z);

            let coral_factor = Lerp::lerp(
                1.0,
                0.0,
                // Fade coral out due to incorrect depth
                ((water_depth.clamped(CORAL_DEPTH.start, CORAL_DEPTH.end) - water_depth).abs()
                    / CORAL_DEPTH_FADEOUT)
                    .min(1.0),
            ) * Lerp::lerp(
                1.0,
                0.0,
                // Fade coral out due to incorrect altitude above the seabed
                ((z as f32 - col.alt) / CORAL_HEIGHT).powi(2),
            ) * FastNoise::new(info.index.seed + 7)
                .get(wpos.map(|e| e as f64) / 32.0)
                .sub(0.2)
                .mul(100.0)
                .clamped(0.0, 1.0);

            let nz = Vec3::iota().map(|e: u32| FastNoise::new(info.index.seed + e * 177));

            let wpos_warped = wpos.map(|e| e as f32)
                + nz.map(|nz| {
                    nz.get(wpos.map(|e| e as f64) / CORAL_SCALE as f64) * CORAL_SCALE * 0.3
                });

            // let is_coral = FastNoise2d::new(info.index.seed + 17)
            //     .get(wpos_warped.xy().map(|e| e as f64) / CORAL_SCALE)
            //     .sub(1.0 - coral_factor)
            //     .max(0.0)
            //     .div(coral_factor) > 0.5;

            let is_coral = [
                FastNoise::new(info.index.seed),
                FastNoise::new(info.index.seed + 177),
            ]
            .iter()
            .all(|nz| {
                nz.get(wpos_warped.map(|e| e as f64) / CORAL_SCALE as f64)
                    .abs()
                    < coral_factor * 0.3
            });

            if is_coral {
                canvas.set(wpos, Block::new(BlockKind::Rock, Rgb::new(170, 220, 210)));
            }
        }
    });
}

pub fn apply_caverns_to<R: Rng>(canvas: &mut Canvas, dynamic_rng: &mut R) {
    let info = canvas.info();

    let canvern_nz_at = |wpos2d: Vec2<i32>| {
        // Horizontal average scale of caverns
        let scale = 2048.0;
        // How common should they be? (0.0 - 1.0)
        let common = 0.15;

        let cavern_nz = info
            .index()
            .noise
            .cave_nz
            .get((wpos2d.map(|e| e as f64) / scale).into_array()) as f32;
        ((cavern_nz * 0.5 + 0.5 - (1.0 - common)).max(0.0) / common).powf(common * 2.0)
    };

    // Get cavern attributes at a position
    let cavern_at = |wpos2d| {
        let alt = info.land().get_alt_approx(wpos2d);

        // Range of heights for the caverns
        let height_range = 16.0..250.0;
        // Minimum distance below the surface
        let surface_clearance = 64.0;

        let cavern_avg_height = Lerp::lerp(
            height_range.start,
            height_range.end,
            info.index()
                .noise
                .cave_nz
                .get((wpos2d.map(|e| e as f64) / 300.0).into_array()) as f32
                * 0.5
                + 0.5,
        );

        let cavern_avg_alt =
            CONFIG.sea_level.min(alt * 0.25) - height_range.end - surface_clearance;

        let cavern = canvern_nz_at(wpos2d);
        let cavern_height = cavern * cavern_avg_height;

        // Stalagtites
        let stalactite = info
            .index()
            .noise
            .cave_nz
            .get(wpos2d.map(|e| e as f64 * 0.015).into_array())
            .sub(0.5)
            .max(0.0)
            .mul((cavern_height as f64 - 5.0).mul(0.15).clamped(0.0, 1.0))
            .mul(32.0 + cavern_avg_height as f64);

        let hill = info
            .index()
            .noise
            .cave_nz
            .get((wpos2d.map(|e| e as f64) / 96.0).into_array()) as f32
            * cavern
            * 24.0;
        let rugged = 0.4; // How bumpy should the floor be relative to the ceiling?
        let cavern_bottom = (cavern_avg_alt - cavern_height * rugged + hill) as i32;
        let cavern_avg_bottom =
            (cavern_avg_alt - ((height_range.start + height_range.end) * 0.5) * rugged) as i32;
        let cavern_top = (cavern_avg_alt + cavern_height) as i32;
        let cavern_avg_top = (cavern_avg_alt + cavern_avg_height) as i32;

        // Stalagmites rise up to meet stalactites
        let stalagmite = stalactite;

        let floor = stalagmite as i32;

        (
            cavern_bottom,
            cavern_top,
            cavern_avg_bottom,
            cavern_avg_top,
            floor,
            stalactite,
            cavern_avg_bottom + 16, // Water level
        )
    };

    let mut mushroom_cache = HashMap::new();

    struct Mushroom {
        pos: Vec3<i32>,
        stalk: f32,
        head_color: Rgb<u8>,
    }

    // Get mushroom block, if any, at a position
    let mut get_mushroom = |wpos: Vec3<i32>, dynamic_rng: &mut R| {
        for (wpos2d, seed) in info.chunks().gen_ctx.structure_gen.get(wpos.xy()) {
            let mushroom = if let Some(mushroom) =
                mushroom_cache.entry(wpos2d).or_insert_with(|| {
                    let mut rng = RandomPerm::new(seed);
                    let (cavern_bottom, cavern_top, _, _, floor, _, water_level) =
                        cavern_at(wpos2d);
                    let pos = wpos2d.with_z(cavern_bottom + floor);
                    if rng.random_bool(0.15)
                        && cavern_top - cavern_bottom > 32
                        && pos.z > water_level - 2
                    {
                        Some(Mushroom {
                            pos,
                            stalk: 12.0 + rng.random::<f32>().powf(2.0) * 35.0,
                            head_color: Rgb::new(
                                50,
                                rng.random_range(70..110),
                                rng.random_range(100..200),
                            ),
                        })
                    } else {
                        None
                    }
                }) {
                mushroom
            } else {
                continue;
            };

            let wposf = wpos.map(|e| e as f64);
            let warp_freq = 1.0 / 32.0;
            let warp_amp = Vec3::new(12.0, 12.0, 12.0);
            let wposf_warped = wposf.map(|e| e as f32)
                + Vec3::new(
                    FastNoise::new(seed).get(wposf * warp_freq),
                    FastNoise::new(seed + 1).get(wposf * warp_freq),
                    FastNoise::new(seed + 2).get(wposf * warp_freq),
                ) * warp_amp
                    * (wposf.z as f32 - mushroom.pos.z as f32)
                        .mul(0.1)
                        .clamped(0.0, 1.0);

            let rpos = wposf_warped - mushroom.pos.map(|e| e as f32);

            let stalk_radius = 2.5f32;
            let head_radius = 18.0f32;
            let head_height = 16.0;

            let dist_sq = rpos.xy().magnitude_squared();
            if dist_sq < head_radius.powi(2) {
                let dist = dist_sq.sqrt();
                let head_dist = ((rpos - Vec3::unit_z() * mushroom.stalk)
                    / Vec2::broadcast(head_radius).with_z(head_height))
                .magnitude();

                let stalk = mushroom.stalk + Lerp::lerp(head_height * 0.5, 0.0, dist / head_radius);

                // Head
                if rpos.z > stalk
                    && rpos.z <= mushroom.stalk + head_height
                    && dist
                        < head_radius * (1.0 - (rpos.z - mushroom.stalk) / head_height).powf(0.125)
                {
                    if head_dist < 0.85 {
                        let radial = (rpos.x.atan2(rpos.y) * 10.0).sin() * 0.5 + 0.5;
                        return Some(Block::new(
                            BlockKind::GlowingMushroom,
                            Rgb::new(30, 50 + (radial * 100.0) as u8, 100 - (radial * 50.0) as u8),
                        ));
                    } else if head_dist < 1.0 {
                        return Some(Block::new(BlockKind::Wood, mushroom.head_color));
                    }
                }

                if rpos.z <= mushroom.stalk + head_height - 1.0
                    && dist_sq
                        < (stalk_radius * Lerp::lerp(1.5, 0.75, rpos.z / mushroom.stalk)).powi(2)
                {
                    // Stalk
                    return Some(Block::new(BlockKind::Wood, Rgb::new(25, 60, 90)));
                } else if ((mushroom.stalk - 0.1)..(mushroom.stalk + 0.9)).contains(&rpos.z) // Hanging orbs
                    && dist > head_radius * 0.85
                    && dynamic_rng.random_bool(0.1)
                {
                    use SpriteKind::*;
                    let sprites = if dynamic_rng.random_bool(0.1) {
                        &[Beehive, Lantern] as &[_]
                    } else {
                        &[Orb, MycelBlue, MycelBlue] as &[_]
                    };
                    return Some(Block::air(*sprites.choose(dynamic_rng).unwrap()));
                }
            }
        }

        None
    };

    canvas.foreach_col(|canvas, wpos2d, _col| {
        if canvern_nz_at(wpos2d) <= 0.0 {
            return;
        }

        let (
            cavern_bottom,
            cavern_top,
            cavern_avg_bottom,
            cavern_avg_top,
            floor,
            stalactite,
            water_level,
        ) = cavern_at(wpos2d);

        let mini_stalactite = info
            .index()
            .noise
            .cave_nz
            .get(wpos2d.map(|e| e as f64 * 0.08).into_array())
            .sub(0.5)
            .max(0.0)
            .mul(
                ((cavern_top - cavern_bottom) as f64 - 5.0)
                    .mul(0.15)
                    .clamped(0.0, 1.0),
            )
            .mul(24.0 + (cavern_avg_top - cavern_avg_bottom) as f64 * 0.2);
        let stalactite_height = (stalactite + mini_stalactite) as i32;

        let moss_common = 1.5;
        let moss = info
            .index()
            .noise
            .cave_nz
            .get(wpos2d.map(|e| e as f64 * 0.035).into_array())
            .sub(1.0 - moss_common)
            .max(0.0)
            .mul(1.0 / moss_common)
            .powf(8.0 * moss_common)
            .mul(
                ((cavern_top - cavern_bottom) as f64)
                    .mul(0.15)
                    .clamped(0.0, 1.0),
            )
            .mul(16.0 + (cavern_avg_top - cavern_avg_bottom) as f64 * 0.35);

        let plant_factor = info
            .index()
            .noise
            .cave_nz
            .get(wpos2d.map(|e| e as f64 * 0.015).into_array())
            .add(1.0)
            .mul(0.5)
            .powf(2.0);

        let is_vine = |wpos: Vec3<f32>, dynamic_rng: &mut R| {
            let wpos = wpos + wpos.xy().yx().with_z(0.0) * 0.2; // A little twist
            let dims = Vec2::new(7.0, 256.0); // Long and thin
            let vine_posf = (wpos + Vec2::new(0.0, (wpos.x / dims.x).floor() * 733.0)) / dims; // ~Random offset
            let vine_pos = vine_posf.map(|e| e.floor() as i32);
            let mut rng = RandomPerm::new(((vine_pos.x << 16) | vine_pos.y) as u32); // Rng for vine attributes
            if rng.random_bool(0.2) {
                let vine_height = (cavern_avg_top - cavern_avg_bottom).max(64) as f32;
                let vine_base = cavern_avg_bottom as f32 + rng.random_range(48.0..vine_height);
                let vine_y = (vine_posf.y.fract() - 0.5).abs() * 2.0 * dims.y;
                let vine_reach = (vine_y * 0.05).powf(2.0).min(1024.0);
                let vine_z = vine_base + vine_reach;
                if Vec2::new(vine_posf.x.fract() * 2.0 - 1.0, (wpos.z - vine_z) / 5.0)
                    .magnitude_squared()
                    < 1.0f32
                {
                    let kind = if dynamic_rng.random_bool(0.025) {
                        BlockKind::GlowingRock
                    } else {
                        BlockKind::Leaves
                    };
                    Some(Block::new(
                        kind,
                        Rgb::new(
                            85,
                            (vine_y + vine_reach).mul(0.05).sin().mul(35.0).add(85.0) as u8,
                            20,
                        ),
                    ))
                } else {
                    None
                }
            } else {
                None
            }
        };

        let mut last_kind = BlockKind::Rock;
        for z in cavern_bottom - 1..cavern_top {
            use SpriteKind::*;

            let wpos = wpos2d.with_z(z);
            let wposf = wpos.map(|e| e as f32);

            let block = if z < cavern_bottom {
                if z > water_level + dynamic_rng.random_range(4..16) {
                    Block::new(BlockKind::Grass, Rgb::new(10, 75, 90))
                } else {
                    Block::new(BlockKind::Rock, Rgb::new(50, 40, 10))
                }
            } else if z < cavern_bottom + floor {
                Block::new(BlockKind::WeakRock, Rgb::new(110, 120, 150))
            } else if z > cavern_top - stalactite_height {
                if dynamic_rng.random_bool(0.0035) {
                    // Glowing rock in stalactites
                    Block::new(BlockKind::GlowingRock, Rgb::new(30, 150, 120))
                } else {
                    Block::new(BlockKind::WeakRock, Rgb::new(110, 120, 150))
                }
            } else if let Some(mushroom_block) = get_mushroom(wpos, dynamic_rng) {
                mushroom_block
            } else if z > cavern_top - moss as i32 {
                let kind = if dynamic_rng
                    .random_bool(0.05 / (1.0 + ((cavern_top - z).max(0) as f64).mul(0.1)))
                {
                    BlockKind::GlowingMushroom
                } else {
                    BlockKind::Leaves
                };
                Block::new(kind, Rgb::new(50, 120, 160))
            } else if z < water_level {
                Block::water(Empty).with_sprite(
                    if z == cavern_bottom + floor && dynamic_rng.random_bool(0.01) {
                        *[Seagrass, SeaGrapes, SeaweedTemperate, StonyCoral]
                            .choose(dynamic_rng)
                            .unwrap()
                    } else {
                        Empty
                    },
                )
            } else if z == water_level
                && dynamic_rng.random_bool(Lerp::lerp(0.0, 0.05, plant_factor))
                && last_kind == BlockKind::Water
            {
                Block::air(CavernLillypadBlue)
            } else if z == cavern_bottom + floor
                && dynamic_rng.random_bool(Lerp::lerp(0.0, 0.5, plant_factor))
                && last_kind == BlockKind::Grass
            {
                Block::air(
                    *if dynamic_rng.random_bool(0.9) {
                        // High density
                        &[GrassBlueShort, GrassBlueMedium, GrassBlueLong] as &[_]
                    } else if dynamic_rng.random_bool(0.5) {
                        // Medium density
                        &[CaveMushroom] as &[_]
                    } else {
                        // Low density
                        &[LeafyPlant, Fern, Pyrebloom, Moonbell, Welwitch, GrassBlue] as &[_]
                    }
                    .choose(dynamic_rng)
                    .unwrap(),
                )
            } else if z == cavern_top - 1 && dynamic_rng.random_bool(0.001) {
                Block::air(
                    *[CrystalHigh, CeilingMushroom, Orb, MycelBlue]
                        .choose(dynamic_rng)
                        .unwrap(),
                )
            } else if let Some(vine) = is_vine(wposf, dynamic_rng)
                .or_else(|| is_vine(wposf.xy().yx().with_z(wposf.z), dynamic_rng))
            {
                vine
            } else {
                Block::empty()
            };

            last_kind = block.kind();

            let block = if block.is_filled() {
                Block::new(
                    block.kind(),
                    block.get_color().unwrap_or_default().map(|e| {
                        (e as f32 * dynamic_rng.random_range(0.95..1.05)).clamped(0.0, 255.0) as u8
                    }),
                )
            } else {
                block
            };

            canvas.set(wpos, block);
        }
    });
}
