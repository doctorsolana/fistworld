# Catapult asset

Rebuild from the repository root with a separate headless Blender process:

```sh
/Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup --python asset_creation/siege/build_catapult.py
```

The script writes `client/assets/game_assets/vehicles/Catapult.glb`. It creates its own
geometry and materials; no purchased model, texture or external input is required.
Coordinates in the source are metres, +Y up and -Z forward. Export converts to Blender
coordinates and back to glTF. The arm pivot and stone socket agree with
`shared/src/components/siege.rs`; named arm, wheel and winch nodes are animated by the client.

Current geometry: **2,048 authored vertices / 3,524 triangles**. Flat normals and material
boundaries produce **6,144 exported render vertices**, across seven meshes and 23 material
primitives. The GLB is approximately 172 KiB. Instances share meshes and materials.
