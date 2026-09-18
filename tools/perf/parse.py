#!/usr/bin/env python3
"""Summarize a FISTFORCE_CLIENT_PERF client log for the perf audit."""
import re
import sys
import statistics

path = sys.argv[1]
lines = open(path, errors="replace").read().splitlines()

perf_re = re.compile(
    r"ClientPerf frame_ms_p50=([\d.]+) frame_ms_p95=([\d.]+) frame_ms_p99=([\d.]+) "
    r"hitch_count=(\d+) hitch_threshold_ms=([\d.]+)")
samples = []
for ln in lines:
    m = perf_re.search(ln)
    if m:
        samples.append(tuple(float(x) for x in m.groups()))

# Steady state: the last 8 samples (10 s cadence) cover the final 80 s at 1x.
steady = samples[-8:]
def col(i):
    return [s[i] for s in steady]

print(f"samples_total={len(samples)}")
if steady:
    print(f"steady_n={len(steady)}")
    print(f"p50 mean={statistics.mean(col(0)):.2f} median={statistics.median(col(0)):.2f} "
          f"min={min(col(0)):.2f} max={max(col(0)):.2f}")
    print(f"p95 mean={statistics.mean(col(1)):.2f} median={statistics.median(col(1)):.2f} "
          f"min={min(col(1)):.2f} max={max(col(1)):.2f}")
    print(f"p99 mean={statistics.mean(col(2)):.2f} median={statistics.median(col(2)):.2f}")
    print(f"steady_p50_series={' '.join(f'{s[0]:.1f}' for s in steady)}")
    print(f"steady_p95_series={' '.join(f'{s[1]:.1f}' for s in steady)}")
    print(f"hitch_sum={sum(int(s[3]) for s in steady)}")

# Census lines: last occurrence of each.
for tag in ("ClientPerfAssets", "ClientPerfMeshes", "ClientPerfWorld", "ClientPerfTerrain",
            "ClientPerfRigs", "PERF DROP snapshot"):
    hits = [ln for ln in lines if tag in ln]
    if hits:
        msg = hits[-1]
        msg = msg.split(tag, 1)[1] if tag in msg else msg
        print(f"last_{tag}:{msg.strip()[:600]}")

# Render diagnostics: keep the last occurrence per path.
rp = re.compile(r"(render/[A-Za-z0-9_/]+/elapsed_cpu):\s*([\d.]+)\s*ms")
last = {}
for ln in lines:
    m = rp.search(ln)
    if m:
        last[m.group(1)] = float(m.group(2))
if last:
    print("render_cpu_ms (last reported per pass):")
    for k, v in sorted(last.items(), key=lambda kv: -kv[1])[:16]:
        print(f"  {k} = {v:.3f}")

# Render target / window size.
for needle in ("Scene render target resized", "Window request"):
    hits = [ln for ln in lines if needle in ln]
    if hits:
        print(f"{needle.replace(' ', '_')}: {hits[-1].split(needle)[-1].strip()[:220]}")

# Frame time / fps diagnostics.
for diag in ("frame_time", "fps", "entity_count"):
    hits = re.findall(rf"^{diag}\s*:\s*([\d.]+)", "\n".join(lines), re.M)
    if hits:
        print(f"last_{diag}={hits[-1]}")

# Startup / readiness signals.
for needle in ("Name accepted", "Spawned client world visuals", "Exiting game",
               "AUTOSPAWN", "panic", "ERROR"):
    hits = [ln for ln in lines if needle in ln]
    print(f"count_{needle.replace(' ', '_')}={len(hits)}")
    if needle in ("AUTOSPAWN", "panic") and hits:
        print(f"  last: {hits[-1].strip()[:300]}")

# Validity: the hero-creator overlay must be closed for a gameplay measurement.
creator = [ln for ln in lines if "creator-preview" in ln]
world_lines = [ln for ln in lines if "ClientPerfWorld" in ln]
print(f"VALID_RUN={'yes' if not creator else 'NO'} (creator-preview lines={len(creator)})")
if creator:
    print(f"  first creator line: {creator[0].strip()[:300]}")
autocreate = [ln for ln in lines if "AUTOCREATE_VOYAGE" in ln]
print(f"autocreate_lines={len(autocreate)}")
