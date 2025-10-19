use bevy_color::{Color, palettes::tailwind};
use bevy_ecs::entity_disabling::Disabled;
use thiserror::Error;

use crate::prelude::*;

#[derive(Default)]
pub struct SteamAudioDebugPlugin;

impl Plugin for SteamAudioDebugPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, update_gizmos.in_set(SteamAudioSystems::Gizmos));
        app.add_observer(remove_gizmo);
        app.insert_gizmo_config(
            SteamAudioGizmos,
            GizmoConfig {
                enabled: true,
                line: GizmoLineConfig {
                    width: 50.0,
                    perspective: true,
                    ..default()
                },
                ..default()
            },
        );
    }
}

#[derive(Default, Component, Reflect)]
#[require(Transform, GlobalTransform)]
#[reflect(Component)]
pub struct SteamAudioGizmo {
    pub vertices: Vec<Vec3>,
    pub indices: Vec<[u32; 3]>,
}

#[derive(Error, Debug, Copy, Clone, PartialEq, Eq)]
pub enum SteamAudioGizmoError {
    #[error("Mesh has no positions")]
    NoPositions,
    #[error("Mesh has no indices")]
    NoIndices,
}

impl TryFrom<&Mesh> for SteamAudioGizmo {
    type Error = SteamAudioGizmoError;

    fn try_from(mesh: &Mesh) -> Result<Self, Self::Error> {
        use itertools::Itertools as _;

        let Some(vertices) = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(|p| p.as_float3())
        else {
            return Err(SteamAudioGizmoError::NoPositions);
        };

        let Some(indices) = mesh.indices() else {
            return Err(SteamAudioGizmoError::NoIndices);
        };
        let gizmo = SteamAudioGizmo {
            vertices: vertices.iter().map(|v| Vec3::from_array(*v)).collect(),
            indices: indices
                .iter()
                .chunks(3)
                .into_iter()
                .map(|mut chunk| {
                    [
                        chunk.next().unwrap() as u32,
                        chunk.next().unwrap() as u32,
                        chunk.next().unwrap() as u32,
                    ]
                })
                .collect(),
        };

        Ok(gizmo)
    }
}

fn update_gizmos(
    mut gizmos: Gizmos<SteamAudioGizmos>,
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
                SteamAudioMaterial::CARPET => tailwind::RED_700,
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

fn remove_gizmo(remove: On<Remove, SteamAudioMaterial>, mut commands: Commands) {
    commands
        .entity(remove.entity)
        .try_remove::<SteamAudioGizmo>();
}

#[derive(Reflect, Default, GizmoConfigGroup)]
pub struct SteamAudioGizmos;
