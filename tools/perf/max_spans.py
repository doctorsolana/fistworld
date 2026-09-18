"""Longest INDIVIDUAL span per system in a chrome trace (finds the stall).

ms/s averages hide a one-off 487 ms tick. This streams the whole file, pairs
B/E events per thread, and reports the worst single call of each system.
"""
import json, sys, collections
path = sys.argv[1]
stack = collections.defaultdict(list)          # tid -> [(name, ts)]
worst = collections.defaultdict(float)         # name -> max duration us
calls = collections.Counter()
with open(path, errors="replace") as f:
    for line in f:
        line = line.strip().rstrip(",")
        if not line.startswith("{"):
            continue
        try:
            e = json.loads(line)
        except Exception:
            continue
        ph = e.get("ph")
        if ph == "B":
            stack[e.get("tid")].append((e.get("name", "?"), e.get("ts", 0)))
        elif ph == "E":
            st = stack.get(e.get("tid"))
            if st:
                name, ts = st.pop()
                d = e.get("ts", 0) - ts
                calls[name] += 1
                if d > worst[name]:
                    worst[name] = d
print(f"{'worst single call (ms)':>22}  {'calls':>9}  system")
for name, us in sorted(worst.items(), key=lambda kv: -kv[1])[:30]:
    print(f"{us/1000.0:22.1f}  {calls[name]:9d}  {name[:110]}")
