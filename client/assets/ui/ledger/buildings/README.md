# Canonical building illustrations

These small pictures show the actual shipped building GLBs rendered by Bevy. Each
kind/observed upgrade shares one 384 × 224 JPEG, encoded at quality 80 with 4:4:4
sampling and progressive delivery. They illustrate the model, not a particular
settlement's live surroundings or construction state.

Regenerate from the repository root after building `target/playtest/capture`:

```sh
python3 asset_creation/ui/build_ledger_thumbnails.py --capture
```

Use `--only hall hall-village hall-town` for a small subset, `--binary PATH` for a
previously built compatible capture executable, or `--recipes-only` to refresh the
checked-in RON recipes without rendering. Pillow is the only Python dependency.
The encoder refuses source artifacts with failed semantic assertions or incorrect
dimensions. Review every regenerated PNG and its `.capture.json` before committing
a changed runtime picture.

`capture/scenarios/ledger-buildings/` contains maintained recipes. The existing
asset fixture waits for the GLB scene and its dependencies and suppresses ordinary
vegetation. Its first shot anchors the authored object at the known `village_lab`
plot; a second free-look shot uses a fixed three-quarter angle and a camera fitted
to the actual transformed mesh vertices. This keeps tall roofs and windmill sails
inside the frame without changing the building's proportions. The 8.885553 m
fixture ground height belongs to that deterministic map/position.

Source PNGs, `.capture.json` files and run logs stay under ignored `logs/captures/`.
`sources.json` records the canonical model SHA-256, recipe, capture revision and
encoded byte count. No full capture screenshot or duplicated model is shipped.
Hall and house variants must be chosen from an observed `CivicHallLevel` or
`HouseAppearance`, never inferred from settlement tier alone.
