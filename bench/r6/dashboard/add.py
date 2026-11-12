#!/usr/bin/env python3
"""Append a point to a series in data.json (or create a series), then rebuild the page.

  add.py <series-id> <label> <value> [--kind measured|reported|estimated] [--note "..."] [--source "..."]
  add.py <series-id> <label> <value> --new "group|title|unit|lower_is_better" [--kind ...] [--note ...]
"""

import argparse
import datetime
import json
import math
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
DATA_PATH = os.path.join(HERE, "data.json")
BUILD_PATH = os.path.join(HERE, "build.py")


def die(msg):
    print(f"error: {msg}", file=sys.stderr)
    sys.exit(1)


def parse_value(text):
    try:
        v = float(text)
    except ValueError:
        die(f"value {text!r} is not numeric")
    if not math.isfinite(v):
        die(f"value {text!r} is not a finite number")
    try:
        return int(text)
    except ValueError:
        return v


def parse_new(spec):
    parts = spec.split("|")
    if len(parts) != 4 or not all(p.strip() for p in parts[:3]):
        die('--new expects "group|title|unit|lower_is_better"')
    flag = parts[3].strip().lower()
    if flag not in ("true", "false"):
        die("lower_is_better must be true or false")
    return {
        "group": parts[0].strip(),
        "title": parts[1].strip(),
        "unit": parts[2].strip(),
        "lower_is_better": flag == "true",
    }


def main(argv=None):
    ap = argparse.ArgumentParser(description="Add a result point to data.json and rebuild the page.")
    ap.add_argument("series_id")
    ap.add_argument("label")
    ap.add_argument("value")
    ap.add_argument("--kind", choices=["measured", "reported", "estimated"], default="reported",
                    help="default: reported (an unreplayed report)")
    ap.add_argument("--source", default=None, help="where the number came from, e.g. 'lane/store/round6'")
    ap.add_argument("--note", default=None, help="note shown under this point")
    ap.add_argument("--new", metavar="GROUP|TITLE|UNIT|LOWER", default=None,
                    help="create the series with this id before adding the point")
    args = ap.parse_args(argv)

    value = parse_value(args.value)
    new_fields = parse_new(args.new) if args.new else None

    with open(DATA_PATH, encoding="utf-8") as f:
        data = json.load(f)

    series = next((s for s in data["series"] if s["id"] == args.series_id), None)
    if new_fields is not None:
        if series is not None:
            die(f"series {args.series_id!r} already exists; drop --new to add a point to it")
        series = {"id": args.series_id, **new_fields, "note": "", "points": []}
        data["series"].append(series)
        action = "Created series"
    else:
        if series is None:
            die(f"no series with id {args.series_id!r}; use --new to create it")
        action = "Added to series"

    point = {"label": args.label, "value": value, "kind": args.kind}
    if args.source:
        point["source"] = args.source
    if args.note:
        point["note"] = args.note
    series["points"].append(point)
    data["updated"] = datetime.date.today().isoformat()

    with open(DATA_PATH, "w", encoding="utf-8") as f:
        json.dump(data, f, indent=2, ensure_ascii=False)
        f.write("\n")

    print(f"{action} {series['id']!r} ({series['group']} / {series['title']})")
    print(f"  + point {args.label!r} = {value} ({args.kind})" + (f", source: {args.source}" if args.source else "")
          + (f", note: {args.note}" if args.note else ""))
    print(f"  updated -> {data['updated']}")

    result = subprocess.run([sys.executable, BUILD_PATH])
    if result.returncode != 0:
        die("build.py failed; data.json was updated but the page was not rebuilt")


if __name__ == "__main__":
    main()
