//! Shared by the retail effects (`fx/draw.rs`) and the profile's bursts and tracers: the CPU builds camera-facing quads into
//! one dynamic mesh each frame. A `QuadBatch` keeps its buffers between frames (it is cleared, not rebuilt) and writes them
//! into the mesh's own attribute vectors in place, so a steady frame allocates nothing. It knows nothing about what the
//! quads mean: the caller encodes its vertex layout in the uv rectangle and the colour, and keeps its own simulation,
//! sorting and material. Not profile-only: it lives here with the other drawing helpers, but classic uses it too.

use bevy::mesh::{Indices, VertexAttributeValues};
use bevy::prelude::*;

/// Overwrites an attribute's vector in place, or adds the attribute the first time.
macro_rules! put {
    ($mesh:expr, $attribute:expr, $variant:ident, $values:expr) => {
        if let Some(VertexAttributeValues::$variant(old)) = $mesh.attribute_mut($attribute.id) {
            old.clear();
            old.extend_from_slice($values);
        } else {
            $mesh.insert_attribute($attribute, $values.to_vec());
        }
    };
}

/// Vertex buffers of triangles, filled by the caller each frame.
#[derive(Default)]
pub struct QuadBatch {
    pub pos: Vec<[f32; 3]>,
    /// Empty unless the batch was made `with_normals`.
    pub nor: Vec<[f32; 3]>,
    pub uv: Vec<[f32; 2]>,
    pub col: Vec<[f32; 4]>,
    pub idx: Vec<u32>,
    normals: bool,
}

impl QuadBatch {
    /// A batch whose mesh also carries a normal per vertex, for materials that shade (lit smoke) or read it.
    pub fn with_normals() -> Self {
        Self { normals: true, ..default() }
    }

    /// Forgets last frame's quads and keeps the buffers.
    pub fn clear(&mut self) {
        self.pos.clear();
        self.nor.clear();
        self.uv.clear();
        self.col.clear();
        self.idx.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.idx.is_empty()
    }

    /// One vertex; returns its index. `normal` is dropped when the batch has none.
    pub fn vertex(&mut self, pos: Vec3, normal: Vec3, uv: Vec2, color: [f32; 4]) -> u32 {
        let i = self.pos.len() as u32;
        self.pos.push(pos.to_array());
        if self.normals {
            self.nor.push(normal.to_array());
        }
        self.uv.push(uv.to_array());
        self.col.push(color);
        i
    }

    pub fn triangle(&mut self, a: u32, b: u32, c: u32) {
        self.idx.extend([a, b, c]);
    }

