use bevy_color::{Color, palettes::tailwind};
use bevy_ecs::entity_disabling::Disabled;

use crate::prelude::*;

#[derive(Default)]
pub struct SteamAudioDebugPlugin;

impl Plugin for SteamAudioDebugPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, update_gizmos.in_set(SteamAudioSystems::Gizmos));
    }
}

#[derive(Default, Component, Reflect)]
#[require(Transform, GlobalTransform)]
pub struct SteamAudioGizmo {
    pub vertices: Vec<Vec3>,
    pub indices: Vec<[u32; 3]>,
}

fn update_gizmos(
    mut gizmos: Gizmos,
    gizmo_handles: Query<
        (&GlobalTransform, &SteamAudioGizmo, &SteamAudioMaterial),
        Allow<Disabled>,
    >,
) {
    for (transform, mesh, material) in gizmo_handles.iter() {
        for indices in &mesh.indices {
            let a = transform.transform_point(mesh.vertices[indices[0] as usize]);
            let b = transform.transform_point(mesh.vertices[indices[1] as usize]);
            let c = transform.transform_point(mesh.vertices[indices[2] as usize]);

            let color = match *material {
                SteamAudioMaterial::GENERIC => tailwind::GRAY_950,
                SteamAudioMaterial::BRICK => tailwind::ORANGE_600,
                SteamAudioMaterial::CONCRETE => tailwind::ZINC_800,
                SteamAudioMaterial::CERAMIC => tailwind::SKY_800,
                SteamAudioMaterial::GRAVEL => tailwind::AMBER_900,
                SteamAudioMaterial::CARPET => tailwind::RED_500,
                SteamAudioMaterial::GLASS => tailwind::SKY_100,
                SteamAudioMaterial::PLASTER => tailwind::ROSE_100,
                SteamAudioMaterial::WOOD => tailwind::YELLOW_950,
                SteamAudioMaterial::METAL => tailwind::ZINC_500,
                SteamAudioMaterial::ROCK => tailwind::STONE_700,
                material => Color::srgba(
                    material.absorption.iter().sum::<f32>() / 3.0,
                    material.scattering,
                    material.transmission.iter().sum::<f32>() / 3.0,
                    1.0,
                )
                .into(),
            };
            gizmos.linestrip([a, b, c, a], color);
        }
    }
}
