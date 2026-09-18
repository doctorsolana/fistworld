#!/usr/bin/env python3
"""Cross-thread tail aggregation: top spans, top children per schedule.

Usage: trace_tail2.py <trace.json> [tail_seconds]
Values are microseconds per wall second; /1000 -> ms/s; /fps -> ms/frame.
"""
import json
import sys
from collections import defaultdict

path = sys.argv[1]
tail_s = float(sys.argv[2]) if len(sys.argv) > 2 else 30.0

decoder = json.JSONDecoder()
buf = ""
pos = 0
# key -> bucket -> [incl_us, count, self_us]
agg = defaultdict(lambda: defaultdict(lambda: [0.0, 0, 0.0]))
stacks = defaultdict(list)  # tid -> [[name, bucket, begin_ts, child_us, parent]]
thread_names = {}
first_ts = None
last_ts = 0.0
n = 0


def process(ev):
    global first_ts, last_ts, n
    ph = ev.get("ph")
    tid = ev.get("tid")
    name = ev.get("name", "?")
    ts = float(ev.get("ts", 0.0))
    if ph == "M" and name == "thread_name":
        thread_names[tid] = ev.get("args", {}).get("name", "?")
        return
    if first_ts is None:
        first_ts = ts
    last_ts = max(last_ts, ts)
    st = stacks[tid]
    if ph == "B":
        parent = st[-1][0] if st else "ROOT"
        st.append([name, int(ts // 1e6), ts, 0.0, parent])
        n += 1
    elif ph == "E":
        if not st:
            return
        nm, bucket, bts, child, parent = st.pop()
        dur = max(0.0, ts - bts)
        e = agg[(parent, nm)][bucket]
        e[0] += dur
        e[1] += 1
        e[2] += dur - child
        if st:
            st[-1][3] += dur


def parse(text):
    global buf, pos
    buf += text
    ln = len(buf)
    while True:
        while pos < ln and buf[pos] != "{":
            pos += 1
        if pos >= ln:
            buf = ""
            pos = 0
            return
        try:
            obj, end = decoder.raw_decode(buf, pos)
        except ValueError:
            buf = buf[pos:]
            pos = 0
            return
        pos = end
        if pos > (1 << 20):
            buf = buf[pos:]
            ln = len(buf)
            pos = 0
        process(obj)


with open(path, "r", errors="replace") as f:
    while True:
        chunk = f.read(1 << 22)
        if not chunk:
            break
        parse(chunk)
parse("\n]\n")

last_bucket = int(last_ts // 1e6)
lo = last_bucket - int(tail_s)
print(f"events={n} duration_s={(last_ts - (first_ts or 0)) / 1e6:.1f} tail=[{lo},{last_bucket}]")

# fps in the tail: count of "update:" spans on any thread.
fps = 0.0
for (parent, name), buckets in agg.items():
    if name == "update:":
        fps += sum(v[1] for b, v in buckets.items() if lo <= b <= last_bucket) / tail_s
print(f"fps_tail={fps:.1f}  (ms/frame = ms/s / fps)")


def totals_for(parent_filter=None, name_filter=None, limit=30):
    rows = []
    for (parent, name), buckets in agg.items():
        if parent_filter is not None and parent_filter not in parent:
            continue
        if name_filter is not None and name_filter not in name:
            continue
        inc = cnt = slf = 0.0
        for b, v in buckets.items():
            if lo <= b <= last_bucket:
                inc += v[0]
                cnt += v[1]
                slf += v[2]
        if cnt >= 5:
            rows.append((inc / 1000.0 / tail_s, slf / 1000.0 / tail_s, cnt / tail_s, name))
    rows.sort(reverse=True)
    return rows[:limit]


def show(title, rows, fps):
    print(f"\n=== {title} (ms/s | self | /frame | count/s) ===")
    for inc, slf, cnt, name in rows:
        print(f"{inc:9.2f} {slf:9.2f} {(inc / fps if fps else 0):7.2f} {cnt:7.1f}  {name[:104]}")


show("top spans overall (all threads)", totals_for(limit=35), fps)
show("children of PostUpdate", totals_for(parent_filter="name=PostUpdate", limit=30), fps)
show("children of Update", totals_for(parent_filter="name=Update", limit=30), fps)
show("children of PreUpdate", totals_for(parent_filter="name=PreUpdate", limit=20), fps)
show("children of animate_targets", totals_for(parent_filter="animate_targets", limit=12), fps)
show("children of Render schedule", totals_for(parent_filter="name=Render", limit=30), fps)
show("children of ExtractSchedule", totals_for(parent_filter="ExtractSchedule", limit=20), fps)

# Merged by span name across all threads, plus per-thread self sums.
merged = defaultdict(lambda: [0.0, 0.0, 0.0])
thread_self = defaultdict(float)
for (parent, name), buckets in agg.items():
    for b, v in buckets.items():
        if lo <= b <= last_bucket:
            merged[name][0] += v[0]
            merged[name][1] += v[2]
            merged[name][2] += v[1]
            thread_self[parent] += v[2] * 0  # placeholder
rows = sorted(merged.items(), key=lambda kv: -kv[1][0])[:45]
print("\n=== merged by span name (all threads): ms/s | self | count/s ===")
for name, (inc, slf, cnt) in rows:
    print(f"{inc/1000/tail_s:9.2f} {slf/1000/tail_s:9.2f} {cnt/tail_s:8.1f}  {name[:110]}")
