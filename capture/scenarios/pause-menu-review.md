# Escape and settings review

`pause-menu-tour.ron` follows production menu buttons through Graphics, Audio and
Controls, then Back, Resume and a keyboard Escape reopen. Only the initially open
Escape menu and offline world are fixtures. Scene scale, master volume, music
enablement and mouse sensitivity are changed and restored through their real input
handlers. Player settings files are isolated by the standard capture harness.

```sh
BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture \
  --scenario capture/scenarios/pause-menu-tour.ron \
  --out logs/pause-menu-review/normal
BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture \
  --scenario capture/scenarios/pause-menu-tour.ron --resolution 1280x720 \
  --out logs/pause-menu-review/small
```

Each PNG has the standard `.capture.json` plus `.menu.json`: actual active pane,
current settings, visible control and text bounds, enabled states, menu containment
and clip checks. The driver waits for the requested production result, nonzero
visible layout and loaded artwork. Missing or clipped controls fail within the
scenario's frame budget. Hidden, retained duplicates cannot receive the tour's
button input. The Controls page rejects the removed Find your hero and Close menu
reference entries. Inspect the PNGs as well as both sidecars before accepting a
visual change; geometry checks do not prove good typography or material appearance.

This is an offline appearance and local-input review, not native pointer hit
testing, native fullscreen/resolution confirmation or audio playback/mix evidence.
Native display transitions remain covered by the existing display capture driver.
The audio samples verify settings changes only. Settings changes do not establish
that an effect or music track was heard at the expected loudness.
