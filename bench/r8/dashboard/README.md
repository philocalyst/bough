# bough round 6 dashboard

data.json is the single source of truth. build.py generates artifact.html (page content for the Artifact publisher)
and index.html (the same page in a full document, for opening locally). Both are self-contained: inline SVG, inline JS and CSS,
no external scripts or sources. Nothing is drawn that is not in data.json; missing values show as "not measured".

Add a point to an existing series (run `grep '"id"' data.json` to list ids):
  python3 add.py <series-id> "<label>" <value> [--kind measured|reported|estimated] [--source "..."] [--note "..."]
Create a new series with its first point:
  python3 add.py <series-id> "<label>" <value> --new "group|title|unit|true-or-false" [--kind ...] [--source "..."]
--kind defaults to reported. Non-numeric values are refused. Each add updates "updated" and reruns build.py.
Rebuild only: python3 build.py

Series fields: id, group, title, unit, lower_is_better, note, paired (optional; true or false overrides whether the
first point is a before value), points[] with label, value, kind, source (optional), note (optional).
Top-level not_measured[] lists sections with status "not measured yet" and an explanation; they render as empty states.

Charts: log-log scaling by N (gb6 families and table series), a heat grid of round6 / base by operation and N,
before/after dumbbells per group and unit (first and last point, ratio on the connector), a waterfall of the counter
footprint with the unattributed gap computed and marked as derived, a sparkline card per series, and the zortax section.
Reported points are hollow and dashed; measured points are filled; estimated points are dotted.
Log scale is used when max/min exceeds 50; linear scales start at zero.

publish.py is no longer needed: artifact.html is built directly. It still runs (it re-derives artifact.html from index.html), so run build.py instead.
