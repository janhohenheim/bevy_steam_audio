use avian3d::prelude::RigidBody;
use bevy_app::prelude::*;
use bevy_asset::prelude::*;
use bevy_ecs::{entity_disabling::Disabled, prelude::*};
use bevy_mesh::prelude::*;
use bevy_pbr::prelude::*;
use bevy_scene::{SceneInstanceReady, prelude::*};
use bevy_steam_audio::{
    STEAM_AUDIO_CONTEXT, audionimbus,
    scene::{Static, SteamAudioInstancedMesh, SteamAudioRootScene, SteamAudioStaticMesh},
    wrapper::{SteamAudioMaterial, ToSteamAudioMesh as _, ToSteamAudioTransform as _},
};
use bevy_transform::prelude::*;
use bevy_trenchbroom::{
    geometry::Brushes, physics::SceneCollidersReady, prelude::GenericMaterial3d,
};
use wildmatch::WildMatch;

pub mod prelude {
    pub use crate::{TrenchBroomSteamAudioScenePlugin, TrenchBroomSteamAudioSettings};
}

pub struct TrenchBroomSteamAudioScenePlugin;

impl Plugin for TrenchBroomSteamAudioScenePlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(register_scene_ready_observer);
        app.init_resource::<TrenchBroomSteamAudioSettings>();
    }
}

#[derive(Resource, Debug, PartialEq)]
pub struct TrenchBroomSteamAudioSettings {
    pub material_mapping: Vec<(WildMatch, SteamAudioMaterial)>,
}

impl Default for TrenchBroomSteamAudioSettings {
    fn default() -> Self {
        Self::empty()
            .map_material("*brick*", SteamAudioMaterial::BRICK)
            .map_material("*concrete*", SteamAudioMaterial::CONCRETE)
            .map_material("*ceramic*", SteamAudioMaterial::CERAMIC)
            .map_material("*gravel*", SteamAudioMaterial::GRAVEL)
            .map_material("*carpet*", SteamAudioMaterial::CARPET)
            .map_material("*glass*", SteamAudioMaterial::GLASS)
            .map_material("*plaster*", SteamAudioMaterial::PLASTER)
            .map_material("*wood*", SteamAudioMaterial::WOOD)
            .map_material("*metal*", SteamAudioMaterial::METAL)
            .map_material("*rock*", SteamAudioMaterial::ROCK)
    }
}

impl TrenchBroomSteamAudioSettings {
    pub fn empty() -> Self {
        Self {
            material_mapping: Vec::new(),
        }
    }

    pub fn map_material(mut self, pattern: impl AsRef<str>, material: SteamAudioMaterial) -> Self {
        self.material_mapping
            .push((WildMatch::new_case_insensitive(pattern.as_ref()), material));
        self
    }
}

fn register_scene_ready_observer(
    add: On<Add, SceneRoot>,
    scene_root: Query<&SceneRoot, Allow<Disabled>>,
    mut commands: Commands,
) -> Result {
    let scene_root = scene_root.get(add.entity)?;
    let Some(path) = scene_root.0.path() else {
        return Ok(());
    };
    let path = path.without_label();
    let Some(extension) = path.path().extension() else {
        return Ok(());
    };
    if extension == "map" || extension == "bsp" {
        commands.entity(add.entity).observe(handle_scene);
    }
    Ok(())
}

