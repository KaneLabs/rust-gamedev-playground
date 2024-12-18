use std::{collections::HashMap, f32::consts::PI, net::UdpSocket, time::SystemTime};

use bevy::{
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    prelude::*,
    window::ExitCondition,
};
use bevy_playground::{
    camera_zoom_system, connection_config, get_server_addr, setup_level, spawn_fireball,
    ClientChannel, NetworkedEntities, Player, PlayerCommand, PlayerInput, Projectile,
    ServerChannel, ServerMessages, SolanaSlotBlock, PROTOCOL_ID,
};
use bevy_rapier3d::prelude::*;
use bevy_renet::{
    renet::{
        transport::{NetcodeServerTransport, ServerAuthentication, ServerConfig},
        RenetServer, ServerEvent,
    },
    transport::NetcodeServerPlugin,
    RenetServerPlugin,
};

use solana_client::rpc_client::RpcClient;

#[cfg(debug_assertions)]
use bevy_egui::{EguiContexts, EguiPlugin};
#[cfg(debug_assertions)]
use renet_visualizer::RenetServerVisualizer;

pub const SOLANA_LOCALHOST: &'static str = "http://localhost:8899";
pub const SOLANA_DEVNET: &'static str = "https://api.devnet.solana.com";
pub const SOLANA_MAINNET: &'static str = "https://api.mainnet-beta.solana.com";

pub enum SolanaRpcUrl {
    Localhost,
    Devnet,
    Mainnet,
}

impl SolanaRpcUrl {
    fn default() -> Self {
        return SolanaRpcUrl::Mainnet;
    }

    fn as_str(&self) -> &'static str {
        match self {
            SolanaRpcUrl::Localhost => SOLANA_LOCALHOST,
            SolanaRpcUrl::Devnet => SOLANA_DEVNET,
            SolanaRpcUrl::Mainnet => SOLANA_MAINNET,
        }
    }
}

#[derive(Component, Resource)]
pub struct Solana {
    pub rpc: SolanaRpcUrl,
    pub client: RpcClient,
    pub faucet_on: bool,
}

impl Solana {
    fn default() -> Self {
        Solana {
            rpc: SolanaRpcUrl::default(),
            client: RpcClient::new(SolanaRpcUrl::default().as_str()),
            faucet_on: false,
        }
    }
}

#[derive(Debug, Default, Resource)]
pub struct ServerLobby {
    pub players: HashMap<u64, Entity>,
}

const PLAYER_MOVE_SPEED: f32 = 5.0;

#[derive(Debug, Component)]
struct Bot {
    auto_cast: Timer,
}

#[derive(Debug, Resource)]
struct BotId(u64);

fn new_renet_server() -> (RenetServer, NetcodeServerTransport) {
    let server = RenetServer::new(connection_config());

    let public_addr = get_server_addr().parse().unwrap();
    let socket = UdpSocket::bind(public_addr).unwrap();
    let server_config = ServerConfig {
        max_clients: 64,
        protocol_id: PROTOCOL_ID,
        public_addr,
        authentication: ServerAuthentication::Unsecure,
    };
    let current_time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();

    let transport = NetcodeServerTransport::new(current_time, server_config, socket).unwrap();

    (server, transport)
}

pub struct SolanaPlugin;

impl Plugin for SolanaPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(LogConnectionsTimer(Timer::from_seconds(
            30.0,
            TimerMode::Repeating,
        )))
        .insert_resource(Solana::default())
        // .add_startup_system(add_devnet_connection)
        .add_startup_system(add_mainnet_connection)
        .add_system(spawn_solana_blocks);
    }
}

fn add_mainnet_connection(mut commands: Commands) {
    commands.spawn(Solana {
        rpc: SolanaRpcUrl::Mainnet,
        client: RpcClient::new(SolanaRpcUrl::Mainnet.as_str()),
        faucet_on: true,
    });
}

#[derive(Resource)]
struct LogConnectionsTimer(Timer);

