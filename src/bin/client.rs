use std::collections::HashMap;

use bevy::input::mouse::MouseMotion;
use bevy::window::{CursorGrabMode, PrimaryWindow, WindowResized};
use bevy::{
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    prelude::Vec3,
    prelude::*,
};
use bevy_egui::{EguiContexts, EguiPlugin};
use bevy_renet::{
    client_connected,
    renet::{ClientId, RenetClient},
    RenetClientPlugin,
};
use multiplayer::network::{
    ClientChannel, ClientLobby, ControlledPlayer, CurrentClientId, NetworkMapping, PlayerInfo,
    ServerChannel,
};
use multiplayer::player::{
    change_fov, grab_mouse, move_player, move_player_body, player_input, spawn_player_model,
    spawn_view_model, CursorState,
};
use multiplayer::world::{spawn_lights, spawn_world_model};
use multiplayer::{
    network::{connection_config, NetworkedEntities, ServerMessages},
    player::{PlayerCommand, PlayerInput},
};

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Connected;

#[cfg(feature = "netcode")]
fn add_netcode_network(app: &mut App) {
    use bevy_renet::netcode::{
        ClientAuthentication, NetcodeClientPlugin, NetcodeClientTransport, NetcodeTransportError,
    };
    use multiplayer::PROTOCOL_ID;
    use std::{net::UdpSocket, time::SystemTime};

    app.add_plugins(NetcodeClientPlugin);

    app.configure_sets(Update, Connected.run_if(client_connected));

    let client = RenetClient::new(connection_config());

    let server_addr = "127.0.0.1:5000".parse().unwrap();
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
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

    app.insert_resource(client);
    app.insert_resource(transport);
    app.insert_resource(CurrentClientId(client_id));

    // If any error is found we just panic
    #[allow(clippy::never_loop)]
    fn panic_on_error_system(mut renet_error: EventReader<NetcodeTransportError>) {
        for e in renet_error.read() {
            panic!("{}", e);
        }
    }

    app.add_systems(Update, panic_on_error_system);
}

#[cfg(feature = "steam")]
fn add_steam_network(app: &mut App) {
    use bevy_renet::steam::{SteamClientPlugin, SteamClientTransport, SteamTransportError};
    use steamworks::{SingleClient, SteamId};

    let (steam_client, single) = steamworks::Client::init_app(480).unwrap();

    steam_client.networking_utils().init_relay_network_access();

    let args: Vec<String> = std::env::args().collect();
    let server_steam_id: u64 = args[1].parse().unwrap();
    let server_steam_id = SteamId::from_raw(server_steam_id);

    let client = RenetClient::new(connection_config());
    let transport = SteamClientTransport::new(&steam_client, &server_steam_id).unwrap();

    app.add_plugins(SteamClientPlugin);
    app.insert_resource(client);
    app.insert_resource(transport);
    app.insert_resource(CurrentClientId(steam_client.user().steam_id().raw()));

    app.configure_sets(Update, Connected.run_if(client_connected));

    app.insert_non_send_resource(single);
    fn steam_callbacks(client: NonSend<SingleClient>) {
        client.run_callbacks();
    }

    app.add_systems(PreUpdate, steam_callbacks);

    // If any error is found we just panic
    #[allow(clippy::never_loop)]
    fn panic_on_error_system(mut renet_error: EventReader<SteamTransportError>) {
        for e in renet_error.read() {
            panic!("{}", e);
        }
    }

    app.add_systems(Update, panic_on_error_system);
}

fn main() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins);
    app.add_plugins(RenetClientPlugin);
    app.add_plugins(FrameTimeDiagnosticsPlugin);
    app.add_plugins(LogDiagnosticsPlugin::default());
    app.add_plugins(EguiPlugin);

    #[cfg(feature = "netcode")]
    add_netcode_network(&mut app);

    #[cfg(feature = "steam")]
    add_steam_network(&mut app);

    app.add_event::<PlayerCommand>();

    app.insert_resource(ClientLobby::default());
    app.insert_resource(PlayerInput::default());
    app.insert_resource(NetworkMapping::default());
    app.insert_resource(CurrentClientId(0));

    app.add_systems(Startup, (spawn_view_model, spawn_world_model));

    app.insert_resource(CursorState::default());

    app.add_systems(
        Update,
        (
            player_input,
            move_player,
            move_player_body,
            grab_mouse,
            change_fov,
        ),
    );

    app.add_systems(
        Update,
        (
            client_send_input,
            client_send_player_commands,
            client_sync_players,
        )
            .in_set(Connected),
    );

    app.run();
}

