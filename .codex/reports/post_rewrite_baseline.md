# Post Rewrite Baseline
Generated: 2026-02-13T21:23:00Z

## cargo check --workspace
    Checking collider_baker v0.1.0 (/Users/terninator/coding/citysim/tools/collider_baker)
error[E0412]: cannot find type `PropKind` in crate `shared`
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:133:50
    |
133 |     let mut prop_lookup: HashMap<String, shared::PropKind> = HashMap::new();
    |                                                  ^^^^^^^^ not found in `shared`
    |
help: consider importing this enum
    |
  7 + use shared::props::PropKind;
    |
help: if you import `PropKind`, refer to it directly
    |
133 -     let mut prop_lookup: HashMap<String, shared::PropKind> = HashMap::new();
133 +     let mut prop_lookup: HashMap<String, PropKind> = HashMap::new();
    |

error[E0412]: cannot find type `PropKind` in crate `shared`
   --> tools/collider_baker/src/main.rs:131:50
    |
131 |     let mut prop_lookup: HashMap<String, shared::PropKind> = HashMap::new();
    |                                                  ^^^^^^^^ not found in `shared`
    |
help: consider importing this enum
    |
  7 + use shared::props::PropKind;
    |
help: if you import `PropKind`, refer to it directly
    |
131 -     let mut prop_lookup: HashMap<String, shared::PropKind> = HashMap::new();
131 +     let mut prop_lookup: HashMap<String, PropKind> = HashMap::new();
    |

error[E0425]: cannot find value `ALL_PROP_KINDS` in crate `shared`
   --> tools/collider_baker/src/main.rs:132:22
    |
