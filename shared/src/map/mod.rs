//! Fixed authored-map schema + loaders shared by client and server.

mod editor_schema;
mod loader;
mod save;
mod schema;

pub use crate::city::{MapPlot, MapRoad, PlotArchetype, PlotZone, RoadClass, RoadSide};
pub use editor_schema::*;
pub use loader::*;
pub use save::*;
pub use schema::*;
