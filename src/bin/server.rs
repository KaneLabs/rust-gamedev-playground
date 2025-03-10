use std::{collections::HashMap, f32::consts::PI};

use bevy::{
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    prelude::*,
};
use bevy_egui::{EguiContexts, EguiPlugin};
use bevy_renet::{
    renet::{ClientId, RenetServer, ServerEvent},
    RenetServerPlugin,
};
use multiplayer::{
    network::{ClientChannel, NetworkedEntities, ServerChannel, ServerLobby, ServerMessages},
    player::{Player, PlayerCommand, PlayerInput, PLAYER_MOVE_SPEED, PLAYER_SPAWN_POSITION},
    world::spawn_world_model,
    Velocity,
};
use renet_visualizer::RenetServerVisualizer;

#[cfg(feature = "netcode")]
fn add_netcode_network(app: &mut App) {
    use bevy_renet::netcode::{
        NetcodeServerPlugin, NetcodeServerTransport, ServerAuthentication, ServerConfig,
    };
    use multiplayer::{network::connection_config, PROTOCOL_ID};
    use std::{net::UdpSocket, time::SystemTime};

    app.add_plugins(NetcodeServerPlugin);

    let server = RenetServer::new(connection_config());

    let public_addr = "127.0.0.1:5000".parse().unwrap();
    let socket = UdpSocket::bind(public_addr).unwrap();
    let current_time: std::time::Duration = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();
    let server_config = ServerConfig {
        current_time,
        max_clients: 64,
        protocol_id: PROTOCOL_ID,
        public_addresses: vec![public_addr],
        authentication: ServerAuthentication::Unsecure,
    };

    let transport = NetcodeServerTransport::new(server_config, socket).unwrap();
    app.insert_resource(server);
    app.insert_resource(transport);
}

#[cfg(feature = "steam")]
fn add_steam_network(app: &mut App) {
    use bevy_renet::steam::{
        AccessPermission, SteamServerConfig, SteamServerPlugin, SteamServerTransport,
    };
    use multiplayer::connection_config;
    use steamworks::SingleClient;

    let (steam_client, single) = steamworks::Client::init_app(480).unwrap();

    let server: RenetServer = RenetServer::new(connection_config());

    let steam_transport_config = SteamServerConfig {
        max_clients: 10,
        access_permission: AccessPermission::Public,
    };
    let transport = SteamServerTransport::new(&steam_client, steam_transport_config).unwrap();

    app.add_plugins(SteamServerPlugin);
    app.insert_resource(server);
    app.insert_non_send_resource(transport);
    app.insert_non_send_resource(single);

    fn steam_callbacks(client: NonSend<SingleClient>) {
        client.run_callbacks();
    }

    app.add_systems(PreUpdate, steam_callbacks);
}

fn main() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins);

    app.add_plugins(RenetServerPlugin);
    app.add_plugins(FrameTimeDiagnosticsPlugin);
    app.add_plugins(LogDiagnosticsPlugin::default());
    app.add_plugins(EguiPlugin);

    app.insert_resource(ServerLobby::default());

    app.insert_resource(RenetServerVisualizer::<200>::default());

    #[cfg(feature = "netcode")]
    add_netcode_network(&mut app);

    #[cfg(feature = "steam")]
    add_steam_network(&mut app);

    app.add_systems(
        Update,
        (
            server_update_system,
            server_network_sync,
            move_players_system,
        ),
    );

    app.add_systems(FixedUpdate, apply_velocity_system);

    app.add_systems(Startup, (spawn_world_model, setup_simple_camera));

    app.run();
}