132 |     for k in shared::ALL_PROP_KINDS.iter().copied() {
    |                      ^^^^^^^^^^^^^^ not found in `shared`
    |
help: consider importing this constant
    |
  7 + use shared::props::ALL_PROP_KINDS;
    |
help: if you import `ALL_PROP_KINDS`, refer to it directly
    |
132 -     for k in shared::ALL_PROP_KINDS.iter().copied() {
132 +     for k in ALL_PROP_KINDS.iter().copied() {
    |

error[E0425]: cannot find value `ALL_PROP_KINDS` in crate `shared`
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:134:22
    |
134 |     for k in shared::ALL_PROP_KINDS.iter().copied() {
    |                      ^^^^^^^^^^^^^^ not found in `shared`
    |
help: consider importing this constant
    |
  7 + use shared::props::ALL_PROP_KINDS;
    |
help: if you import `ALL_PROP_KINDS`, refer to it directly
    |
134 -     for k in shared::ALL_PROP_KINDS.iter().copied() {
134 +     for k in ALL_PROP_KINDS.iter().copied() {
    |

error[E0412]: cannot find type `BuildingType` in crate `shared`
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:139:54
    |
139 |     let mut building_lookup: HashMap<String, shared::BuildingType> = HashMap::new();
    |                                                      ^^^^^^^^^^^^ not found in `shared`
    |
help: consider importing this enum
    |
  7 + use shared::prelude::BuildingType;
    |
help: if you import `BuildingType`, refer to it directly
    |
139 -     let mut building_lookup: HashMap<String, shared::BuildingType> = HashMap::new();
139 +     let mut building_lookup: HashMap<String, BuildingType> = HashMap::new();
    |

error[E0412]: cannot find type `BuildingType` in crate `shared`
   --> tools/collider_baker/src/main.rs:137:54
    |
137 |     let mut building_lookup: HashMap<String, shared::BuildingType> = HashMap::new();
    |                                                      ^^^^^^^^^^^^ not found in `shared`
    |
help: consider importing this enum
    |
  7 + use shared::prelude::BuildingType;
    |
help: if you import `BuildingType`, refer to it directly
    |
137 -     let mut building_lookup: HashMap<String, shared::BuildingType> = HashMap::new();
137 +     let mut building_lookup: HashMap<String, BuildingType> = HashMap::new();
    |

error[E0425]: cannot find value `ALL_BUILDING_TYPES` in crate `shared`
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:140:22
    |
140 |     for b in shared::ALL_BUILDING_TYPES.iter().copied() {
    |                      ^^^^^^^^^^^^^^^^^^ not found in `shared`
    |
help: consider importing this constant
    |
  7 + use shared::building::ALL_BUILDING_TYPES;
    |
help: if you import `ALL_BUILDING_TYPES`, refer to it directly
    |
140 -     for b in shared::ALL_BUILDING_TYPES.iter().copied() {
140 +     for b in ALL_BUILDING_TYPES.iter().copied() {
    |

error[E0425]: cannot find value `ALL_BUILDING_TYPES` in crate `shared`
   --> tools/collider_baker/src/main.rs:138:22
    |
138 |     for b in shared::ALL_BUILDING_TYPES.iter().copied() {
    |                      ^^^^^^^^^^^^^^^^^^ not found in `shared`
    |
help: consider importing this constant
    |
  7 + use shared::building::ALL_BUILDING_TYPES;
    |
help: if you import `ALL_BUILDING_TYPES`, refer to it directly
    |
138 -     for b in shared::ALL_BUILDING_TYPES.iter().copied() {
138 +     for b in ALL_BUILDING_TYPES.iter().copied() {
    |

error[E0412]: cannot find type `BakedCollider` in crate `shared`
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:231:50
    |
231 |     let mut out_entries: HashMap<String, shared::BakedCollider> = HashMap::new();
    |                                                  ^^^^^^^^^^^^^ not found in `shared`
    |
help: consider importing this enum
    |
  7 + use shared::colliders::BakedCollider;
    |
help: if you import `BakedCollider`, refer to it directly
    |
231 -     let mut out_entries: HashMap<String, shared::BakedCollider> = HashMap::new();
231 +     let mut out_entries: HashMap<String, BakedCollider> = HashMap::new();
    |

error[E0412]: cannot find type `BakedCollider` in crate `shared`
   --> tools/collider_baker/src/main.rs:229:50
    |
229 |     let mut out_entries: HashMap<String, shared::BakedCollider> = HashMap::new();
    |                                                  ^^^^^^^^^^^^^ not found in `shared`
    |
help: consider importing this enum
    |
  7 + use shared::colliders::BakedCollider;
    |
help: if you import `BakedCollider`, refer to it directly
    |
229 -     let mut out_entries: HashMap<String, shared::BakedCollider> = HashMap::new();
229 +     let mut out_entries: HashMap<String, BakedCollider> = HashMap::new();
    |

error[E0433]: failed to resolve: could not find `BakedCollider` in `shared`
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:302:29
    |
302 |                     shared::BakedCollider::ConvexHull { points: hull },
    |                             ^^^^^^^^^^^^^ could not find `BakedCollider` in `shared`
    |
help: consider importing this enum
    |
  7 + use shared::colliders::BakedCollider;
    |
help: if you import `BakedCollider`, refer to it directly
    |
302 -                     shared::BakedCollider::ConvexHull { points: hull },
302 +                     BakedCollider::ConvexHull { points: hull },
    |

error[E0433]: failed to resolve: could not find `BakedCollider` in `shared`
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:347:33
    |
347 |                         shared::BakedCollider::ConvexHull { points: hull },
    |                                 ^^^^^^^^^^^^^ could not find `BakedCollider` in `shared`
    |
help: consider importing this enum
    |
  7 + use shared::colliders::BakedCollider;
    |
help: if you import `BakedCollider`, refer to it directly
    |
347 -                         shared::BakedCollider::ConvexHull { points: hull },
347 +                         BakedCollider::ConvexHull { points: hull },
    |

error[E0433]: failed to resolve: could not find `BakedCollider` in `shared`
   --> tools/collider_baker/src/main.rs:304:21
    |
304 |             shared::BakedCollider::ConvexHull { points: hull },
    |                     ^^^^^^^^^^^^^ could not find `BakedCollider` in `shared`
    |
help: consider importing this enum
    |
  7 + use shared::colliders::BakedCollider;
    |
help: if you import `BakedCollider`, refer to it directly
    |
304 -             shared::BakedCollider::ConvexHull { points: hull },
304 +             BakedCollider::ConvexHull { points: hull },
    |

error[E0433]: failed to resolve: could not find `BakedCollider` in `shared`
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:364:29
    |
364 |                     shared::BakedCollider::CompoundConvex { hulls },
    |                             ^^^^^^^^^^^^^ could not find `BakedCollider` in `shared`
    |
help: consider importing this enum
    |
  7 + use shared::colliders::BakedCollider;
    |
help: if you import `BakedCollider`, refer to it directly
    |
364 -                     shared::BakedCollider::CompoundConvex { hulls },
364 +                     BakedCollider::CompoundConvex { hulls },
    |

error[E0422]: cannot find struct, variant or union type `BakedColliderDb` in crate `shared`
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:370:22
    |
370 |     let db = shared::BakedColliderDb {
    |                      ^^^^^^^^^^^^^^^ not found in `shared`
    |
help: consider importing this struct
    |
  7 + use shared::colliders::BakedColliderDb;
    |
help: if you import `BakedColliderDb`, refer to it directly
    |
370 -     let db = shared::BakedColliderDb {
370 +     let db = BakedColliderDb {
    |

error[E0422]: cannot find struct, variant or union type `BakedColliderDb` in crate `shared`
   --> tools/collider_baker/src/main.rs:308:22
    |
308 |     let db = shared::BakedColliderDb {
    |                      ^^^^^^^^^^^^^^^ not found in `shared`
    |
help: consider importing this struct
    |
  7 + use shared::colliders::BakedColliderDb;
    |
help: if you import `BakedColliderDb`, refer to it directly
    |
308 -     let db = shared::BakedColliderDb {
308 +     let db = BakedColliderDb {
    |

error[E0282]: type annotations needed
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:135:28
    |
135 |         prop_lookup.insert(k.id().to_string(), k);
    |                            ^ cannot infer type

error[E0282]: type annotations needed
   --> tools/collider_baker/src/main.rs:133:28
    |
133 |         prop_lookup.insert(k.id().to_string(), k);
    |                            ^ cannot infer type

error[E0282]: type annotations needed
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:141:32
    |
141 |         building_lookup.insert(b.id().to_string(), b);
    |                                ^ cannot infer type

error[E0282]: type annotations needed
   --> tools/collider_baker/src/main.rs:139:32
    |
139 |         building_lookup.insert(b.id().to_string(), b);
    |                                ^ cannot infer type

error[E0282]: type annotations needed
   --> tools/collider_baker/src/main.rs:157:13
    |
157 |             bt.scene_path().map(|s| s.to_string())
    |             ^^ cannot infer type

error[E0282]: type annotations needed
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:159:13
    |
159 |             bt.scene_path().map(|s| s.to_string())
    |             ^^ cannot infer type

error[E0282]: type annotations needed
   --> tools/collider_baker/src/main.rs:157:34
    |
157 |             bt.scene_path().map(|s| s.to_string())
    |                                  ^  - type must be known at this point
    |
help: consider giving this closure parameter an explicit type
    |
157 |             bt.scene_path().map(|s: /* Type */| s.to_string())
    |                                   ++++++++++++

error[E0282]: type annotations needed
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:159:34
    |
159 |             bt.scene_path().map(|s| s.to_string())
    |                                  ^  - type must be known at this point
    |
help: consider giving this closure parameter an explicit type
    |
159 |             bt.scene_path().map(|s: /* Type */| s.to_string())
    |                                   ++++++++++++

error[E0282]: type annotations needed
   --> tools/collider_baker/src/main.rs:166:18
    |
166 |             Some(pk.scene_path().to_string())
    |                  ^^ cannot infer type

error[E0282]: type annotations needed
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:168:18
    |
168 |             Some(pk.scene_path().to_string())
    |                  ^^ cannot infer type

Some errors have detailed explanations: E0282, E0412, E0422, E0425, E0433.
For more information about an error, try `rustc --explain E0282`.
error: could not compile `collider_baker` (bin "collider_baker_v2") due to 14 previous errors
warning: build failed, waiting for other jobs to finish...
error: could not compile `collider_baker` (bin "collider_baker") due to 12 previous errors

## cargo test --workspace
   Compiling collider_baker v0.1.0 (/Users/terninator/coding/citysim/tools/collider_baker)
error[E0412]: cannot find type `PropKind` in crate `shared`
   --> tools/collider_baker/src/main.rs:131:50
    |
131 |     let mut prop_lookup: HashMap<String, shared::PropKind> = HashMap::new();
    |                                                  ^^^^^^^^ not found in `shared`
    |
help: consider importing this enum
    |
  7 + use shared::props::PropKind;
    |
help: if you import `PropKind`, refer to it directly
    |
131 -     let mut prop_lookup: HashMap<String, shared::PropKind> = HashMap::new();
131 +     let mut prop_lookup: HashMap<String, PropKind> = HashMap::new();
    |

error[E0425]: cannot find value `ALL_PROP_KINDS` in crate `shared`
   --> tools/collider_baker/src/main.rs:132:22
    |
132 |     for k in shared::ALL_PROP_KINDS.iter().copied() {
    |                      ^^^^^^^^^^^^^^ not found in `shared`
    |
help: consider importing this constant
    |
  7 + use shared::props::ALL_PROP_KINDS;
    |
help: if you import `ALL_PROP_KINDS`, refer to it directly
    |
132 -     for k in shared::ALL_PROP_KINDS.iter().copied() {
132 +     for k in ALL_PROP_KINDS.iter().copied() {
    |

error[E0412]: cannot find type `BuildingType` in crate `shared`
   --> tools/collider_baker/src/main.rs:137:54
    |
137 |     let mut building_lookup: HashMap<String, shared::BuildingType> = HashMap::new();
    |                                                      ^^^^^^^^^^^^ not found in `shared`
    |
help: consider importing this enum
    |
  7 + use shared::prelude::BuildingType;
    |
help: if you import `BuildingType`, refer to it directly
    |
137 -     let mut building_lookup: HashMap<String, shared::BuildingType> = HashMap::new();
137 +     let mut building_lookup: HashMap<String, BuildingType> = HashMap::new();
    |

error[E0425]: cannot find value `ALL_BUILDING_TYPES` in crate `shared`
   --> tools/collider_baker/src/main.rs:138:22
    |
138 |     for b in shared::ALL_BUILDING_TYPES.iter().copied() {
    |                      ^^^^^^^^^^^^^^^^^^ not found in `shared`
    |
help: consider importing this constant
    |
  7 + use shared::building::ALL_BUILDING_TYPES;
    |
help: if you import `ALL_BUILDING_TYPES`, refer to it directly
    |
138 -     for b in shared::ALL_BUILDING_TYPES.iter().copied() {
138 +     for b in ALL_BUILDING_TYPES.iter().copied() {
    |

error[E0412]: cannot find type `PropKind` in crate `shared`
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:133:50
    |
133 |     let mut prop_lookup: HashMap<String, shared::PropKind> = HashMap::new();
    |                                                  ^^^^^^^^ not found in `shared`
    |
help: consider importing this enum
    |
  7 + use shared::props::PropKind;
    |
help: if you import `PropKind`, refer to it directly
    |
133 -     let mut prop_lookup: HashMap<String, shared::PropKind> = HashMap::new();
133 +     let mut prop_lookup: HashMap<String, PropKind> = HashMap::new();
    |

error[E0412]: cannot find type `BakedCollider` in crate `shared`
   --> tools/collider_baker/src/main.rs:229:50
    |
229 |     let mut out_entries: HashMap<String, shared::BakedCollider> = HashMap::new();
    |                                                  ^^^^^^^^^^^^^ not found in `shared`
    |
help: consider importing this enum
    |
  7 + use shared::colliders::BakedCollider;
    |
help: if you import `BakedCollider`, refer to it directly
    |
229 -     let mut out_entries: HashMap<String, shared::BakedCollider> = HashMap::new();
229 +     let mut out_entries: HashMap<String, BakedCollider> = HashMap::new();
    |

error[E0433]: failed to resolve: could not find `BakedCollider` in `shared`
   --> tools/collider_baker/src/main.rs:304:21
    |
304 |             shared::BakedCollider::ConvexHull { points: hull },
    |                     ^^^^^^^^^^^^^ could not find `BakedCollider` in `shared`
    |
help: consider importing this enum
    |
  7 + use shared::colliders::BakedCollider;
    |
help: if you import `BakedCollider`, refer to it directly
    |
304 -             shared::BakedCollider::ConvexHull { points: hull },
304 +             BakedCollider::ConvexHull { points: hull },
    |

error[E0422]: cannot find struct, variant or union type `BakedColliderDb` in crate `shared`
   --> tools/collider_baker/src/main.rs:308:22
    |
308 |     let db = shared::BakedColliderDb {
    |                      ^^^^^^^^^^^^^^^ not found in `shared`
    |
help: consider importing this struct
    |
  7 + use shared::colliders::BakedColliderDb;
    |
help: if you import `BakedColliderDb`, refer to it directly
    |
308 -     let db = shared::BakedColliderDb {
308 +     let db = BakedColliderDb {
    |

error[E0425]: cannot find value `ALL_PROP_KINDS` in crate `shared`
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:134:22
    |
134 |     for k in shared::ALL_PROP_KINDS.iter().copied() {
    |                      ^^^^^^^^^^^^^^ not found in `shared`
    |
help: consider importing this constant
    |
  7 + use shared::props::ALL_PROP_KINDS;
    |
help: if you import `ALL_PROP_KINDS`, refer to it directly
    |
134 -     for k in shared::ALL_PROP_KINDS.iter().copied() {
134 +     for k in ALL_PROP_KINDS.iter().copied() {
    |

error[E0412]: cannot find type `BuildingType` in crate `shared`
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:139:54
    |
139 |     let mut building_lookup: HashMap<String, shared::BuildingType> = HashMap::new();
    |                                                      ^^^^^^^^^^^^ not found in `shared`
    |
help: consider importing this enum
    |
  7 + use shared::prelude::BuildingType;
    |
help: if you import `BuildingType`, refer to it directly
    |
139 -     let mut building_lookup: HashMap<String, shared::BuildingType> = HashMap::new();
139 +     let mut building_lookup: HashMap<String, BuildingType> = HashMap::new();
    |

error[E0425]: cannot find value `ALL_BUILDING_TYPES` in crate `shared`
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:140:22
    |
140 |     for b in shared::ALL_BUILDING_TYPES.iter().copied() {
    |                      ^^^^^^^^^^^^^^^^^^ not found in `shared`
    |
help: consider importing this constant
    |
  7 + use shared::building::ALL_BUILDING_TYPES;
    |
help: if you import `ALL_BUILDING_TYPES`, refer to it directly
    |
140 -     for b in shared::ALL_BUILDING_TYPES.iter().copied() {
140 +     for b in ALL_BUILDING_TYPES.iter().copied() {
    |

error[E0412]: cannot find type `BakedCollider` in crate `shared`
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:231:50
    |
231 |     let mut out_entries: HashMap<String, shared::BakedCollider> = HashMap::new();
    |                                                  ^^^^^^^^^^^^^ not found in `shared`
    |
help: consider importing this enum
    |
  7 + use shared::colliders::BakedCollider;
    |
help: if you import `BakedCollider`, refer to it directly
    |
231 -     let mut out_entries: HashMap<String, shared::BakedCollider> = HashMap::new();
231 +     let mut out_entries: HashMap<String, BakedCollider> = HashMap::new();
    |

error[E0433]: failed to resolve: could not find `BakedCollider` in `shared`
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:302:29
    |
302 |                     shared::BakedCollider::ConvexHull { points: hull },
    |                             ^^^^^^^^^^^^^ could not find `BakedCollider` in `shared`
    |
help: consider importing this enum
    |
  7 + use shared::colliders::BakedCollider;
    |
help: if you import `BakedCollider`, refer to it directly
    |
302 -                     shared::BakedCollider::ConvexHull { points: hull },
302 +                     BakedCollider::ConvexHull { points: hull },
    |

error[E0433]: failed to resolve: could not find `BakedCollider` in `shared`
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:347:33
    |
347 |                         shared::BakedCollider::ConvexHull { points: hull },
    |                                 ^^^^^^^^^^^^^ could not find `BakedCollider` in `shared`
    |
help: consider importing this enum
    |
  7 + use shared::colliders::BakedCollider;
    |
help: if you import `BakedCollider`, refer to it directly
    |
347 -                         shared::BakedCollider::ConvexHull { points: hull },
347 +                         BakedCollider::ConvexHull { points: hull },
    |

error[E0433]: failed to resolve: could not find `BakedCollider` in `shared`
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:364:29
    |
364 |                     shared::BakedCollider::CompoundConvex { hulls },
    |                             ^^^^^^^^^^^^^ could not find `BakedCollider` in `shared`
    |
help: consider importing this enum
    |
  7 + use shared::colliders::BakedCollider;
    |
help: if you import `BakedCollider`, refer to it directly
    |
364 -                     shared::BakedCollider::CompoundConvex { hulls },
364 +                     BakedCollider::CompoundConvex { hulls },
    |

error[E0422]: cannot find struct, variant or union type `BakedColliderDb` in crate `shared`
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:370:22
    |
370 |     let db = shared::BakedColliderDb {
    |                      ^^^^^^^^^^^^^^^ not found in `shared`
    |
help: consider importing this struct
    |
  7 + use shared::colliders::BakedColliderDb;
    |
help: if you import `BakedColliderDb`, refer to it directly
    |
370 -     let db = shared::BakedColliderDb {
370 +     let db = BakedColliderDb {
    |

error[E0282]: type annotations needed
   --> tools/collider_baker/src/main.rs:133:28
    |
133 |         prop_lookup.insert(k.id().to_string(), k);
    |                            ^ cannot infer type

error[E0282]: type annotations needed
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:135:28
    |
135 |         prop_lookup.insert(k.id().to_string(), k);
    |                            ^ cannot infer type

error[E0282]: type annotations needed
   --> tools/collider_baker/src/main.rs:139:32
    |
139 |         building_lookup.insert(b.id().to_string(), b);
    |                                ^ cannot infer type

error[E0282]: type annotations needed
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:141:32
    |
141 |         building_lookup.insert(b.id().to_string(), b);
    |                                ^ cannot infer type

error[E0282]: type annotations needed
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:159:13
    |
159 |             bt.scene_path().map(|s| s.to_string())
    |             ^^ cannot infer type

error[E0282]: type annotations needed
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:159:34
    |
159 |             bt.scene_path().map(|s| s.to_string())
    |                                  ^  - type must be known at this point
    |
help: consider giving this closure parameter an explicit type
    |
159 |             bt.scene_path().map(|s: /* Type */| s.to_string())
    |                                   ++++++++++++

error[E0282]: type annotations needed
   --> tools/collider_baker/src/main.rs:157:13
    |
157 |             bt.scene_path().map(|s| s.to_string())
    |             ^^ cannot infer type

error[E0282]: type annotations needed
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:168:18
    |
168 |             Some(pk.scene_path().to_string())
    |                  ^^ cannot infer type

error[E0282]: type annotations needed
   --> tools/collider_baker/src/main.rs:157:34
    |
157 |             bt.scene_path().map(|s| s.to_string())
    |                                  ^  - type must be known at this point
    |
help: consider giving this closure parameter an explicit type
    |
157 |             bt.scene_path().map(|s: /* Type */| s.to_string())
    |                                   ++++++++++++

error[E0282]: type annotations needed
   --> tools/collider_baker/src/main.rs:166:18
    |
166 |             Some(pk.scene_path().to_string())
    |                  ^^ cannot infer type

Some errors have detailed explanations: E0282, E0412, E0422, E0425, E0433.
For more information about an error, try `rustc --explain E0282`.
error: could not compile `collider_baker` (bin "collider_baker" test) due to 12 previous errors
warning: build failed, waiting for other jobs to finish...
error: could not compile `collider_baker` (bin "collider_baker_v2" test) due to 14 previous errors

## cargo clippy --workspace --all-targets --no-deps -- -D warnings
    Checking collider_baker v0.1.0 (/Users/terninator/coding/citysim/tools/collider_baker)
error[E0412]: cannot find type `PropKind` in crate `shared`
   --> tools/collider_baker/src/main.rs:131:50
    |
131 |     let mut prop_lookup: HashMap<String, shared::PropKind> = HashMap::new();
    |                                                  ^^^^^^^^ not found in `shared`
    |
help: consider importing this enum
    |
  7 + use shared::props::PropKind;
    |
help: if you import `PropKind`, refer to it directly
    |
131 -     let mut prop_lookup: HashMap<String, shared::PropKind> = HashMap::new();
131 +     let mut prop_lookup: HashMap<String, PropKind> = HashMap::new();
    |

error[E0425]: cannot find value `ALL_PROP_KINDS` in crate `shared`
   --> tools/collider_baker/src/main.rs:132:22
    |
132 |     for k in shared::ALL_PROP_KINDS.iter().copied() {
    |                      ^^^^^^^^^^^^^^ not found in `shared`
    |
help: consider importing this constant
    |
  7 + use shared::props::ALL_PROP_KINDS;
    |
help: if you import `ALL_PROP_KINDS`, refer to it directly
    |
132 -     for k in shared::ALL_PROP_KINDS.iter().copied() {
132 +     for k in ALL_PROP_KINDS.iter().copied() {
    |

error[E0412]: cannot find type `BuildingType` in crate `shared`
   --> tools/collider_baker/src/main.rs:137:54
    |
137 |     let mut building_lookup: HashMap<String, shared::BuildingType> = HashMap::new();
    |                                                      ^^^^^^^^^^^^ not found in `shared`
    |
help: consider importing this enum
    |
  7 + use shared::prelude::BuildingType;
    |
help: if you import `BuildingType`, refer to it directly
    |
137 -     let mut building_lookup: HashMap<String, shared::BuildingType> = HashMap::new();
137 +     let mut building_lookup: HashMap<String, BuildingType> = HashMap::new();
    |

error[E0425]: cannot find value `ALL_BUILDING_TYPES` in crate `shared`
   --> tools/collider_baker/src/main.rs:138:22
    |
138 |     for b in shared::ALL_BUILDING_TYPES.iter().copied() {
    |                      ^^^^^^^^^^^^^^^^^^ not found in `shared`
    |
help: consider importing this constant
    |
  7 + use shared::building::ALL_BUILDING_TYPES;
    |
help: if you import `ALL_BUILDING_TYPES`, refer to it directly
    |
138 -     for b in shared::ALL_BUILDING_TYPES.iter().copied() {
138 +     for b in ALL_BUILDING_TYPES.iter().copied() {
    |

error[E0412]: cannot find type `BakedCollider` in crate `shared`
   --> tools/collider_baker/src/main.rs:229:50
    |
229 |     let mut out_entries: HashMap<String, shared::BakedCollider> = HashMap::new();
    |                                                  ^^^^^^^^^^^^^ not found in `shared`
    |
help: consider importing this enum
    |
  7 + use shared::colliders::BakedCollider;
    |
help: if you import `BakedCollider`, refer to it directly
    |
229 -     let mut out_entries: HashMap<String, shared::BakedCollider> = HashMap::new();
229 +     let mut out_entries: HashMap<String, BakedCollider> = HashMap::new();
    |

error[E0433]: failed to resolve: could not find `BakedCollider` in `shared`
   --> tools/collider_baker/src/main.rs:304:21
    |
304 |             shared::BakedCollider::ConvexHull { points: hull },
    |                     ^^^^^^^^^^^^^ could not find `BakedCollider` in `shared`
    |
help: consider importing this enum
    |
  7 + use shared::colliders::BakedCollider;
    |
help: if you import `BakedCollider`, refer to it directly
    |
304 -             shared::BakedCollider::ConvexHull { points: hull },
304 +             BakedCollider::ConvexHull { points: hull },
    |

error[E0422]: cannot find struct, variant or union type `BakedColliderDb` in crate `shared`
   --> tools/collider_baker/src/main.rs:308:22
    |
308 |     let db = shared::BakedColliderDb {
    |                      ^^^^^^^^^^^^^^^ not found in `shared`
    |
help: consider importing this struct
    |
  7 + use shared::colliders::BakedColliderDb;
    |
help: if you import `BakedColliderDb`, refer to it directly
    |
308 -     let db = shared::BakedColliderDb {
308 +     let db = BakedColliderDb {
    |

error[E0412]: cannot find type `PropKind` in crate `shared`
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:133:50
    |
133 |     let mut prop_lookup: HashMap<String, shared::PropKind> = HashMap::new();
    |                                                  ^^^^^^^^ not found in `shared`
    |
help: consider importing this enum
    |
  7 + use shared::props::PropKind;
    |
help: if you import `PropKind`, refer to it directly
    |
133 -     let mut prop_lookup: HashMap<String, shared::PropKind> = HashMap::new();
133 +     let mut prop_lookup: HashMap<String, PropKind> = HashMap::new();
    |

error[E0425]: cannot find value `ALL_PROP_KINDS` in crate `shared`
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:134:22
    |
134 |     for k in shared::ALL_PROP_KINDS.iter().copied() {
    |                      ^^^^^^^^^^^^^^ not found in `shared`
    |
help: consider importing this constant
    |
  7 + use shared::props::ALL_PROP_KINDS;
    |
help: if you import `ALL_PROP_KINDS`, refer to it directly
    |
134 -     for k in shared::ALL_PROP_KINDS.iter().copied() {
134 +     for k in ALL_PROP_KINDS.iter().copied() {
    |

error[E0412]: cannot find type `BuildingType` in crate `shared`
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:139:54
    |
139 |     let mut building_lookup: HashMap<String, shared::BuildingType> = HashMap::new();
    |                                                      ^^^^^^^^^^^^ not found in `shared`
    |
help: consider importing this enum
    |
  7 + use shared::prelude::BuildingType;
    |
help: if you import `BuildingType`, refer to it directly
    |
139 -     let mut building_lookup: HashMap<String, shared::BuildingType> = HashMap::new();
139 +     let mut building_lookup: HashMap<String, BuildingType> = HashMap::new();
    |

error[E0425]: cannot find value `ALL_BUILDING_TYPES` in crate `shared`
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:140:22
    |
140 |     for b in shared::ALL_BUILDING_TYPES.iter().copied() {
    |                      ^^^^^^^^^^^^^^^^^^ not found in `shared`
    |
help: consider importing this constant
    |
  7 + use shared::building::ALL_BUILDING_TYPES;
    |
help: if you import `ALL_BUILDING_TYPES`, refer to it directly
    |
140 -     for b in shared::ALL_BUILDING_TYPES.iter().copied() {
140 +     for b in ALL_BUILDING_TYPES.iter().copied() {
    |

error[E0412]: cannot find type `BakedCollider` in crate `shared`
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:231:50
    |
231 |     let mut out_entries: HashMap<String, shared::BakedCollider> = HashMap::new();
    |                                                  ^^^^^^^^^^^^^ not found in `shared`
    |
help: consider importing this enum
    |
  7 + use shared::colliders::BakedCollider;
    |
help: if you import `BakedCollider`, refer to it directly
    |
231 -     let mut out_entries: HashMap<String, shared::BakedCollider> = HashMap::new();
231 +     let mut out_entries: HashMap<String, BakedCollider> = HashMap::new();
    |

error[E0433]: failed to resolve: could not find `BakedCollider` in `shared`
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:302:29
    |
302 |                     shared::BakedCollider::ConvexHull { points: hull },
    |                             ^^^^^^^^^^^^^ could not find `BakedCollider` in `shared`
    |
help: consider importing this enum
    |
  7 + use shared::colliders::BakedCollider;
    |
help: if you import `BakedCollider`, refer to it directly
    |
302 -                     shared::BakedCollider::ConvexHull { points: hull },
302 +                     BakedCollider::ConvexHull { points: hull },
    |

error[E0433]: failed to resolve: could not find `BakedCollider` in `shared`
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:347:33
    |
347 |                         shared::BakedCollider::ConvexHull { points: hull },
    |                                 ^^^^^^^^^^^^^ could not find `BakedCollider` in `shared`
    |
help: consider importing this enum
    |
  7 + use shared::colliders::BakedCollider;
    |
help: if you import `BakedCollider`, refer to it directly
    |
347 -                         shared::BakedCollider::ConvexHull { points: hull },
347 +                         BakedCollider::ConvexHull { points: hull },
    |

error[E0433]: failed to resolve: could not find `BakedCollider` in `shared`
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:364:29
    |
364 |                     shared::BakedCollider::CompoundConvex { hulls },
    |                             ^^^^^^^^^^^^^ could not find `BakedCollider` in `shared`
    |
help: consider importing this enum
    |
  7 + use shared::colliders::BakedCollider;
    |
help: if you import `BakedCollider`, refer to it directly
    |
364 -                     shared::BakedCollider::CompoundConvex { hulls },
364 +                     BakedCollider::CompoundConvex { hulls },
    |

error[E0422]: cannot find struct, variant or union type `BakedColliderDb` in crate `shared`
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:370:22
    |
370 |     let db = shared::BakedColliderDb {
    |                      ^^^^^^^^^^^^^^^ not found in `shared`
    |
help: consider importing this struct
    |
  7 + use shared::colliders::BakedColliderDb;
    |
help: if you import `BakedColliderDb`, refer to it directly
    |
370 -     let db = shared::BakedColliderDb {
370 +     let db = BakedColliderDb {
    |

error[E0282]: type annotations needed
   --> tools/collider_baker/src/main.rs:133:28
    |
133 |         prop_lookup.insert(k.id().to_string(), k);
    |                            ^ cannot infer type

error[E0282]: type annotations needed
   --> tools/collider_baker/src/main.rs:139:32
    |
139 |         building_lookup.insert(b.id().to_string(), b);
    |                                ^ cannot infer type

error[E0282]: type annotations needed
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:135:28
    |
135 |         prop_lookup.insert(k.id().to_string(), k);
    |                            ^ cannot infer type

error[E0282]: type annotations needed
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:141:32
    |
141 |         building_lookup.insert(b.id().to_string(), b);
    |                                ^ cannot infer type

error[E0282]: type annotations needed
   --> tools/collider_baker/src/main.rs:157:13
    |
157 |             bt.scene_path().map(|s| s.to_string())
    |             ^^ cannot infer type

error[E0282]: type annotations needed
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:159:13
    |
159 |             bt.scene_path().map(|s| s.to_string())
    |             ^^ cannot infer type

error[E0282]: type annotations needed
   --> tools/collider_baker/src/main.rs:157:34
    |
157 |             bt.scene_path().map(|s| s.to_string())
    |                                  ^  - type must be known at this point
    |
help: consider giving this closure parameter an explicit type
    |
157 |             bt.scene_path().map(|s: /* Type */| s.to_string())
    |                                   ++++++++++++

error[E0282]: type annotations needed
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:159:34
    |
159 |             bt.scene_path().map(|s| s.to_string())
    |                                  ^  - type must be known at this point
    |
help: consider giving this closure parameter an explicit type
    |
159 |             bt.scene_path().map(|s: /* Type */| s.to_string())
    |                                   ++++++++++++

error[E0282]: type annotations needed
   --> tools/collider_baker/src/bin/collider_baker_v2.rs:168:18
    |
168 |             Some(pk.scene_path().to_string())
    |                  ^^ cannot infer type

error[E0282]: type annotations needed
   --> tools/collider_baker/src/main.rs:166:18
    |
166 |             Some(pk.scene_path().to_string())
    |                  ^^ cannot infer type

Some errors have detailed explanations: E0282, E0412, E0422, E0425, E0433.
For more information about an error, try `rustc --explain E0282`.
error: could not compile `collider_baker` (bin "collider_baker") due to 12 previous errors
warning: build failed, waiting for other jobs to finish...
error: could not compile `collider_baker` (bin "collider_baker" test) due to 12 previous errors
error: could not compile `collider_baker` (bin "collider_baker_v2" test) due to 14 previous errors
error: could not compile `collider_baker` (bin "collider_baker_v2") due to 14 previous errors
