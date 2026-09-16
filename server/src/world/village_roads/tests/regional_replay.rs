use super::*;

/// Replay accepted farm parcels without rebuilding a different procedural
/// town. The town export records terrain, buildings, yards and fences; runtime
/// prop removals are not part of this snapshot and are deliberately excluded.
#[test]
#[ignore = "loads a recorded map; run isolated with FISTWORLD_LAB_FARM_REPLAY"]
fn replay_recorded_farm_work_access() {
    let path = std::env::var("FISTWORLD_LAB_FARM_REPLAY").expect("a town snapshot path");
    let snapshot: shared::settlement_snapshot::TownSnapshot =
        serde_json::from_reader(std::fs::File::open(path).unwrap()).unwrap();
    let mut terrain =
        WorldTerrain::from_loaded_map(shared::map::load_map(&snapshot.map_id).unwrap());
    assert_eq!(
        terrain.generator.active_map_content_hash(),
        snapshot.map_content_hash
    );
    for chunk in snapshot.terrain_deltas {
        terrain.set_delta_chunk(chunk.coord, chunk.to_delta_data());
    }
    let wanted = std::env::var("FISTWORLD_LAB_FARM_ID")
        .ok()
        .and_then(|id| id.parse::<u64>().ok())
        .unwrap_or(15);
    let farm = snapshot
        .buildings
        .iter()
        .find(|b| b.id == Some(shared::components::BuildingId(wanted)))
        .expect("the requested farm exists in the snapshot");
    let fields: Vec<_> = snapshot
        .fields
        .iter()
        .filter(|field| field.component.farmstead.distance_squared(farm.position) < 0.01)
        .collect();
    assert_eq!(fields.len(), 2);
    let mut live = SpatialObstacleGrid::default();
    for layer in ["terrain", "buildings", "fields", "yards and walls"] {
        match layer {
            "buildings" => {
                for building in &snapshot.buildings {
                    live.insert(shared::spatial::ObstacleEntry {
                        center: building.footprint_center,
                        half_extents: building.footprint * 0.5 + Vec2::splat(CHARACTER_NAV_RADIUS),
                        rotation: building.rotation,
                        obstacle_type: 0,
                    });
                }
            }
            "fields" => {
                for field in &snapshot.fields {
                    for obstacle in field
                        .component
                        .ground_obstacles(field.position, field.rotation)
                    {
                        live.insert(obstacle);
                    }
                }
            }
            "yards and walls" => {
                for building in &snapshot.buildings {
                    if let Some(yard) = &building.yard {
                        for obstacle in yard.ground_obstacles(building.position, building.rotation)
                        {
                            live.insert(obstacle);
                        }
                    }
                }
                for wall in &snapshot.fortifications {
                    for obstacle in wall.ground_obstacles() {
                        live.insert(obstacle);
                    }
                }
            }
            _ => {}
        }
        let mut accepted = 0;
        for field in &fields {
            for salt in 0..4 {
                let stand = reachable_farm_work_stand_in_shape(
                    &terrain,
                    farm.position,
                    farm.rotation,
                    field.position,
                    salt,
                    Some(&live),
                    None,
                    None,
                    field.component.shape.as_ref(),
                );
                eprintln!(
                    "FARM REPLAY {layer} farm={wanted} plot={} salt={salt} stand={stand:?}",
                    field.component.plot_index
                );
                accepted += usize::from(stand.is_some());
            }
        }
        if layer == "yards and walls" {
            assert_eq!(
                accepted, 8,
                "a staffed completed farm must retain certified work access"
            );
        }
    }
}

fn point(value: &serde_json::Value) -> Vec2 {
    Vec2::new(
        value[0].as_f64().unwrap() as f32,
        value[1].as_f64().unwrap() as f32,
    )
}

