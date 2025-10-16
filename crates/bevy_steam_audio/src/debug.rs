use bevy_color::{Color, palettes::tailwind};
use bevy_ecs::entity_disabling::Disabled;

use crate::prelude::*;

#[derive(Default)]
pub struct SteamAudioDebugPlugin;

impl Plugin for SteamAudioDebugPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(init_gizmo);
        app.add_systems(
            PostUpdate,
            spawn_gizmos.chain().in_set(SteamAudioSystems::Gizmos),
        );
    }
}

#[derive(Default, Component, Reflect)]
pub struct SpawnSteamAudioGizmo {
    pub vertices: Vec<Vec3>,
    pub indices: Vec<[u32; 3]>,
}

fn init_gizmo(
    add: On<Add, SpawnSteamAudioGizmo>,
    mut gizmo_assets: ResMut<Assets<GizmoAsset>>,
    mut commands: Commands,
    gizmo_handles: Query<(&SpawnSteamAudioGizmo, &SteamAudioMaterial), Allow<Disabled>>,
) {
    let Ok((mesh, material)) = gizmo_handles.get(add.entity) else {
        return;
    };
    let mut gizmo = GizmoAsset::new();

    for indices in &mesh.indices {
        let a = mesh.vertices[indices[0] as usize];
        let b = mesh.vertices[indices[1] as usize];
        let c = mesh.vertices[indices[2] as usize];

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
        gizmo.linestrip([a, b, c, a], color);
    }
    let handle = gizmo_assets.add(gizmo);
    commands.entity(add.entity).try_insert(Gizmo {
        handle,
        depth_bias: -0.0001,
        ..default()
    });
}

fn spawn_gizmos() {}