fn client_send_input(player_input: Res<PlayerInput>, mut client: ResMut<RenetClient>) {
    let input_message = bincode::serialize(&*player_input).unwrap();

    client.send_message(ClientChannel::Input, input_message);
}

fn client_send_player_commands(
    mut player_commands: EventReader<PlayerCommand>,
    mut client: ResMut<RenetClient>,
) {
    for command in player_commands.read() {
        let command_message = bincode::serialize(command).unwrap();
        client.send_message(ClientChannel::Command, command_message);
    }
}

fn client_sync_players(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut client: ResMut<RenetClient>,
    client_id: Res<CurrentClientId>,
    mut lobby: ResMut<ClientLobby>,
    mut network_mapping: ResMut<NetworkMapping>,
    player_query: Query<Entity, With<ControlledPlayer>>,
) {
    let client_id = client_id.0;
    while let Some(message) = client.receive_message(ServerChannel::ServerMessages) {
        let server_message = bincode::deserialize(&message).unwrap();
        match server_message {
            ServerMessages::PlayerCreate {
                id,
                translation,
                rotation,
                entity,
            } => {
                println!(
                    "Received player create for ID: {}, our ID: {}",
                    id, client_id
                );

                // If this is our own player, just store the mapping
                if id == client_id {
                    println!("This is our own player, storing mapping");
                    if let Ok(local_player) = player_query.get_single() {
                        let player_info = PlayerInfo {
                            server_entity: entity,
                            client_entity: local_player,
                        };
                        lobby.players.insert(id, player_info);
                        network_mapping.0.insert(entity, local_player);
                        println!(
                            "Mapped our player: server entity {:?} to client entity {:?}",
                            entity, local_player
                        );
                    } else {
                        println!("WARNING: Could not find our local player entity!");
                    }
                    continue;
                }

                println!("Spawning other player {} at {:?}", id, translation);

                let transform = Transform {
                    translation: Vec3::new(translation[0], translation[1], translation[2]),
                    rotation: Quat::from_array(rotation),
                    ..default()
                };

                // Use the shared player model function
                let client_entity = spawn_player_model(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    transform,
                    id,
                    false, // not local player
                );

                let player_info = PlayerInfo {
                    server_entity: entity,
                    client_entity,
                };
                lobby.players.insert(id, player_info);
                network_mapping.0.insert(entity, client_entity);
                println!(
                    "Added player {} to lobby with client entity {:?}",
                    id, client_entity
                );
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
        }
    }

    while let Some(message) = client.receive_message(ServerChannel::NetworkedEntities) {
        let networked_entities: NetworkedEntities = bincode::deserialize(&message).unwrap();

        for i in 0..networked_entities.entities.len() {
            if let Some(entity) = network_mapping.0.get(&networked_entities.entities[i]) {
                // Skip updates for our own player
                if let Some(player_info) = lobby.players.get(&client_id) {
                    if player_info.client_entity == *entity {
                        continue;
                    }
                }

                let transform = Transform {
                    translation: networked_entities.translations[i].into(),
                    rotation: Quat::from_array(networked_entities.rotations[i]),
                    ..default()
                };
                commands.entity(*entity).insert(transform);
            }
        }
    }
}

// #[derive(Component)]
// struct Target;

// fn update_target_system(
//     primary_window: Query<&Window, With<PrimaryWindow>>,
//     mut target_query: Query<&mut Transform, With<Target>>,
//     camera_query: Query<(&Camera, &GlobalTransform)>,
// ) {
//     let (camera, camera_transform) = camera_query.single();
//     let mut target_transform = target_query.single_mut();
//     if let Some(cursor_pos) = primary_window.single().cursor_position() {
//         if let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_pos) {
//             if let Some(distance) = ray.intersect_plane(Vec3::Y, InfinitePlane3d::new(Vec3::Y)) {
//                 target_transform.translation = ray.direction * distance + ray.origin;
//             }
//         }
//     }
// }

// fn camera_follow(
//     time: Res<Time>,
//     mut camera_query: Query<&mut Transform, (With<Camera>, Without<ControlledPlayer>)>,
//     player_query: Query<&Transform, With<ControlledPlayer>>,
// ) {
//     let mut cam_transform = camera_query.single_mut();
//     if let Ok(player_transform) = player_query.get_single() {
//         let eye = Vec3::new(
//             player_transform.translation.x,
//             8.,
//             player_transform.translation.z + 2.5,
//         );
//         if eye.distance(cam_transform.translation) > 10.0 {
//             cam_transform.translation = eye;
//         } else {
//             cam_transform
//                 .translation
//                 .smooth_nudge(&eye, 8.0, time.delta_secs());
//         }
//     }
// }
