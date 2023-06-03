use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugin(HelloPlugin)
        .run();
}

// System
// Added to App's Schedule: https://docs.rs/bevy_ecs/latest/bevy_ecs/schedule/struct.Schedule.html
fn hello_world() {
    println!("hello world!");
}

// Components
#[derive(Component)]
struct Person;

#[derive(Component)]
struct Name(String);

fn add_people(mut commands: Commands) {
    let rees_bundle = (Person, Name("Rees LaBreee".to_string()));
    let andy_bundle = (Person, Name("Andy Buckhovich".to_string()));
    let ryan_bundle = (Person, Name("Ryan Kane".to_string()));
    commands.spawn(rees_bundle);
    commands.spawn(andy_bundle);
    commands.spawn(ryan_bundle);
}

fn greet_people(query: Query<&Name, With<Person>>) {
    for name in &query {
        println!("hello {}!", name.0);
    }
}

// Plugins
pub struct HelloPlugin;
impl Plugin for HelloPlugin {
    fn build(&self, app: &mut App) {
        app.add_startup_system(add_people)
            .add_system(hello_world)
            .add_system(greet_people);
    }
}
