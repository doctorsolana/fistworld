//! Fixed authored-map schema + loaders shared by client and server.

mod authored_schema;
mod loader;
mod save;
mod schema;
mod session;

pub use crate::city::{MapPlot, MapRoad, PlotArchetype, PlotZone, RoadClass, RoadSide};
pub use authored_schema::*;
pub use loader::*;
pub use save::*;
pub use schema::*;
pub use session::*;
