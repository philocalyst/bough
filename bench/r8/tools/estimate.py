"""estimate.py : estimates for cells with no measurement, from the framework's own measured cells.

Method (one for every estimate, stated once, quoted per value):
  * Only values the harness judged valid ("ok") are used. Invalid values are shown as they are and never feed a model.
  * Columns are grouped into classes of one kind of quantity (latency in ms, CPU per frame in ms, resident memory,
    startup, idle CPU, frame rate). Inside a class the logarithm of a value is modelled as  a[fw] + b[col]  and fitted
    by median polish (alternating row and column medians, 20 sweeps, outliers do not move medians).
  * a[fw] is that framework's own level in the class: the median over its own valid cells of  log(value) - b[col].
    It needs at least MIN_OWN (2) own cells in the class. With fewer, the framework's level over ALL classes is used
    and the estimate is flagged "cross-class" (low confidence).
  * estimate = exp(a[fw] + b[col]).  Range = estimate x exp(+-2 s), s = 1.4826 x MAD of this framework's own residuals
    in the class (at least 0.15, so a framework with two cells does not get a falsely tight range).
The estimate says "if this framework behaves, relative to the others, as it does in its own measured cells".
It is not a measurement, is never ranked with measurements and says nothing about a defect that only shows in the
missing cell."""
import math, statistics

MIN_OWN = 2
MIN_OWN_CLASS = {"frame rate (fps)": 1}
FLOOR = 0.15

def med(xs):
    return statistics.median(xs) if xs else None

def fit(vals):
    """vals: {(fw, col): value > 0}. Returns a {fw: level}, b {col: level} by median polish of log values."""
    L = {k: math.log(v) for k, v in vals.items() if v and v > 0}
    fws = sorted({k[0] for k in L}); cols = sorted({k[1] for k in L})
    a = {f: 0.0 for f in fws}; b = {c: 0.0 for c in cols}
    for _ in range(20):
        for c in cols:
            b[c] = med([L[(f, c)] - a[f] for f in fws if (f, c) in L])
        for f in fws:
            a[f] = med([L[(f, c)] - b[c] for c in cols if (f, c) in L])
    return a, b, L

def estimates(matrix, cls, only_cols=None):
    """matrix {col: {fw: (value, state)}}, cls {col: class}. Returns {col: {fw: dict}} for every 'miss' cell."""
    fws = sorted({f for c in matrix.values() for f in c})
    classes = sorted(set(cls[c] for c in matrix if c in cls))
    fitted = {}
    for k in classes:
        vals = {(f, c): v for c in matrix if cls.get(c) == k for f, (v, s) in matrix[c].items() if (s == "ok" or (s == "unk" and k == "startup (ms)")) and v}
        fitted[k] = fit(vals)
    # level over all classes: median of own residuals against each class's column effect
    glevel, gspread = {}, {}
    for f in fws:
        r = []
        for k, (a, b, L) in fitted.items():
            for (ff, c), l in L.items():
                if ff == f:
                    r.append(l - b[c])
        glevel[f] = med(r); gspread[f] = (1.4826 * med([abs(x - glevel[f]) for x in r]) if len(r) > 1 else None)
    out = {}
    for c, row in matrix.items():
        k = cls.get(c)
        if k is None or (only_cols and c not in only_cols):
            continue
        a, b, L = fitted[k]
        if c not in b:
            continue  # nobody measured this column validly: nothing to anchor on
        for f in fws:
            v, s = row.get(f, (None, "miss"))
            if s != "miss":
                continue
            own = [(cc, l - b[cc]) for (ff, cc), l in L.items() if ff == f]
            if len(own) >= MIN_OWN_CLASS.get(k, MIN_OWN):
                lvl = med([x for _, x in own]); resid = [abs(x - lvl) for _, x in own]
                sd = max(FLOOR, 1.4826 * med(resid)); how = "own"
                note = f"{len(own)} own valid cells in class '{k}' (log-level {lvl:+.2f} against the column medians); column level from {sum(1 for (ff,cc) in L if cc==c)} frameworks"
            elif glevel.get(f) is not None:
                lvl = glevel[f]; sd = max(FLOOR * 2, gspread[f] or 0.3); how = "cross-class"
                note = f"only {len(own)} own valid cell(s) in class '{k}': level taken across all classes (cross-class, low confidence); column level from {sum(1 for (ff,cc) in L if cc==c)} frameworks"
            else:
                continue
            est = math.exp(lvl + b[c])
            out.setdefault(c, {})[f] = {"v": est, "lo": est * math.exp(-2 * sd), "hi": est * math.exp(2 * sd), "how": how, "note": note, "class": k}
    return out


def sig2(v):
    """Two significant figures: the stated accuracy of an estimate is no better than that."""
    import math
    if not v: return v
    return round(v, 1 - int(math.floor(math.log10(abs(v)))))

def _score(errs, inside):
    import statistics
    ae = sorted(abs(e) for e in errs)
    return {"n": len(errs), "coverage": sum(inside) / len(inside) if inside else None,
            "median_factor": math.exp(statistics.median(ae)) if ae else None, "p90_factor": math.exp(ae[int(0.9 * (len(ae) - 1))]) if ae else None,
            "worst_factor": math.exp(ae[-1]) if ae else None, "within_2x": sum(1 for e in ae if e <= math.log(2)) / len(ae) if ae else None}

def validate(matrix, cls, keep_family=None):
    """Hold-out tests of the method on the valid cells it is fitted on.
    loo: hide one valid cell at a time and estimate it from the rest (what the method does for a failed cell of a framework that has others).
    family: for every framework with many valid cells keep only the cells of one scenario family (the spinner without rows, as floem has) and estimate all the
    others (what the method does for a framework whose cells mostly failed). Reports how often the stated range contained the true value and the size of the error."""
    errs, inside, worst = [], [], []
    for c in matrix:
        for fw, (v, s) in matrix[c].items():
            if s != "ok" or not v: continue
            m2 = {cc: dict(row) for cc, row in matrix.items()}
            m2[c][fw] = (None, "miss")
            e = estimates(m2, cls, only_cols={c}).get(c, {}).get(fw)
            if not e: continue
            er = math.log(e["v"] / v); errs.append(er); inside.append(e["lo"] <= v <= e["hi"]); worst.append((abs(er), c, fw, v, e["v"]))
    loo = _score(errs, inside); loo["worst"] = [{"column": c, "framework": fw, "measured": v, "estimated": ev} for _, c, fw, v, ev in sorted(worst, reverse=True)[:5]]
    ferr, fin, fw_worst = [], [], []
    if keep_family:
        for fw in sorted({f for c in matrix for f in matrix[c]}):
            own = [c for c in matrix if matrix[c].get(fw, (None, "miss"))[1] == "ok"]
            if len(own) < 12: continue
            m2 = {cc: dict(row) for cc, row in matrix.items()}
            for c in own:
                if c not in keep_family: m2[c][fw] = (None, "miss")
            E = estimates(m2, cls)
            for c in own:
                if c in keep_family: continue
                e = E.get(c, {}).get(fw)
                if not e: continue
                v = matrix[c][fw][0]
                er = math.log(e["v"] / v); ferr.append(er); fin.append(e["lo"] <= v <= e["hi"]); fw_worst.append((abs(er), c, fw, v, e["v"]))
    fam = _score(ferr, fin) if ferr else None
    if fam: fam["worst"] = [{"column": c, "framework": fw, "measured": v, "estimated": ev} for _, c, fw, v, ev in sorted(fw_worst, reverse=True)[:5]]
    return {"leave_one_out": loo, "floem_like": fam}
