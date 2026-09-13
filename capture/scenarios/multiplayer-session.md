# Connected two-player scenario

Run `python3 capture/multiplayer_session.py --out logs/multiplayer-session/<fresh-name>`
after building the matching client and server. This is a connected scenario,
so the offline capture binary's RON fixture loader does not execute it.

The maintained executable recipe lives in `capture/multiplayer_session.py`:

- Map `village_lab`, immutable terrain seed 3, secure Village Lab, eight founders,
  no scripted second-day wave, natural immigration enabled, real time at 1x.
- Two isolated 1024×576 real clients use the production launcher, creator, Home,
  terrain right-click orders and pause-menu Disconnect. Developer access is off.
- Account `PeacefulA` and a deterministically searched different ordinary account
  share the current preferred-coast hash signature. Both starters must be
  observed at once, with initial hull centres between 6m and 35m apart.
- A stationary initial view of both boats comes first. B begins an ordinary
  outward sail, then disconnects while moving. A observes the abandoned route
  stop and the hull remain parked for four seconds. B rejoins with the same
  PersonId, possessions and aboard hero within 1.5m of its boat centre. The
  retained route must resume at least 1m without receiving a new movement order.
- Camera then frames the
  actual shore probe and origin at zoom 110. Twenty half-second-spaced real
  capture frames follow ordinary landing orders. At least 2m of motion and
  actual removal of both heroes' `AboardBoat` markers are required.
- Both plain move orders bring the landed heroes within 2.5m. A twelve-frame
  one-second-spaced sequence plus a twenty-second observation verifies that
  neither hero engages, readies a weapon, swings or loses health.
- Player B disconnects through the pause menu and rejoins the same account
  twice, including a case change. Player A remains connected under the same
  peer and PersonId and moves at least 2m in each fourteen-frame sequence.
  Rejoining restores B's original PersonId, account, health, cargo and wallet
  without a creator or opening cinematic. A final fresh movement order must
  also complete on A.

All actors, boats and network sessions are real. The runner may change camera
focus and resolution; it may not replace authoritative state to manufacture
readiness. Every wait has a bound and a semantic predicate. The continuous
sampler monitors the survivor even when B's command or capture is pending.

Review `report.json`, `samples.jsonl`, each PNG and its `.capture.json` and
`.session.json`. Entity strings are observer-local; compare stable PersonIds,
account names and explicit local-peer associations. Check motion frames in
order for hull admission, landing, nearby peaceful bodies and continuing
survivor motion. Baselines are not automatically approved. All run outputs
belong under ignored `logs/`.

The mandatory admission conflict is player/player. Natural NPC boat sightings
are recorded opportunistically and must not be described as a tested
player/NPC conflict without corresponding images and admission evidence.
General vessel collision avoidance while sailing remains outside this arrival
allocation regression; later sailing distances are observations, not claims of
safe ship-to-ship navigation.
