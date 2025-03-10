//! This example showcases a 3D first-person camera.
//!
//! The setup presented here is a very common way of organizing a first-person game
//! where the player can see their own arms. We use two industry terms to differentiate
//! the kinds of models we have:
//!
//! - The *view model* is the model that represents the player's body.
//! - The *world model* is everything else.
//!
//! ## Motivation
//!
//! The reason for this distinction is that these two models should be rendered with different field of views (FOV).
//! The view model is typically designed and animated with a very specific FOV in mind, so it is
//! generally *fixed* and cannot be changed by a player. The world model, on the other hand, should
//! be able to change its FOV to accommodate the player's preferences for the following reasons:
//! - *Accessibility*: How prone is the player to motion sickness? A wider FOV can help.
//! - *Tactical preference*: Does the player want to see more of the battlefield?
//!     Or have a more zoomed-in view for precision aiming?
//! - *Physical considerations*: How well does the in-game FOV match the player's real-world FOV?
//!     Are they sitting in front of a monitor or playing on a TV in the living room? How big is the screen?
//!
//! ## Implementation
//!
//! The `Player` is an entity holding two cameras, one for each model. The view model camera has a fixed
//! FOV of 70 degrees, while the world model camera has a variable FOV that can be changed by the player.
//!
//! We use different `RenderLayers` to select what to render.
//!
//! - The world model camera has no explicit `RenderLayers` component, so it uses the layer 0.
//!     All static objects in the scene are also on layer 0 for the same reason.
//! - The view model camera has a `RenderLayers` component with layer 1, so it only renders objects
//!     explicitly assigned to layer 1. The arm of the player is one such object.
//!     The order of the view model camera is additionally bumped to 1 to ensure it renders on top of the world model.
//! - The light source in the scene must illuminate both the view model and the world model, so it is
//!     assigned to both layers 0 and 1.
//!
//! ## Controls
//!
//! | Key Binding          | Action        |
//! |:---------------------|:--------------|
//! | mouse                | Look around   |
//! | arrow up             | Decrease FOV  |
//! | arrow down           | Increase FOV  |

use std::f32::consts::FRAC_PI_2;

use bevy::{
    core_pipeline::core_3d::Camera3d, input::mouse::AccumulatedMouseMotion, pbr::NotShadowCaster,
    prelude::*, render::view::RenderLayers,
};
use bevy_renet::renet::ClientId;
use serde::{Deserialize, Serialize};

use crate::{
    network::{ControlledPlayer, CurrentClientId, ServerLobby},
    world::WorldModelCamera,
};

#[derive(Debug, Component)]
pub struct Player {
    pub id: ClientId,
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, Component, Resource)]
pub struct PlayerInput {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub yaw: f32,
    pub pitch: f32,
}

#[derive(Debug, Serialize, Deserialize, Event)]
pub enum PlayerCommand {
    BasicAttack { cast_at: Vec3 },
}

#[derive(Debug, Component, Deref, DerefMut)]
pub struct CameraSensitivity(Vec2);

impl Default for CameraSensitivity {
    fn default() -> Self {
        Self(
            // These factors are just arbitrary mouse sensitivity values.
            // It's often nicer to have a faster horizontal sensitivity than vertical.
            // We use a component for them so that we can make them user-configurable at runtime
            // for accessibility reasons.
            // It also allows you to inspect them in an editor if you `Reflect` the component.
            Vec2::new(0.003, 0.002),
        )
    }
}

pub const PLAYER_MOVE_SPEED: f32 = 5.0;

/// Used by the view model camera and the player's arm.
/// The light source belongs to both layers.
pub const VIEW_MODEL_RENDER_LAYER: usize = 1;

#[derive(Resource)]
pub struct CursorState {
    pub locked: bool,
}

impl Default for CursorState {
    fn default() -> Self {
        Self { locked: true }
    }
}

// Add these constants for spawn positions
pub const PLAYER_SPAWN_POSITION: Vec3 = Vec3::new(0.0, 0.51, 0.0);
pub const PLAYER_SPAWN_HEIGHT: f32 = 0.51;

// Optional: Define multiple spawn points if needed
pub const SPAWN_POINTS: [Vec3; 4] = [
    Vec3::new(0.0, PLAYER_SPAWN_HEIGHT, 0.0),
    Vec3::new(5.0, PLAYER_SPAWN_HEIGHT, 0.0),
    Vec3::new(0.0, PLAYER_SPAWN_HEIGHT, 5.0),
    Vec3::new(5.0, PLAYER_SPAWN_HEIGHT, 5.0),
];

// Function to get a spawn position based on player ID
pub fn get_spawn_position(player_id: u64) -> Vec3 {
    // Use player ID to select a spawn point
    let index = (player_id % SPAWN_POINTS.len() as u64) as usize;
    SPAWN_POINTS[index]
}

pub fn spawn_view_model(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    client_id: Res<CurrentClientId>,
) {
    commands
        .spawn((
            Player { id: client_id.0 },
            CameraSensitivity::default(),
            Transform::from_xyz(0.0, 1.0, 0.0),
        ))
        .with_children(|parent| {
            // Camera with fixed FOV
            parent.spawn((
                Camera3d::default(),
                Projection::Perspective(PerspectiveProjection {
                    fov: 90.0_f32.to_radians(),
                    ..default()
                }),
            ));

            // View model (gun)
            parent.spawn((
                Mesh3d(meshes.add(Cuboid::new(0.1, 0.1, 0.5))),
                MeshMaterial3d(materials.add(Color::srgb(1.0, 0.0, 0.0))),
                Transform::from_xyz(0.5, -0.4, -0.8)
                    .with_rotation(Quat::from_rotation_x(-0.2)),
            ));
        });
}

