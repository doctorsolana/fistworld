# Fly.io playtest server

The repository ships a Dockerfile and `fly.toml` for one authoritative, always-on
playtest server in Fly.io's Amsterdam region. The public game protocol is UDP 5000;
it is not an HTTP service.

## Cost and lifecycle

- One `performance-1x` Machine with one dedicated performance vCPU and 2 GB RAM.
  This is the current playtest tier; it avoids the long shared-CPU scheduling stalls
  that made route searches and simulation ticks visibly hitch under village load.
- One dedicated IPv4 address. Fly requires a dedicated IPv4 address for public UDP
  and bills it separately.
- No volume. The world and player identities live for exactly one server process;
  a restart or deployment intentionally begins a clean world.
- Autostop is deliberately disabled. Stopping the Machine ends that live world.

Check current prices before leaving the server running long term:
<https://fly.io/docs/about/pricing/>.

## First deployment

```bash
brew install flyctl
fly auth login
fly apps list
fly config validate
fly deploy
fly ips list
fly ips allocate-v4        # only if the app has no dedicated IPv4 yet
fly status
fly logs
```

Fly's UDP proxy requires the process to bind `fly-global-services:5000`. The shared
protocol selects that address when Fly injects `FLY_APP_NAME`; local servers continue
to bind `0.0.0.0:5000`.

## Join from the game

In the main menu, select **Fly.io**. The checked-in preset currently points to
`169.155.61.28:5000`; alternatively, enter the dedicated IPv4 address shown by
`fly ips list` and leave the port at 5000. The server deliberately runs without
the unrestricted local `FISTWORLD_DEV` flag. To grant yourself God Mode without
granting every remote player, configure a hosted access key once:

```bash
fly secrets set FISTWORLD_GOD_KEY="<private-test-key>" --app fistforce
```

In Play mode, press **J**, enter that key in the **Server Admin** prompt and press
Enter. A successful challenge unlocks God Mode only for that network connection;
disconnecting locks it again. The server accepts at most five failed attempts per
connection, and currently accepts four-character keys for the early playtest period.
Raise that minimum before the hosted world gains durable state or untrusted testers.
Do not put the key in `fly.toml` or commit it to the repository.
The current test key and complete operator runbook belong only in the git-ignored
local file `docs/FLY-PRIVATE.md`.

After changing the dedicated IP, update `client/assets/servers.ron` so the **Fly.io**
preset remains accurate.

## Operate it

```bash
fly status
fly logs
fly machine list
fly machine restart <machine-id>  # intentionally resets the current world
fly machine stop <machine-id>     # pause compute billing; also loses the live world
fly machine start <machine-id>    # start a new world on the same Machine and IP
fly deploy                        # also starts a fresh world today
```

Natural immigration is enabled on ordinary hosted worlds. New people enter from a
real map-edge coast in one-use Dinghies, choose among existing settlements using
food, homes, jobs, prosperity, unrest, personal preference and a bounded distance
bias, sail to dry land and then walk through the ordinary Moot immigration queue.
The default base cadence is three immigrants per world day, modified by season
and settlement attractiveness. Natural arrivals pause when the total villager
population reaches 5,000 and resume after it falls below that ceiling. The checked-in
`fly.toml` sets both defaults explicitly. Operational overrides are:

```bash
fly secrets set FISTWORLD_NATURAL_IMMIGRATION=0      # disable it
fly secrets set FISTWORLD_IMMIGRANTS_PER_DAY=2
fly secrets set FISTWORLD_WORLD_NPC_CAP=3000
```

`FISTWORLD_IMMIGRATION_INTERVAL_DAYS` remains a compatibility fallback for an old
deployment, but the clearer arrivals-per-day setting takes precedence.

To stop compute charges while keeping the app and its IP allocation:

```bash
fly machine stop <machine-id>
```

The dedicated IPv4 remains billable while allocated. To remove that charge, release
it explicitly after confirming the address is no longer needed:

```bash
fly ips release <address>
```

Do not attach a volume until the server has one versioned world-state file containing
both society and account ownership. Persisting only player profiles into a reset world
would create orphaned companies, properties, jobs, and identities.