fn spawn_solana_blocks(
    time: Res<Time>,
    mut timer: ResMut<LogConnectionsTimer>,
    query: Query<&Solana>,
    solana: ResMut<Solana>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut server: ResMut<RenetServer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Runs after first tick
    if timer.0.tick(time.delta()).just_finished() {
        println!("Connected to  {}", solana.rpc.as_str());

        // Run for each connected solana rpc if it is on
        for solana in &query {
            if solana.faucet_on {
                let epoch = solana.client.get_epoch_info().unwrap();
                println!("Spawning Solana block for slot: {}", epoch.absolute_slot);

                let spawn_location_transform = Transform::from_xyz(0.0, 20.0, 0.0);
                println!("Spawn location: {:?}", spawn_location_transform.translation);

                // Spawn new
                let entity: Entity = commands
                    .spawn(PbrBundle {
                        mesh: meshes.add(Mesh::from(shape::Box::new(1.0, 1.0, 1.0))),
                        material: materials.add(Color::rgb(0.8, 0.7, 0.6).into()),
                        transform: spawn_location_transform,
                        ..Default::default()
                    })
                    .insert(RigidBody::Dynamic)
                    .insert(Collider::cuboid(1.0, 1.0, 1.0))
                    .insert(Restitution::coefficient(0.7))
                    .insert(SolanaSlotBlock {
                        id: epoch.absolute_slot,
                    })
                    .id();

                println!("Created Solana block entity: {:?}", entity);

                let message = ServerMessages::SpawnSolanaBlock {
                    entity: entity,
                    transform: (0.0, 20.0, 0.0),
                    slot: epoch.absolute_slot,
                };

                let message = bincode::serialize(&message).unwrap();
                server.broadcast_message(ServerChannel::ServerMessages, message);
                println!("Broadcasted Solana block spawn message");
            }
        }
    }
}

fn main() {
    let mut app = App::new();

    #[cfg(debug_assertions)]
    {
        app.add_plugins(DefaultPlugins);
        app.add_plugin(RapierDebugRenderPlugin::default());
        app.add_plugin(EguiPlugin);
    }
    #[cfg(not(debug_assertions))]
    {
        app.add_plugins(MinimalPlugins)
            .add_plugin(AssetPlugin::default())
            .add_plugin(TaskPoolPlugin::default())
            .add_plugin(TypeRegistrationPlugin::default())
            .add_plugin(FrameCountPlugin::default());
    }

    app.add_plugin(RenetServerPlugin);
    app.add_plugin(NetcodeServerPlugin);
    app.add_plugin(RapierPhysicsPlugin::<NoUserData>::default());
    app.add_plugin(FrameTimeDiagnosticsPlugin::default());
    app.add_plugin(LogDiagnosticsPlugin::default());

    app.add_plugin(SolanaPlugin);

    app.insert_resource(ServerLobby::default());
    app.insert_resource(BotId(0));

    #[cfg(debug_assertions)]
    app.add_plugin(EguiPlugin);

    let (server, transport) = new_renet_server();
    app.insert_resource(server);
    app.insert_resource(transport);

    #[cfg(debug_assertions)]
    app.insert_resource(RenetServerVisualizer::<200>::default());

    app.add_systems((
        server_update_system,
        server_network_sync,
        move_players_system,
        update_projectiles_system,
        #[cfg(debug_assertions)]
        update_visualizer_system,
        projectile_collision_system,
        spawn_bot,
        bot_autocast,
    ));

    app.add_system(projectile_on_removal_system.in_base_set(CoreSet::PostUpdate));
    app.add_system(solana_block_on_removal_system.in_base_set(CoreSet::PostUpdate));
    app.add_startup_system(setup_level);
    #[cfg(debug_assertions)]
    app.add_system(camera_zoom_system);
    #[cfg(debug_assertions)]
    app.add_system(camera_movement_system);

    app.run();
}

