# Card template

Copy into `docs/lanes/<lane>/cards/<yyyy-mm-dd>-<slug>.md` and freeze it (commit) **before** building or measuring.
Edits after measurement start are recorded as amendments, with the reason; the original stays.

```
lane:            <slug>
state:           CARD
owner:           <one session or person; one experiment, one owner>
predecessor:     <commit> (toolchain, features, profile)
question:        <one sentence>
change:          <the representation or mechanism, and what it replaces>
implication:     <what passing would establish, exactly; and what it would not>
platforms:       <OS / backend / hardware class>
workloads:       <scenes, row counts, inputs; pinned competitor commits if comparing>
gates:           <pixel/scene/semantic gates; the negative control that must fail>
thresholds:      <per cell, e.g. every STATUS cell ≤ 1.05x; win target ≥ X on Y>
measurement:     <instruments (instructions, ABAB GPU pairs, AWAGY run.py), repetitions, idle-machine rule>
budget:          <time / compute; remote hosts only with an explicit allocation>
stop conditions: <when to stop and record REJECTED / METHOD_BOUNDARY>
do not revisit:  <settled items this card must not reopen (docs/SETTLED.md)>
```

The outcome goes in the same lane, as a report next to the card: the state reached, every cell, failures and
timeouts, and what the next card should be.
