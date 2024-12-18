use std::{collections::HashMap, net::UdpSocket, time::SystemTime};

use bevy::{
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    prelude::{shape::Icosphere, *},
};
use bevy_egui::{EguiContexts, EguiPlugin};
use bevy_flycam::{FlyCam, NoCameraPlayerPlugin, MovementSettings};

use bevy_playground::{
    connection_config, get_server_addr, setup_level, ClientChannel, NetworkedEntities, PlayerCommand, PlayerInput, ServerChannel, ServerMessages, SolanaSlotBlock, PROTOCOL_ID
};
use bevy_rapier3d::prelude::{Collider, Restitution, RigidBody};
use bevy_renet::{
    renet::{
        transport::{ClientAuthentication, NetcodeClientTransport, NetcodeTransportError},
        RenetClient,
    },
    transport::NetcodeClientPlugin,
    RenetClientPlugin,
};
use renet_visualizer::{RenetClientVisualizer, RenetVisualizerStyle};
use smooth_bevy_cameras::{
    controllers::fps::FpsCameraPlugin, LookTransform, LookTransformBundle, LookTransformPlugin,
    Smoother,
};

#[derive(Component)]
struct ControlledPlayer;

#[derive(Default, Resource)]
struct NetworkMapping(HashMap<Entity, Entity>);

#[derive(Debug)]
struct PlayerInfo {
    client_entity: Entity,
    server_entity: Entity,
}

#[derive(Debug, Default, Resource)]
struct ClientLobby {
    players: HashMap<u64, PlayerInfo>,
}


fn new_renet_client() -> (RenetClient, NetcodeClientTransport) {
    let client = RenetClient::new(connection_config());
    let server_addr = get_server_addr().parse().unwrap();
    let socket = UdpSocket::bind("0.0.0.0:0").unwrap(); // Bind to any available port
    let current_time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();
    let client_id = current_time.as_millis() as u64;
    let authentication = ClientAuthentication::Unsecure {
        client_id,
        protocol_id: PROTOCOL_ID,
        server_addr,
        user_data: None,
    };

    let transport = NetcodeClientTransport::new(current_time, authentication, socket).unwrap();

    (client, transport)
}

fn main() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins);
    app.add_plugin(RenetClientPlugin);
    app.add_plugin(NetcodeClientPlugin);
    app.add_plugin(LookTransformPlugin);
    app.add_plugin(FrameTimeDiagnosticsPlugin::default());
    app.add_plugin(LogDiagnosticsPlugin::default());
    app.add_plugin(EguiPlugin);
    app.add_plugin(NoCameraPlayerPlugin);

    app.add_event::<PlayerCommand>();
    app.insert_resource(PlayerInput::default());

    app.insert_resource(ClientLobby::default());
    let (client, transport) = new_renet_client();
    app.insert_resource(client);
    app.insert_resource(transport);

    app.insert_resource(NetworkMapping::default());

    app.insert_resource(MovementSettings {
        sensitivity: 0.00015,
        speed: 12.0,
    });

    app.add_systems(
        (
            client_sync_players,
            client_send_input,
            client_send_player_commands,
        ).distributive_run_if(bevy_renet::transport::client_connected),
    );

    app.insert_resource(RenetClientVisualizer::<200>::new(
        RenetVisualizerStyle::default(),
    ));
    app.add_system(update_visulizer_system);

    app.add_startup_system(setup_level);
    app.add_startup_system(setup_camera_fps);
    app.add_system(panic_on_error_system);

    app.run();
}

// If any error is found we just panic
fn panic_on_error_system(mut renet_error: EventReader<NetcodeTransportError>) {
    for e in renet_error.iter() {
        panic!("{}", e);
    }
}

fn update_visulizer_system(
    mut egui_contexts: EguiContexts,
    mut visualizer: ResMut<RenetClientVisualizer<200>>,
    client: Res<RenetClient>,
    mut show_visualizer: Local<bool>,
    keyboard_input: Res<Input<KeyCode>>,
) {
    visualizer.add_network_info(client.network_info());
    if keyboard_input.just_pressed(KeyCode::F1) {
        *show_visualizer = !*show_visualizer;
    }
    if *show_visualizer {
        visualizer.show_window(egui_contexts.ctx_mut());
    }
}

fn client_send_input(
    mut client: ResMut<RenetClient>,
    mut player_input: ResMut<PlayerInput>,
    camera_query: Query<&Transform, With<FlyCam>>,
) {
    if let Ok(camera_transform) = camera_query.get_single() {
        player_input.position = camera_transform.translation.into();
    }

    let input_message = bincode::serialize(&*player_input).unwrap();
    client.send_message(ClientChannel::Input, input_message);
}

