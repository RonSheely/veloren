use common_ecs::dispatch;
use common_state::State;
use specs::DispatcherBuilder;

mod sim;
mod tick;

pub use tick::WeatherJob;

/// How often the weather is updated, in seconds
const WEATHER_DT: f32 = 5.0;

#[cfg(feature = "worldgen")]
pub fn add_server_systems(dispatch_builder: &mut DispatcherBuilder) {
    dispatch::<tick::Sys>(dispatch_builder, &[]);
}

#[cfg(feature = "worldgen")]
pub fn init(state: &mut State) {
    use crate::weather::sim::LightningCells;

    state.ecs_mut().insert(None::<WeatherJob>);
    state.ecs_mut().insert(LightningCells::default());
}
