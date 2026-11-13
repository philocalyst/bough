"""Loader for flattened gui-bench runs (metrics.tsv, validity.tsv, cells.tsv from scripts/flatten.py).

A value has one of three states: 'ok' (the harness judged its report valid), 'inv' (the report failed a validity
check of the harness) or 'miss' (no measurement: the cell did not run that probe, or errored)."""
import csv, collections, os

def _rows(path):
    with open(path) as f:
        return list(csv.DictReader(f, delimiter="\t"))

class Run:
    def __init__(self, d, name=None):
        self.dir = d
        self.name = name or os.path.basename(d.rstrip("/"))
        self.M = {}
        self.valid = {}          # (cell key, report) -> bool
        self.failed = collections.defaultdict(list)   # (cell key, report) -> [check names]
        self.cells = {}
        self.fws = []
        for c in _rows(os.path.join(d, "cells.tsv")):
            k = self.ckey(c)
            self.cells[k] = c
            if c["framework"] not in self.fws:
                self.fws.append(c["framework"])
        for v in _rows(os.path.join(d, "validity.tsv")):
            k = (self.ckey(v), v["report"])
            if v["check"] == "":  # UNREADABLE rows
                self.valid[k] = False
                continue
            self.valid[k] = v["valid"] == "True"
            if v["status"] == "fail":
                self.failed[k].append(v["check"])
        for m in _rows(os.path.join(d, "metrics.tsv")):
            if m["mvariant"] not in ("None", ""):
                continue
            try:
                val = float(m["median"])
            except ValueError:
                continue
            key = (self.ckey(m), m["report"], m["section"], m["metric"])
            self.M[key] = val

    @staticmethod
    def ckey(r):
        hz = r.get("hz") or "60.0"
        try:
            hz = str(int(float(hz)))
        except ValueError:
            pass
        return (r["scenario"], r["framework"], r["variant"], str(r["rows"]), str(r["updates"]), hz)

    def get(self, fw, scenario, report, section, metric, rows, variant="default", updates="None", hz="60"):
        k = (scenario, fw, variant, str(rows), str(updates), str(hz))
        v = self.M.get((k, report, section, metric))
        if v is None:
            return None, "miss"
        ok = self.valid.get((k, report), True)
        return v, ("ok" if ok else "inv")

    def cell_errors(self, fw, scenario, rows, variant="default", updates="None", hz="60"):
        c = self.cells.get((scenario, fw, variant, str(rows), str(updates), str(hz)))
        return None if c is None else c["errors"]

def fmt(v, st, nd=2, scale=1.0):
    if st == "miss" or v is None:
        return "-"
    s = f"{v * scale:.{nd}f}"
    return s + (" †" if st == "inv" else "")