pub fn move_player(
    player_input: Res<PlayerInput>,
    mut player: Query<&mut Transform, With<Player>>,
) {
    if let Ok(mut transform) = player.get_single_mut() {
        transform.rotation =
            Quat::from_euler(EulerRot::YXZ, player_input.yaw, player_input.pitch, 0.0);
    }
}

pub fn move_player_body(
    mut player: Single<&mut Transform, With<Player>>,
    player_input: Res<PlayerInput>,
    time: Res<Time>,
) {
    let mut transform = player.into_inner();
    let x = (player_input.right as i8 - player_input.left as i8) as f32;
    let z = (player_input.down as i8 - player_input.up as i8) as f32;

    if x != 0.0 || z != 0.0 {
        // Get forward and right vectors from the camera's rotation
        let forward = transform.forward();
        let right = transform.right();

        // Calculate movement direction relative to camera
        let movement =
            (forward * -z + right * x).normalize() * PLAYER_MOVE_SPEED * time.delta_secs();

        // Only move in the horizontal plane (prevent flying/sinking)
        transform.translation += Vec3::new(movement.x, 0.0, movement.z);
    }
}

pub fn change_fov(
    input: Res<ButtonInput<KeyCode>>,
    mut camera: Query<&mut Projection, With<WorldModelCamera>>,
) {
    if let Ok(mut projection) = camera.get_single_mut() {
        let Projection::Perspective(ref mut perspective) = projection.as_mut() else {
            return;
        };

        if input.pressed(KeyCode::ArrowUp) {
            perspective.fov -= 1.0_f32.to_radians();
            perspective.fov = perspective.fov.max(20.0_f32.to_radians());
        }
        if input.pressed(KeyCode::ArrowDown) {
            perspective.fov += 1.0_f32.to_radians();
            perspective.fov = perspective.fov.min(160.0_f32.to_radians());
        }
    }
}

pub fn player_input(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut player_input: ResMut<PlayerInput>,
    accumulated_mouse_motion: Res<AccumulatedMouseMotion>,
    camera_sensitivity: Query<&CameraSensitivity, With<Player>>,
    cursor_state: Res<CursorState>,
) {
    // Original keyboard input handling
    player_input.left = keyboard_input.pressed(KeyCode::KeyA);
    player_input.right = keyboard_input.pressed(KeyCode::KeyD);
    player_input.up = keyboard_input.pressed(KeyCode::KeyW);
    player_input.down = keyboard_input.pressed(KeyCode::KeyS);

    // Only update rotation when cursor is locked
    if cursor_state.locked {
        if let Ok(sensitivity) = camera_sensitivity.get_single() {
            let delta = accumulated_mouse_motion.delta;
            if delta != Vec2::ZERO {
                player_input.yaw -= delta.x * sensitivity.x;
                player_input.pitch = (player_input.pitch - delta.y * sensitivity.y)
                    .clamp(-FRAC_PI_2 + 0.01, FRAC_PI_2 - 0.01);
            }
        }
    }
}

pub fn grab_mouse(
    mut windows: Query<&mut Window>,
    mouse: Res<ButtonInput<MouseButton>>,
    key: Res<ButtonInput<KeyCode>>,
    mut cursor_state: ResMut<CursorState>,
) {
    let mut window = windows.single_mut();

    if cursor_state.locked {
        window.cursor_options.grab_mode = bevy::window::CursorGrabMode::Locked;
        window.cursor_options.visible = false;
    }

    if key.just_pressed(KeyCode::Escape) {
        cursor_state.locked = false;
        window.cursor_options.grab_mode = bevy::window::CursorGrabMode::None;
        window.cursor_options.visible = true;
    }

    if mouse.just_pressed(MouseButton::Left) && !cursor_state.locked {
        cursor_state.locked = true;
    }
}

// Add this function to create a consistent player model
pub fn spawn_player_model(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    transform: Transform,
    is_local_player: bool,
) -> Entity {
    let mut player_entity = commands.spawn((
        // Common components for all players
        Mesh3d(meshes.add(Cuboid::new(0.5, 1.8, 0.5))), // Player body
        MeshMaterial3d(materials.add(Color::srgb(0.8, 0.2, 0.2))), // Red color
        transform,
    ));

    // For local player, add first-person view components
    if is_local_player {
        player_entity
            .with_children(|parent| {
                // Camera
                parent.spawn((
                    Camera3d::default(),
                    Projection::Perspective(PerspectiveProjection {
                        fov: 90.0_f32.to_radians(),
                        ..default()
                    }),
                ));

                // First-person view model (gun/bat)
                parent.spawn((
                    Mesh3d(meshes.add(Cuboid::new(0.1, 0.1, 0.5))),
                    MeshMaterial3d(materials.add(Color::srgb(1.0, 0.0, 0.0))), // Red for local
                    Transform::from_xyz(0.5, -0.4, -0.8)
                        .with_rotation(Quat::from_rotation_x(-0.2)),
                ));
            })
            .insert(ControlledPlayer)
            .insert(CameraSensitivity::default())
            .id()
    } else {
        // For other players, add third-person view components
        player_entity
            .with_children(|parent| {
                // Third-person visible gun/bat
                parent.spawn((
                    Mesh3d(meshes.add(Cuboid::new(0.1, 0.1, 0.5))),
                    MeshMaterial3d(materials.add(Color::srgb(0.3, 0.3, 0.3))), // Gray for others
                    // Position the gun/bat to be visible to others and point forward
                    Transform::from_xyz(0.3, 0.0, 0.5)
                        .with_rotation(Quat::from_rotation_y(0.0)),
                ));
            })
            .id()
    }
}