fn client_send_player_commands(
    mut player_commands: EventReader<PlayerCommand>,
    mut client: ResMut<RenetClient>,
) {
    for command in player_commands.iter() {
        let command_message = bincode::serialize(command).unwrap();
        client.send_message(ClientChannel::Command, command_message);
    }
}

fn client_sync_players(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut client: ResMut<RenetClient>,
    transport: Res<NetcodeClientTransport>,
    mut lobby: ResMut<ClientLobby>,
    mut network_mapping: ResMut<NetworkMapping>,
) {
    let client_id = transport.client_id();
    while let Some(message) = client.receive_message(ServerChannel::ServerMessages) {
        let server_message = bincode::deserialize(&message).unwrap();
        match server_message {
            ServerMessages::PlayerCreate {
                id,
                translation,
                entity,
            } => {
                println!("Player {} connected.", id);

                let mut client_entity = commands.spawn(PbrBundle {
                    mesh: meshes.add(Mesh::from(shape::Capsule::default())),
                    material: materials.add(Color::rgb(0.8, 0.7, 0.6).into()),
                    transform: Transform::from_xyz(translation[0], translation[1], translation[2]),
                    ..Default::default()
                });

                if client_id == id {
                    client_entity.insert(ControlledPlayer);
                }

                let player_info = PlayerInfo {
                    server_entity: entity,
                    client_entity: client_entity.id(),
                };
                lobby.players.insert(id, player_info);
                network_mapping.0.insert(entity, client_entity.id());
            }
            ServerMessages::PlayerRemove { id } => {
                println!("Player {} disconnected.", id);
                if let Some(PlayerInfo {
                    server_entity,
                    client_entity,
                }) = lobby.players.remove(&id)
                {
                    commands.entity(client_entity).despawn();
                    network_mapping.0.remove(&server_entity);
                }
            }
            ServerMessages::SpawnProjectile {
                entity,
                translation,
            } => {
                let projectile_entity = commands.spawn(PbrBundle {
                    mesh: meshes.add(
                        Mesh::try_from(Icosphere {
                            radius: 0.1,
                            subdivisions: 5,
                        })
                        .unwrap(),
                    ),
                    material: materials.add(Color::rgb(1.0, 0.0, 0.0).into()),
                    transform: Transform::from_translation(translation.into()),
                    ..Default::default()
                });
                network_mapping.0.insert(entity, projectile_entity.id());
            }
            ServerMessages::DespawnProjectile { entity } => {
                if let Some(entity) = network_mapping.0.remove(&entity) {
                    commands.entity(entity).despawn();
                }
            }
            ServerMessages::SpawnSolanaBlock {
                entity,
                transform,
                slot,
            } => {
                println!(
                    "Solana Slot {} spawned. Transform: {}, {}, {}",
                    slot, transform.0, transform.1, transform.2
                );

                // Spawn location
                let spawn_location = Transform::from_xyz(transform.0, transform.1, transform.2);

                // Spawn new
                let solana_block_entity = commands
                    .spawn(PbrBundle {
                        mesh: meshes.add(Mesh::from(shape::Box::new(1.0, 1.0, 1.0))),
                        material: materials.add(Color::rgb(0.8, 0.7, 0.6).into()),
                        transform: spawn_location,
                        ..Default::default()
                    })
                    .insert(RigidBody::Dynamic)
                    // .insert(LockedAxes::ROTATION_LOCKED | LockedAxes::TRANSLATION_LOCKED_Y)
                    .insert(Collider::cuboid(1.0, 1.0, 1.0))
                    .insert(Restitution::coefficient(0.7))
                    .insert(SolanaSlotBlock { id: slot })
                    .id();

                network_mapping.0.insert(entity, solana_block_entity);
            }
            ServerMessages::DespawnSolanaBlock { entity } => {
                if let Some(entity) = network_mapping.0.remove(&entity) {
                    commands.entity(entity).despawn();
                }
            }
        }
    }

    while let Some(message) = client.receive_message(ServerChannel::NetworkedEntities) {
        let networked_entities: NetworkedEntities = bincode::deserialize(&message).unwrap();

        for i in 0..networked_entities.entities.len() {
            if let Some(entity) = network_mapping.0.get(&networked_entities.entities[i]) {
                let translation = networked_entities.translations[i].into();
                let transform = Transform {
                    translation,
                    ..Default::default()
                };
                commands.entity(*entity).insert(transform);
            }
        }
    }
}

fn setup_camera_fps(mut commands: Commands) {
    commands.spawn((
        Camera3dBundle {
            transform: Transform::from_xyz(0.0, 2.0, 0.5),
            ..default()
        },
        FlyCam,
    ));
}