fn handle_scene(
    ready: On<SceneCollidersReady>,
    brushes: Query<(), With<Brushes>>,
    mesh_handles: Query<(&Mesh3d, &GenericMaterial3d, &GlobalTransform)>,
    meshes: Res<Assets<Mesh>>,
    names: Query<NameOrEntity>,
    mut root: ResMut<SteamAudioRootScene>,
    settings: Res<TrenchBroomSteamAudioSettings>,
    rigid_body: Query<(Option<&RigidBody>, Has<Static>, &Children)>,
    mut commands: Commands,
) -> Result {
    let mut errors = Vec::new();
    // All brushes are children of the scene root
    for entity in ready.collider_entities.iter().copied() {
        let scene_entity_name = names.get(entity).unwrap();
        if !brushes.contains(entity) {
            continue;
        }
        // The brushes hold the total geometry, but what's more interesting to us is the individual material meshes.
        // These are one level below the brushes.
        let Ok((body, is_static, potential_materials)) = rigid_body.get(entity) else {
            continue;
        };
        let is_static = body
            .map(|body| body.is_static() || is_static)
            .unwrap_or(is_static);

        for entity in potential_materials {
            let mat_entity_name = names.get(*entity).unwrap();
            let Ok((mesh, material, transform)) = mesh_handles.get(*entity) else {
                continue;
            };
            let Some(material_name) = material.0.path() else {
                // This shouldn't happen: TrenchBroom loads materials from disk
                errors.push(format!(
                    "{scene_entity_name}/{mat_entity_name}: Failed to get TrenchBroom material path"
                ));
                continue;
            };
            let material_name = material_name.path().to_string_lossy();
            let Some(mesh) = meshes.get(mesh) else {
                // This shouldn't happen: TrenchBroom directly creates and inserts its meshes
                errors.push(format!(
                    "{scene_entity_name}/{mat_entity_name}: Failed to get TrenchBroom mesh"
                ));
                continue;
            };

            let material = settings
                .material_mapping
                .iter()
                .find(|(pattern, _)| pattern.matches(material_name.as_ref()))
                .map(|(_, material)| *material)
                .unwrap_or_default();
            if is_static {
                let mesh = mesh.clone().transformed_by(transform.compute_transform());
                let audio_mesh = match mesh.to_steam_audio_mesh(&root, material.into()) {
                    Ok(audio_mesh) => audio_mesh,
                    Err(err) => {
                        errors.push(format!(
                            "{scene_entity_name}/{mat_entity_name}: Failed to convert mesh to Steam Audio mesh: {err}"
                        ));
                        continue;
                    }
                };
                root.add_static_mesh(audio_mesh.clone());
                commands
                    .entity(*entity)
                    .insert(SteamAudioStaticMesh(audio_mesh));
            } else {
                let mut sub_scene = match audionimbus::Scene::try_new(
                    &STEAM_AUDIO_CONTEXT,
                    &audionimbus::SceneSettings::Default,
                ) {
                    Ok(sub_scene) => sub_scene,
                    Err(err) => {
                        errors.push(format!(
                            "{scene_entity_name}/{mat_entity_name}: Failed to create sub-scene: {err}"
                        ));
                        continue;
                    }
                };
                let audio_mesh = match mesh.to_steam_audio_mesh(&sub_scene, material.into()) {
                    Ok(audio_mesh) => audio_mesh,
                    Err(err) => {
                        errors.push(format!(
                            "{scene_entity_name}/{mat_entity_name}: Failed to convert mesh to Steam Audio mesh: {err}"
                        ));
                        continue;
                    }
                };
                sub_scene.add_static_mesh(audio_mesh.clone());
                sub_scene.commit();

                let transform = transform.to_steam_audio_transform();

                let instanced_mesh_settings = audionimbus::InstancedMeshSettings {
                    sub_scene: sub_scene.clone(),
                    transform,
                };
                let instanced_mesh = match audionimbus::InstancedMesh::try_new(
                    &root,
                    instanced_mesh_settings,
                ) {
                    Ok(instanced_mesh) => instanced_mesh,
                    Err(err) => {
                        errors.push(format!("{scene_entity_name}/{mat_entity_name}: Failed to create instanced mesh: {err}"));
                        continue;
                    }
                };
                root.add_instanced_mesh(instanced_mesh.clone());
                commands
                    .entity(*entity)
                    .try_insert(SteamAudioInstancedMesh(instanced_mesh));
            }

            #[cfg(feature = "debug")]
            {
                use bevy_steam_audio::debug::SteamAudioGizmo;

                let gizmo = match SteamAudioGizmo::try_from(mesh) {
                    Ok(gizmo) => gizmo,
                    Err(err) => {
                        errors.push(format!(
                            "{scene_entity_name}/{mat_entity_name}: Failed to create Gizmo: {err}"
                        ));
                        continue;
                    }
                };
                commands.entity(*entity).insert(gizmo);
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(BevyError::from(errors.join("\n")))
    }
}