/// Run with FISTWORLD_LAB_ROUTE_REPLAY pointing at one opt-in planner snapshot.
/// Keep map-bound changes out of the ordinary parallel unit suite.
#[test]
#[ignore = "loads a recorded map; run isolated with FISTWORLD_LAB_ROUTE_REPLAY"]
fn replay_recorded_regional_survey() {
    let path = std::env::var("FISTWORLD_LAB_ROUTE_REPLAY").expect("a failed survey snapshot path");
    let snapshot: serde_json::Value =
        serde_json::from_reader(std::fs::File::open(path).unwrap()).unwrap();
    let map = snapshot["map"].as_str().unwrap();
    let authored = WorldTerrain::from_loaded_map(shared::map::load_map(map).unwrap());
    assert_eq!(
        authored.generator.active_map_content_hash(),
        snapshot["map_hash"].as_u64().unwrap()
    );
    let mut edited = WorldTerrain::from_loaded_map(shared::map::load_map(map).unwrap());
    for chunk in snapshot["terrain_deltas"].as_array().unwrap() {
        edited.set_delta_chunk(
            ChunkCoord::new(
                chunk[0].as_i64().unwrap() as i32,
                chunk[1].as_i64().unwrap() as i32,
            ),
            shared::terrain::TerrainDeltaData {
                deltas: chunk[2]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_f64().unwrap() as f32)
                    .collect(),
                version: 1,
            },
        );
    }
    let buildings: Vec<_> = snapshot["buildings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| BuildingBlocker {
            center: point(b),
            half: Vec2::new(b[2].as_f64().unwrap() as f32, b[3].as_f64().unwrap() as f32),
            rotation: b[4].as_f64().unwrap() as f32,
        })
        .collect();
    let mut live = SpatialObstacleGrid::default();
    for b in &buildings {
        live.insert(shared::spatial::ObstacleEntry {
            center: b.center,
            half_extents: b.half,
            rotation: b.rotation,
            obstacle_type: 0,
        });
    }
    let mut props = PropBlockers::default();
    for prop in snapshot["props"].as_array().unwrap() {
        props.insert_radius(point(prop), prop[2].as_f64().unwrap() as f32);
    }
    let empty_props = PropBlockers::default();
    let mut complete = false;
    for (label, terrain, props, has_buildings) in [
        ("authored terrain", &authored, &empty_props, false),
        ("edited terrain", &edited, &empty_props, false),
        ("edited + buildings", &edited, &empty_props, true),
        ("edited + props", &edited, &props, false),
        ("full snapshot", &edited, &props, true),
    ] {
        let survey = RoadSurvey {
            decks: None,
            terrain,
            buildings: if has_buildings { &buildings } else { &[] },
            live_buildings: has_buildings.then_some(&live),
            props,
            start: point(&snapshot["start"]),
            goal: point(&snapshot["goal"]),
            min: point(&snapshot["min"]),
            max: point(&snapshot["max"]),
            max_nodes: snapshot["max_nodes"].as_u64().unwrap() as usize,
            cell_size: snapshot["cell_size"].as_f64().unwrap() as f32,
            coarse_stride: snapshot["stride"].as_i64().unwrap() as i32,
            fine_endpoint_radius: snapshot["fine_endpoint_radius"].as_f64().unwrap() as f32,
        };
        let mut scratch = SurveyScratch::default();
        let mut state = SurveySearchState::default();
        let result = resume_survey_a_star(&survey, &mut scratch, &mut state, None);
        let found = matches!(result, SurveySearchResult::Found(_));
        eprintln!(
            "REPLAY {label}: found={found} expanded={} frontier={} closest={:?}",
            state.expanded,
            scratch.open.len(),
            scratch
                .closed
                .iter()
                .map(|cell| survey_point(*cell, survey.cell_size))
                .min_by(|a, b| a
                    .distance_squared(survey.goal)
                    .total_cmp(&b.distance_squared(survey.goal)))
        );
        if let SurveySearchResult::Found(path) = result {
            assert!(
                path.windows(2)
                    .all(|p| survey.line_clear(p[0], p[1], &mut scratch))
            );
            if label == "full snapshot" {
                complete = true;
            }
        }
    }
    assert!(
        complete,
        "the recorded full live corridor is still unreachable"
    );
}
