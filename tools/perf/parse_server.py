"""Summarise ServerPerf lines from a headless server log."""
import re, sys, statistics as st

PAT = re.compile(
    r"ServerPerf tick avg=([\d.]+)ms max=([\d.]+)ms over_20%=([\d.]+)% clock_delivery=([\d.]+)% "
    r"\| phases world=([\d.]+)/([\d.]+) core=([\d.]+)/([\d.]+) navigation=([\d.]+)/([\d.]+) ms"
)
VILL = re.compile(r"villagers=(\d+) idle=(\d+) migrating=(\d+) settled=(\d+) nav_pending=(\d+) nav_failed=(\d+)")

rows = []
for line in open(sys.argv[1], errors="replace"):
    m = PAT.search(line)
    if not m:
        continue
    v = VILL.search(line)
    rows.append(dict(
        tick_avg=float(m[1]), tick_max=float(m[2]), over=float(m[3]), clock=float(m[4]),
        world_avg=float(m[5]), world_max=float(m[6]),
        core_avg=float(m[7]), core_max=float(m[8]),
        nav_avg=float(m[9]), nav_max=float(m[10]),
        villagers=int(v[1]) if v else 0, nav_pending=int(v[5]) if v else 0,
        nav_failed=int(v[6]) if v else 0,
    ))

if not rows:
    print("NO ServerPerf SAMPLES"); sys.exit(0)

def col(k): return [r[k] for r in rows]
print(f"samples={len(rows)}  (one per 3 s)")
print(f"villagers {rows[0]['villagers']} -> {rows[-1]['villagers']} (peak {max(col('villagers'))})")
print(f"nav_pending peak={max(col('nav_pending'))} nav_failed last={rows[-1]['nav_failed']}")
print()
print("                 mean    p50    p95     max")
for k, name in [("tick_avg", "tick avg ms"), ("tick_max", "tick MAX ms"),
                ("world_avg", "world avg"), ("core_avg", "core avg"), ("nav_avg", "nav avg"),
                ("world_max", "world MAX"), ("core_max", "core MAX"), ("nav_max", "nav MAX")]:
    c = sorted(col(k))
    print(f"{name:<14} {st.mean(c):7.2f} {c[len(c)//2]:6.2f} {c[int(len(c)*.95)]:6.2f} {c[-1]:7.2f}")
print()
over = col("over"); clock = col("clock")
print(f"over-budget%% (tick > 20 ms): mean={st.mean(over):.1f} max={max(over):.1f}")
print(f"clock delivery%%: min={min(clock):.1f} mean={st.mean(clock):.1f}  (<100 = server falling behind)")
print(f"BUDGET: 60 Hz tick = 16.67 ms")
print()
print("tick_avg series:", " ".join(f"{r['tick_avg']:.1f}" for r in rows))
print("villager series:", " ".join(str(r["villagers"]) for r in rows))
