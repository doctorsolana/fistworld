mod input;
mod state;
mod ui;
mod visuals;

pub(crate) use input::planned_plot_placements;
pub(crate) use input::snap_road_point;
pub use input::{handle_city_shortcuts, handle_city_tool_input, handle_city_ui_actions};
pub use state::{
    CityEditorState, EditorCityPreviewVisual, EditorPlotBuildingVisual, EditorPlotVisual,
    EditorRoadVisual, PlotToolSettings, RoadToolSettings,
};
pub use ui::draw_city_tool_controls;
pub use visuals::{apply_city_visual_refresh, setup_city_scene, update_city_preview_visuals};