    /// One quad around `centre` with half extents `a` and `b`. The uv rectangle spans the quad: its `min` is at the corner
    /// `centre - a - b` and its `max` at `centre + a + b` (so the caller picks the layout: a plain 0..1 texture, an atlas
    /// cell, a kind and seed packed into the coordinates). Every vertex gets `color` and `normal`.
    pub fn quad(&mut self, centre: Vec3, a: Vec3, b: Vec3, normal: Vec3, uv: Rect, color: [f32; 4]) {
        let first = self.pos.len() as u32;
        for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            let at = Vec2::new(0.5 + 0.5 * x, 0.5 + 0.5 * y);
            self.vertex(centre + a * x + b * y, normal, uv.min + (uv.max - uv.min) * at, color);
        }
        self.triangle(first, first + 1, first + 2);
        self.triangle(first + 1, first + 3, first + 2);
    }

    /// Writes the quads into `mesh`, reusing the vectors it already holds, and tells whether there was anything to draw. With
    /// nothing to draw the mesh is left alone (the caller hides its entity).
    pub fn write(&self, mesh: &mut Mesh) -> bool {
        if self.is_empty() {
            return false;
        }
        put!(mesh, Mesh::ATTRIBUTE_POSITION, Float32x3, &self.pos);
        if self.normals {
            put!(mesh, Mesh::ATTRIBUTE_NORMAL, Float32x3, &self.nor);
        }
        put!(mesh, Mesh::ATTRIBUTE_UV_0, Float32x2, &self.uv);
        put!(mesh, Mesh::ATTRIBUTE_COLOR, Float32x4, &self.col);
        if let Some(Indices::U32(old)) = mesh.indices_mut() {
            old.clear();
            old.extend_from_slice(&self.idx);
        } else {
            mesh.insert_indices(Indices::U32(self.idx.clone()));
        }
        true
    }

    /// `write` into the mesh `handle` names; the answer is the visibility the entity should have. The mesh asset is not touched (so not
    /// re-uploaded) when there is nothing to draw.
    pub fn upload(&self, meshes: &mut Assets<Mesh>, handle: &Handle<Mesh>) -> Visibility {
        let drawn = !self.is_empty() && meshes.get_mut(handle).is_some_and(|mut mesh| self.write(&mut mesh));
        if drawn { Visibility::Inherited } else { Visibility::Hidden }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::PrimitiveTopology;

    fn mesh() -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
    }

    fn unit(batch: &mut QuadBatch, at: f32) {
        batch.quad(Vec3::X * at, Vec3::X, Vec3::Y, Vec3::Z, Rect::new(4.0, 2.0, 5.0, 3.0), [1.0, 0.5, 0.25, 0.125]);
    }

    #[test]
    fn a_quad_spans_its_uv_rectangle_corner_to_corner() {
        let mut b = QuadBatch::with_normals();
        unit(&mut b, 10.0);
        assert_eq!(b.pos, [[9.0, -1.0, 0.0], [11.0, -1.0, 0.0], [9.0, 1.0, 0.0], [11.0, 1.0, 0.0]]);
        assert_eq!(b.uv, [[4.0, 2.0], [5.0, 2.0], [4.0, 3.0], [5.0, 3.0]]);
        assert_eq!((b.nor.len(), b.col.len(), b.idx.as_slice()), (4, 4, [0, 1, 2, 1, 3, 2].as_slice()));
        unit(&mut b, 0.0);
        assert_eq!(&b.idx[6..], [4, 5, 6, 5, 7, 6], "the second quad indexes its own vertices");
        let mut plain = QuadBatch::default();
        unit(&mut plain, 0.0);
        assert!(plain.nor.is_empty(), "no normals unless asked for");
    }

    #[test]
    fn buffers_are_reused_across_writes_and_an_empty_batch_hides() {
        let mut b = QuadBatch::with_normals();
        let mut m = mesh();
        for i in 0..8 {
            unit(&mut b, i as f32);
        }
        assert!(b.write(&mut m));
        let (batch_ptr, mesh_ptr) = (b.pos.as_ptr(), m.attribute(Mesh::ATTRIBUTE_POSITION).unwrap().as_float3().unwrap().as_ptr());
        // a smaller frame goes through the same allocations on both sides
        b.clear();
        assert!(b.is_empty() && b.pos.capacity() >= 32, "clear keeps the buffers");
        unit(&mut b, 3.0);
        assert!(b.write(&mut m));
        assert_eq!(b.pos.as_ptr(), batch_ptr);
        let written = m.attribute(Mesh::ATTRIBUTE_POSITION).unwrap().as_float3().unwrap();
        assert_eq!((written.as_ptr(), written.len()), (mesh_ptr, 4), "the mesh's own vector was overwritten, not replaced");
        assert_eq!(m.indices().unwrap().len(), 6);
        assert_eq!(m.attribute(Mesh::ATTRIBUTE_NORMAL).unwrap().len(), 4);

        b.clear();
        assert!(!b.write(&mut m), "nothing to draw");
        assert_eq!(m.count_vertices(), 4, "and the mesh is left as it was");
        let mut meshes = Assets::<Mesh>::default();
        let handle = meshes.add(m);
        assert_eq!(b.upload(&mut meshes, &handle), Visibility::Hidden);
        unit(&mut b, 0.0);
        assert_eq!(b.upload(&mut meshes, &handle), Visibility::Inherited);
    }
}
