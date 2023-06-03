
use bevy::prelude::*;

fn main() {
    App::new().add_system(hello_world).run();
}


// System
// Added to App's Schedule: https://docs.rs/bevy_ecs/latest/bevy_ecs/schedule/struct.Schedule.html
fn hello_world() {
    println!("hello world!");
}