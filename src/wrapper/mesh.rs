use bevy_mesh::PrimitiveTopology;
use itertools::Itertools as _;

use crate::prelude::*;

pub(super) fn plugin(app: &mut App) {
    let _ = app;
}

pub(crate) trait ToSteamAudioMesh {
    fn to_steam_audio_mesh(
        &self,
        scene: &audionimbus::Scene,
        material: audionimbus::Material,
    ) -> Result<audionimbus::StaticMesh>;
}

impl ToSteamAudioMesh for Mesh {
    fn to_steam_audio_mesh(
        &self,
        scene: &audionimbus::Scene,
        material: audionimbus::Material,
    ) -> Result<audionimbus::StaticMesh> {
        if self.primitive_topology() != PrimitiveTopology::TriangleList {
            return Err("Mesh is not a triangle list".into());
        }
        let vertices = self
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .ok_or("Mesh has no position attribute")?
            .as_float3()
            .ok_or("Mesh position attribute is not a float3")?
            .iter()
            .map(|v| audionimbus::Vector3::from(*v))
            .collect::<Vec<_>>();
        let triangles = self
            .indices()
            .ok_or("Mesh has no indices")?
            .iter()
            .chunks(3)
            .into_iter()
            .map(|mut chunk| {
                let v0 = chunk.next().unwrap();
                let v1 = chunk.next().unwrap();
                let v2 = chunk.next().unwrap();
                audionimbus::Triangle::new(v0 as i32, v1 as i32, v2 as i32)
            })
            .collect::<Vec<_>>();
        let settings = audionimbus::StaticMeshSettings {
            vertices: &vertices,
            triangles: &triangles,
            material_indices: &vec![0; triangles.len()],
            materials: &[material],
        };
        audionimbus::StaticMesh::try_new(scene, &settings).map_err(|e| e.into())
    }
}
