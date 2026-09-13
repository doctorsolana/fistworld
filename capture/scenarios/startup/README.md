# FistWorld startup views

These scenarios photograph the production launcher, join form, name rejection,
connection notice, name submission notice and world-preparation modal. They stage
**offline presentation inputs**, not a successful connection or measured world
progress. The capture-only `CaptureConfig` resource suppresses connection startup;
normal game sessions keep the real network path.

```sh
BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture \
  --scenario capture/scenarios/startup/launcher.ron \
  --resolution 1600x900 --out logs/startup-review/implementation/normal/launcher
BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture \
  --scenario capture/scenarios/startup/join-game.ron \
  --resolution 1280x720 --out logs/startup-review/implementation/small/join-game
BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture \
  --scenario capture/scenarios/startup/loading-motion.ron \
  --resolution 1024x768 --out logs/startup-review/implementation/narrow/loading-motion
```

Run all six view scenarios at ordinary, small and narrow sizes when editing
layout. The launcher first records the untouched menu, then hovers Connect,
opens its two fixture server presets through the production toggle and closes
it again. The hover waits for the actual label spring while asserting a fixed
hit area; popup readiness requires production dropdown state, visible bounds and actual
stack order above the Connect control.
Presets are explicitly staged (Local server and Development server), and no
connection is made. `loading-motion.ron` runs 181 uninterrupted frames at 60 Hz with one
composed PNG probe every 15 frames. No terrain is required, and no arbitrary sleep
is used to infer readiness: the harness inspects real visible control geometry,
fonts and image dependencies.

Inspect each PNG together with `.capture.json` and `.startup.json`. The latter
records the capture harness diagnostic title, text/colors, panel/control bounds
and loaded artwork. Name views also require opaque wood backing and real
header/parchment overlap beneath the torn edge. Native game window branding is
verified separately.
`loading-animation.json` records every sampled diamond fill, orientation and
position, requires all three to visibly pulse in sequence, and checks that their
group stays in place. The motion evidence measures native UI animation; it is not
a server, generation-time or frame-rate benchmark.