#[allow(clippy::too_many_arguments)]
fn server_update_system(
    mut server_events: EventReader<ServerEvent>,
    mut commands: Commands,
    mut lobby: ResMut<ServerLobby>,
    mut server: ResMut<RenetServer>,
    players: Query<(Entity, &Player, &Transform)>,
) {
    for event in server_events.read() {
        match event {
            ServerEvent::ClientConnected { client_id } => {
                println!("Player {} connected.", client_id);

                // First, send all existing players to the new client
                for (entity, player, transform) in players.iter() {
                    println!("Sending existing player {} to new client {}", player.id, client_id);
                    let translation: [f32; 3] = transform.translation.into();
                    let rotation: [f32; 4] = transform.rotation.into();
                    
                    let message = bincode::serialize(&ServerMessages::PlayerCreate {
                        id: player.id,
                        entity,
                        translation,
                        rotation,
                    }).unwrap();
                    
                    // Send this existing player to the new client
                    server.send_message(*client_id, ServerChannel::ServerMessages, message);
                }

                // Then spawn the new player
                let transform = Transform::from_translation(PLAYER_SPAWN_POSITION);
                let player_entity = commands
                    .spawn((
                        Player { id: *client_id },
                        transform,
                        PlayerInput::default(),
                        Velocity(Vec3::ZERO),
                    ))
                    .id();
                
                lobby.players.insert(*client_id, player_entity);
                
                // Broadcast the new player to ALL clients (including the new one)
                let translation: [f32; 3] = transform.translation.into();
                let rotation: [f32; 4] = transform.rotation.into();
                
                let message = bincode::serialize(&ServerMessages::PlayerCreate {
                    id: *client_id,
                    entity: player_entity,
                    translation,
                    rotation,
                }).unwrap();
                
                server.broadcast_message(ServerChannel::ServerMessages, message);
            }
            ServerEvent::ClientDisconnected { client_id, reason } => {
                println!("Player {} disconnected: {}", client_id, reason);
                if let Some(player_entity) = lobby.players.remove(client_id) {
                    commands.entity(player_entity).despawn();
                }

                let message =
                    bincode::serialize(&ServerMessages::PlayerRemove { id: *client_id }).unwrap();
                server.broadcast_message(ServerChannel::ServerMessages, message);
            }
        }
    }

    for client_id in server.clients_id() {
        while let Some(message) = server.receive_message(client_id, ClientChannel::Input) {
            let input: PlayerInput = bincode::deserialize(&message).unwrap();
            if let Some(player_entity) = lobby.players.get(&client_id) {
                commands.entity(*player_entity).insert(input);
            }
        }
    }
}

#[allow(clippy::type_complexity)]
fn server_network_sync(
    mut server: ResMut<RenetServer>,
    query: Query<(Entity, &Transform), With<Player>>,
) {
    let mut networked_entities = NetworkedEntities::default();
    for (entity, transform) in query.iter() {
        networked_entities.entities.push(entity);
        networked_entities.translations.push(transform.translation.into());
        networked_entities.rotations.push(transform.rotation.into());
    }

    let sync_message = bincode::serialize(&networked_entities).unwrap();
    server.broadcast_message(ServerChannel::NetworkedEntities, sync_message);
}

fn move_players_system(mut query: Query<(&mut Velocity, &mut Transform, &PlayerInput)>) {
    for (mut velocity, mut transform, input) in query.iter_mut() {
        transform.rotation = Quat::from_euler(EulerRot::YXZ, input.yaw, input.pitch, 0.0);

        let x = (input.right as i8 - input.left as i8) as f32;
        let y = (input.down as i8 - input.up as i8) as f32;
        let direction = Vec2::new(x, y).normalize_or_zero();
        
        let forward = transform.forward();
        let right = transform.right();
        let movement = (forward * -y + right * x).normalize_or_zero() * PLAYER_MOVE_SPEED;
        
        velocity.0.x = movement.x;
        velocity.0.z = movement.z;
    }
}

fn apply_velocity_system(mut query: Query<(&Velocity, &mut Transform)>, time: Res<Time>) {
    for (velocity, mut transform) in query.iter_mut() {
        transform.translation += velocity.0 * time.delta_secs();
    }
}

pub fn setup_simple_camera(mut commands: Commands) {
    // camera
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(-20.5, 30.0, 20.5).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}
