//! Not retail: the baked noise the volume shader (`shaders/volume.wesl`) samples instead of computing value noise per step. A
//! 64-cubed RGBA8 volume of periodic value noise, made once at startup (it tiles, so the shader wraps its coordinates):
//!  - r: the gas, three octaves (cell sizes 1, 1/2 and 1/4 of a base cell) at 0.58, 0.3 and 0.12;
//!  - g: the slow warp noise that makes the gas churn in place;
//!  - b: the dust lanes' noise.
//!
//! The base lattice has `CELLS` cells along each edge of the texture; the shader divides its noise coordinates by it.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// Texels along each edge.
pub const SIZE: usize = 64;
/// Cells of the base lattice along each edge (the shader's `CELLS`).
pub const CELLS: usize = 8;

fn hash(x: u32, y: u32, z: u32, seed: u32) -> f32 {
    let mut h = x.wrapping_mul(0x8da6_b343) ^ y.wrapping_mul(0xd816_3841) ^ z.wrapping_mul(0xcb1a_b31f) ^ seed.wrapping_mul(0x9e37_79b1);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^= h >> 15;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// Value noise at `p` (in cells) on a lattice that wraps every `period` cells, 0..1.
pub fn periodic(p: Vec3, period: u32, seed: u32) -> f32 {
    let i = p.floor();
    let f = p - i;
    let u = f * f * (Vec3::splat(3.0) - 2.0 * f);
    let wrap = |v: f32| (v as i64).rem_euclid(period as i64) as u32;
    let (x, y, z) = (wrap(i.x), wrap(i.y), wrap(i.z));
    let (x1, y1, z1) = ((x + 1) % period, (y + 1) % period, (z + 1) % period);
    let h = |a, b, c| hash(a, b, c, seed);
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let a = lerp(lerp(h(x, y, z), h(x1, y, z), u.x), lerp(h(x, y1, z), h(x1, y1, z), u.x), u.y);
    let b = lerp(lerp(h(x, y, z1), h(x1, y, z1), u.x), lerp(h(x, y1, z1), h(x1, y1, z1), u.x), u.y);
    lerp(a, b, u.z)
}

/// The texel data, RGBA8.
pub fn texels() -> Vec<u8> {
    let mut data = Vec::with_capacity(SIZE * SIZE * SIZE * 4);
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    for z in 0..SIZE {
        for y in 0..SIZE {
            for x in 0..SIZE {
                // the texel's centre in base cells
                let p = (Vec3::new(x as f32, y as f32, z as f32) + Vec3::splat(0.5)) * (CELLS as f32 / SIZE as f32);
                let c = CELLS as u32;
                let gas = 0.58 * periodic(p, c, 1) + 0.3 * periodic(p * 2.0, c * 2, 2) + 0.12 * periodic(p * 4.0, c * 4, 3);
                data.extend_from_slice(&[byte(gas), byte(periodic(p, c, 4)), byte(periodic(p, c, 5)), 255]);
            }
        }
    }
    data
}

pub fn image() -> Image {
    let mut image = Image::new(
        Extent3d { width: SIZE as u32, height: SIZE as u32, depth_or_array_layers: SIZE as u32 },
        TextureDimension::D3,
        texels(),
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_noise_tiles_and_stays_in_range() {
        for (p, period) in [(Vec3::new(0.3, 1.7, 5.2), 8), (Vec3::new(-2.5, 0.1, 7.9), 16)] {
            let shifted = p + Vec3::new(period as f32, -(period as f32), period as f32);
            assert!((periodic(p, period, 7) - periodic(shifted, period, 7)).abs() < 1e-5, "one period over is the same noise");
            assert!((0.0..=1.0).contains(&periodic(p, period, 7)));
        }
        assert_ne!(periodic(Vec3::splat(0.5), 8, 1), periodic(Vec3::splat(0.5), 8, 2), "the seed changes the noise");
    }

    #[test]
    fn the_baked_volume_is_a_full_rgba_cube_with_texture_in_every_channel() {
        let data = texels();
        assert_eq!(data.len(), SIZE * SIZE * SIZE * 4);
        for ch in 0..3 {
            let v: Vec<u8> = data.iter().skip(ch).step_by(4).copied().collect();
            let (lo, hi) = (*v.iter().min().unwrap(), *v.iter().max().unwrap());
            assert!(hi - lo > 100, "channel {ch} varies ({lo}..{hi})");
        }
        assert!(data.iter().skip(3).step_by(4).all(|&a| a == 255));
    }
}