#[allow(clippy::too_many_arguments)]
fn server_update_system(
    mut server_events: EventReader<ServerEvent>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut lobby: ResMut<ServerLobby>,
    mut server: ResMut<RenetServer>,
    players: Query<(Entity, &Player, &Transform)>,
) {
    for event in server_events.iter() {
        match event {
            ServerEvent::ClientConnected { client_id } => {
                println!("Player {} connected.", client_id);

                // Initialize other players for this new client
                for (entity, player, transform) in players.iter() {
                    let translation: [f32; 3] = transform.translation.into();
                    let message = bincode::serialize(&ServerMessages::PlayerCreate {
                        id: player.id,
                        entity,
                        translation,
                    })
                    .unwrap();
                    server.send_message(*client_id, ServerChannel::ServerMessages, message);
                }

                // Spawn new player
                let transform = Transform::from_xyz(
                    (fastrand::f32() - 0.5) * 40.,
                    0.51,
                    (fastrand::f32() - 0.5) * 40.,
                );
                let player_entity = commands
                    .spawn(PbrBundle {
                        mesh: meshes.add(Mesh::from(shape::Capsule::default())),
                        material: materials.add(Color::rgb(0.8, 0.7, 0.6).into()),
                        transform,
                        ..Default::default()
                    })
                    .insert(RigidBody::Dynamic)
                    .insert(LockedAxes::ROTATION_LOCKED | LockedAxes::TRANSLATION_LOCKED_Y)
                    .insert(Collider::capsule_y(0.5, 0.5))
                    .insert(PlayerInput::default())
                    .insert(Velocity::default())
                    .insert(Player { id: *client_id })
                    .id();

                lobby.players.insert(*client_id, player_entity);

                let translation: [f32; 3] = transform.translation.into();
                let message = bincode::serialize(&ServerMessages::PlayerCreate {
                    id: *client_id,
                    entity: player_entity,
                    translation,
                })
                .unwrap();
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
        while let Some(message) = server.receive_message(client_id, ClientChannel::Command) {
            let command: PlayerCommand = bincode::deserialize(&message).unwrap();
            match command {
                PlayerCommand::BasicAttack { mut cast_at } => {
                    println!(
                        "Received basic attack from client {}: {:?}",
                        client_id, cast_at
                    );

                    if let Some(player_entity) = lobby.players.get(&client_id) {
                        if let Ok((_, _, player_transform)) = players.get(*player_entity) {
                            cast_at[1] = player_transform.translation[1];

                            let direction =
                                (cast_at - player_transform.translation).normalize_or_zero();
                            let mut translation = player_transform.translation + (direction * 0.7);
                            translation[1] = 1.0;

                            let fireball_entity = spawn_fireball(
                                &mut commands,
                                &mut meshes,
                                &mut materials,
                                translation,
                                direction,
                            );
                            let message = ServerMessages::SpawnProjectile {
                                entity: fireball_entity,
                                translation: translation.into(),
                            };
                            let message = bincode::serialize(&message).unwrap();
                            server.broadcast_message(ServerChannel::ServerMessages, message);
                        }
                    }
                }
            }
        }
        while let Some(message) = server.receive_message(client_id, ClientChannel::Input) {
            let input: PlayerInput = bincode::deserialize(&message).unwrap();
            if let Some(player_entity) = lobby.players.get(&client_id) {
                commands.entity(*player_entity).insert(input);
            }
        }
    }
}

fn update_projectiles_system(
    mut commands: Commands,
    mut projectiles: Query<(Entity, &mut Projectile)>,
    time: Res<Time>,
) {
    for (entity, mut projectile) in projectiles.iter_mut() {
        projectile.duration.tick(time.delta());
        if projectile.duration.finished() {
            commands.entity(entity).despawn();
        }
    }
}

#[allow(clippy::type_complexity)]
fn server_network_sync(
    mut server: ResMut<RenetServer>,
    query: Query<(Entity, &Transform), Or<(With<Player>, With<Projectile>, With<SolanaSlotBlock>)>>,
) {
    let mut networked_entities = NetworkedEntities::default();
    for (entity, transform) in query.iter() {
        networked_entities.entities.push(entity);
        networked_entities
            .translations
            .push(transform.translation.into());
    }

    let sync_message = bincode::serialize(&networked_entities).unwrap();
    server.broadcast_message(ServerChannel::NetworkedEntities, sync_message);
}

fn move_players_system(mut query: Query<(&mut Transform, &PlayerInput), With<Player>>) {
    for (mut transform, input) in query.iter_mut() {
        // Update the player's position based on the camera position
        transform.translation = Vec3::from(input.position);
    }
}

fn camera_movement_system(
    time: Res<Time>,
    keyboard_input: Res<Input<KeyCode>>,
    mut query: Query<&mut Transform, With<Camera>>,
) {
    let mut direction = Vec3::ZERO;
    if keyboard_input.pressed(KeyCode::W) {
        direction.z -= 1.0;
    }
    if keyboard_input.pressed(KeyCode::S) {
        direction.z += 1.0;
    }
    if keyboard_input.pressed(KeyCode::A) {
        direction.x -= 1.0;
    }
    if keyboard_input.pressed(KeyCode::D) {
        direction.x += 1.0;
    }

    if direction != Vec3::ZERO {
        let speed = 50.0; // Adjust the speed as needed
        for mut transform in query.iter_mut() {
            transform.translation += speed * direction.normalize() * time.delta_seconds();
        }
    }
}

pub fn setup_simple_camera(mut commands: Commands) {
    // camera
    commands.spawn(Camera3dBundle {
        transform: Transform::from_xyz(0., 30.0, 20.5).looking_at(Vec3::ZERO, Vec3::Y),
        ..Default::default()
    });
}

fn projectile_collision_system(
    mut commands: Commands,
    mut collision_events: EventReader<CollisionEvent>,
    projectile_query: Query<Option<&Projectile>>,
    solana_entity_query: Query<Option<&SolanaSlotBlock>>,
) {
    for collision_event in collision_events.iter() {
        if let CollisionEvent::Started(entity1, entity2, _) = collision_event {
            // let entity1Id = commands.entity(*entity1).id();
            // let entity2Id = commands.entity(*entity2).id();

            // commands.entity(entity2Id).despawn();
            println!("Projectile Collision Event Started");

            if let Ok(Some(_)) = projectile_query.get(*entity1) {
                println!("Projectile Collision Event Started");
                if let Ok(Some(_)) = solana_entity_query.get(*entity1) {
                    commands.entity(*entity2).despawn();
                }
                // commands.entity(*entity2).despawn();
            }
            if let Ok(Some(_)) = projectile_query.get(*entity2) {
                println!("Projectile Collision Event Started");
                if let Ok(Some(_)) = solana_entity_query.get(*entity1) {
                    commands.entity(*entity1).despawn();
                }
            }
        } else if let CollisionEvent::Stopped(e1, e2, _) = collision_event {
            println!("Collision Event Stopped");
        }
    }
}

fn projectile_on_removal_system(
    mut server: ResMut<RenetServer>,
    mut removed_projectiles: RemovedComponents<Projectile>,
) {
    for entity in &mut removed_projectiles {
        let message = ServerMessages::DespawnProjectile { entity };
        let message = bincode::serialize(&message).unwrap();

        server.broadcast_message(ServerChannel::ServerMessages, message);
    }
}

fn solana_block_on_removal_system(
    mut server: ResMut<RenetServer>,
    mut removed_projectiles: RemovedComponents<SolanaSlotBlock>,
) {
    for entity in &mut removed_projectiles {
        let message = ServerMessages::DespawnSolanaBlock { entity };
        let message = bincode::serialize(&message).unwrap();

        server.broadcast_message(ServerChannel::ServerMessages, message);
    }
}

fn spawn_bot(
    keyboard_input: Res<Input<KeyCode>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut lobby: ResMut<ServerLobby>,
    mut server: ResMut<RenetServer>,
    mut bot_id: ResMut<BotId>,
    mut commands: Commands,
) {
    if keyboard_input.just_pressed(KeyCode::Space) {
        let client_id = bot_id.0;
        bot_id.0 += 1;
        // Spawn new player
        let transform = Transform::from_xyz(
            (fastrand::f32() - 0.5) * 40.,
            0.51,
            (fastrand::f32() - 0.5) * 40.,
        );
        let player_entity = commands
            .spawn(PbrBundle {
                mesh: meshes.add(Mesh::from(shape::Capsule::default())),
                material: materials.add(Color::rgb(0.8, 0.7, 0.6).into()),
                transform,
                ..Default::default()
            })
            .insert(RigidBody::Fixed)
            .insert(LockedAxes::ROTATION_LOCKED | LockedAxes::TRANSLATION_LOCKED_Y)
            .insert(Collider::capsule_y(0.5, 0.5))
            .insert(Player { id: client_id })
            .insert(Bot {
                auto_cast: Timer::from_seconds(3.0, TimerMode::Repeating),
            })
            .id();

        lobby.players.insert(client_id, player_entity);

        let translation: [f32; 3] = transform.translation.into();
        let message = bincode::serialize(&ServerMessages::PlayerCreate {
            id: client_id,
            entity: player_entity,
            translation,
        })
        .unwrap();
        server.broadcast_message(ServerChannel::ServerMessages, message);
    }
}

fn bot_autocast(
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut server: ResMut<RenetServer>,
    mut bots: Query<(&Transform, &mut Bot), With<Player>>,
    mut commands: Commands,
) {
    for (transform, mut bot) in &mut bots {
        bot.auto_cast.tick(time.delta());
        if !bot.auto_cast.just_finished() {
            continue;
        }

        for i in 0..8 {
            let direction = Vec2::from_angle(PI / 4. * i as f32);
            let direction = Vec3::new(direction.x, 0., direction.y).normalize();
            let translation: Vec3 = transform.translation + direction;

            let fireball_entity = spawn_fireball(
                &mut commands,
                &mut meshes,
                &mut materials,
                translation,
                direction,
            );
            let message = ServerMessages::SpawnProjectile {
                entity: fireball_entity,
                translation: translation.into(),
            };
            let message = bincode::serialize(&message).unwrap();
            server.broadcast_message(ServerChannel::ServerMessages, message);
        }
    }
}

#[cfg(debug_assertions)]
fn update_visualizer_system(
    mut egui_contexts: EguiContexts,
    mut visualizer: ResMut<RenetServerVisualizer<200>>,
    server: Res<RenetServer>,
) {
    visualizer.update(&server);
    visualizer.show_window(egui_contexts.ctx_mut());
}
