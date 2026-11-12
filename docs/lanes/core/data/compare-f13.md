# `f13` against `f3`

new / base per cell (values as in the matrix). **bold**: above 1.02x; `~`: above 1.02x but inside the cell's own scatter. Cells under 200 instructions
are counter-dominated: `+k` is the absolute difference in instructions.

## Structure, fresh tree

| shape / operation | 1e3 base | 1e3 new | 1e3 | 1e4 | 1e5 | 1e6 base | 1e6 new | 1e6 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ordered-wide / insert8-front | 8,424 | 8,423 | 1.000 | 0.999 | 1.000 | 475,040 | 475,041 | 1.000 |
| ordered-wide / remove8-front | 1,478 | 1,479 | 1.001 | 1.000 | 1.000 | 2,438 | 2,438 | 1.000 |
| ordered-wide / move8-front | 3,471 | 3,472 | 1.000 | 1.000 | 1.000 | 100,912 | 105,971 | 1.050~ |
| ordered-wide / insert8-middle | 7,975 | 7,975 | 1.000 | 0.999 | 0.998 | 288,813 | 288,813 | 1.000 |
| ordered-wide / remove8-middle | 1,053 | 1,053 | 1.000 | 1.000 | 1.000 | 1,764 | 1,764 | 1.000 |
| ordered-wide / move8-middle | 10,406 | 10,404 | 1.000 | 0.999 | 0.998 | 465,189 | 475,221 | 1.022~ |
| ordered-wide / insert8-end | 1,066 | 1,066 | 1.000 | 1.000 | 1.000 | 89,435 | 89,648 | 1.002 |
| ordered-wide / remove8-end | 771 | 771 | 1.000 | 1.000 | 1.000 | 800 | 800 | 1.000 |
| ordered-wide / move8-end | 10,122 | 10,122 | 1.000 | 0.999 | 1.000 | 464,335 | 489,206 | **1.054** |
| ordered-wide / insert64-front | 11,317 | 11,317 | 1.000 | 0.999 | 1.000 | 465,005 | 477,920 | 1.028~ |
| ordered-wide / remove64-front | 4,638 | 4,638 | 1.000 | 1.000 | 1.000 | 377,919 | 377,920 | 1.000 |
| ordered-wide / move64-front | 15,375 | 15,383 | 1.001 | 1.000 | 1.001 | 388,886 | 388,886 | 1.000 |
| ordered-wide / insert64-middle | 10,903 | 10,903 | 1.000 | 0.999 | 0.998 | 292,246 | 292,209 | 1.000 |
| ordered-wide / remove64-middle | 2,471 | 2,471 | 1.000 | 1.000 | 1.000 | 3,412 | 3,412 | 1.000 |
| ordered-wide / move64-middle | 18,913 | 18,883 | 0.998 | 1.000 | 0.999 | 486,634 | 460,996 | 0.947 |
| ordered-wide / insert64-end | 10,533 | 10,533 | 1.000 | 0.999 | 0.996 | 105,532 | 93,564 | 0.887 |
| ordered-wide / remove64-end | 1,941 | 1,941 | 1.000 | 1.000 | 1.000 | 1,066 | 1,066 | 1.000 |
| ordered-wide / move64-end | 13,656 | 13,622 | 0.998 | 1.000 | 1.001 | 384,352 | 384,205 | 1.000 |
| ordered-wide / append1 | 554 | 554 | 1.000 | 1.000 | 1.000 | 102,279 | 100,252 | 0.980 |
| ordered-wide / translate64 | 1,453 | 1,453 | 1.000 | 1.000 | 1.000 | 1,535 | 1,535 | 1.000 |
| ordered-wide / translate-half | 2,485 | 2,039 | 0.821 | 0.459 | 0.335 | 849,441 | 271,499 | 0.320 |
| ordered-wide / bulk-build | 98,808 | 98,775 | 1.000 | 1.000 | 1.000 | 86,406,710 | 86,054,883 | 0.996 |
| ordered-wide / build-append8 | 198,128 | 198,122 | 1.000 | 1.000 | 1.000 | 187,579,566 | 187,656,027 | 1.000 |
| ordered-wide / freeze-cold | 26,978 | 26,969 | 1.000 | 1.000 | 1.000 | 24,544,574 | 24,430,537 | 0.995 |
| ordered-wide / freeze-idle | 1,507 | 1,507 | 1.000 | 1.000 | 1.000 | 472,444 | 472,444 | 1.000 |
| ordered-wide / freeze-one | 1,805 | 1,805 | 1.000 | 1.000 | 1.000 | 472,742 | 472,742 | 1.000 |
| ordered-wide / freeze-all | 6,446 | 6,446 | 1.000 | 1.000 | 1.000 | 5,003,993 | 5,003,994 | 1.000 |
| ordered-wide / parent-last | 229 | 229 | 1.000 | 1.000 | 1.000 | 15,900 | 15,900 | 1.000 |
| ordered-wide / ancestors-last | 295 | 295 | 1.000 | 1.000 | 1.000 | 15,965 | 15,965 | 1.000 |
| ordered-wide / subtree-root | 205 | 205 | 1.000 | 1.000 | 1.000 | 17,812 | 17,812 | 1.000 |
| ordered-wide / subtree-leaf | 114 | 114 | 1.000 | 1.000 | 1.000 | 153 | 153 | 1.000 |
| ordered-chains32 / parent-last | 88 | 88 | 1.000 | 1.000 | 1.000 | 161 | 161 | 1.000 |
| ordered-chains32 / ancestors-last | 475 | 475 | 1.000 | 1.000 | 1.000 | 16,841 | 16,841 | 1.000 |
| ordered-chains32 / subtree-root | 205 | 205 | 1.000 | 1.000 | 1.000 | 17,812 | 17,812 | 1.000 |
| ordered-chains32 / subtree-leaf | 114 | 114 | 1.000 | 1.000 | 1.000 | 153 | 153 | 1.000 |
| ordered-wide / set-ink-pos | 371 | 371 | 1.000 | 1.000 | 1.000 | 452 | 452 | 1.000 |
| ordered-wide / set-ink-handle | 337 | 337 | 1.000 | 1.000 | 1.000 | 337 | 337 | 1.000 |
| ordered-wide / pos-of-handle | 24 | 24 | 1.000 | 1.000 | 1.000 | 24 | 24 | 1.000 |
| ordered-wide / touch-handle | 117 | 117 | 1.000 | 1.000 | 1.000 | 117 | 117 | 1.000 |
| ordered-wide / set-inks-handles64 | 18,664 | 18,664 | 1.000 | 1.000 | 1.000 | 35,713 | 35,713 | 1.000 |
| ordered-wide / insert8-front+pos64 | 10,581 | 10,581 | 1.000 | 1.000 | 1.000 | 474,630 | 459,428 | 0.968 |
| ordered-wide / insert8-front+posall | 9,086 | 9,086 | 1.000 | 0.998 | 1.000 | 872,736 | 885,277 | 1.014 |

## Structure, warm columns

| shape / operation | 1e3 base | 1e3 new | 1e3 | 1e4 | 1e5 | 1e6 base | 1e6 new | 1e6 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ordered-wide / insert8-front | 3,745 | 3,784 | 1.010 | 1.000 | 1.000 | 375,213 | 375,213 | 1.000 |
| ordered-wide / remove8-front | 1,479 | 1,479 | 1.000 | 1.000 | 1.000 | 2,438 | 2,438 | 1.000 |
| ordered-wide / move8-front | 3,438 | 3,472 | 1.010 | 1.000 | 1.000 | 6,167 | 6,167 | 1.000 |
| ordered-wide / insert8-middle | 3,299 | 3,335 | 1.011 | 1.000 | 1.000 | 188,988 | 188,988 | 1.000 |
| ordered-wide / remove8-middle | 1,017 | 1,053 | 1.035~ | 1.000 | 1.000 | 1,764 | 1,764 | 1.000 |
| ordered-wide / move8-middle | 5,730 | 5,764 | 1.006 | 1.000 | 1.000 | 377,985 | 377,985 | 1.000 |
| ordered-wide / insert8-end | 1,066 | 1,066 | 1.000 | 1.000 | 1.000 | 2,721 | 2,722 | 1.000 |
| ordered-wide / remove8-end | 727 | 771 | 1.061~ | 1.000 | 1.000 | 800 | 799 | 0.999 |
| ordered-wide / move8-end | 5,483 | 5,482 | 1.000 | 1.000 | 1.000 | 377,014 | 377,015 | 1.000 |
| ordered-wide / insert64-front | 6,641 | 6,677 | 1.005 | 1.000 | 1.000 | 378,106 | 378,108 | 1.000 |
| ordered-wide / remove64-front | 4,457 | 4,457 | 1.000 | 1.000 | 1.000 | 377,740 | 377,740 | 1.000 |
| ordered-wide / move64-front | 15,166 | 15,407 | 1.016 | 1.000 | 1.000 | 388,637 | 388,704 | 1.000 |
| ordered-wide / insert64-middle | 6,227 | 6,263 | 1.006 | 1.000 | 1.000 | 192,467 | 192,467 | 1.000 |
| ordered-wide / remove64-middle | 2,427 | 2,471 | 1.018 | 1.000 | 1.000 | 3,412 | 3,412 | 1.000 |
| ordered-wide / move64-middle | 14,206 | 14,206 | 1.000 | 1.000 | 1.002 | 386,729 | 386,729 | 1.000 |
| ordered-wide / insert64-end | 5,901 | 5,901 | 1.000 | 1.000 | 1.000 | 5,722 | 5,721 | 1.000 |
| ordered-wide / remove64-end | 1,760 | 1,760 | 1.000 | 1.000 | 1.000 | 887 | 887 | 1.000 |
| ordered-wide / move64-end | 13,405 | 13,405 | 1.000 | 1.000 | 1.001 | 384,171 | 383,897 | 0.999 |
| ordered-wide / append1 | 554 | 554 | 1.000 | 1.000 | 1.000 | 2,510 | 2,510 | 1.000 |
| ordered-wide / translate64 | 1,453 | 1,453 | 1.000 | 1.000 | 1.000 | 1,535 | 1,535 | 1.000 |
| ordered-wide / translate-half | 2,485 | 2,039 | 0.821 | 0.459 | 0.335 | 849,441 | 271,499 | 0.320 |
| ordered-wide / bulk-build | 98,722 | 98,778 | 1.001 | 1.000 | 1.000 | 86,301,622 | 86,430,627 | 1.001 |
| ordered-wide / build-append8 | 198,204 | 198,127 | 1.000 | 1.000 | 1.000 | 187,616,452 | 188,161,440 | 1.003 |
| ordered-wide / freeze-cold | 26,970 | 26,972 | 1.000 | 1.000 | 1.000 | 24,512,328 | 24,600,455 | 1.004 |
| ordered-wide / freeze-idle | 1,507 | 1,507 | 1.000 | 1.000 | 1.000 | 472,444 | 472,444 | 1.000 |
| ordered-wide / freeze-one | 1,805 | 1,805 | 1.000 | 1.000 | 1.000 | 472,743 | 472,742 | 1.000 |
| ordered-wide / freeze-all | 6,446 | 6,446 | 1.000 | 1.000 | 1.000 | 5,003,993 | 5,003,994 | 1.000 |
| ordered-wide / parent-last | 229 | 229 | 1.000 | 1.000 | 1.000 | 15,900 | 15,900 | 1.000 |
| ordered-wide / ancestors-last | 295 | 295 | 1.000 | 1.000 | 1.000 | 15,965 | 15,965 | 1.000 |
| ordered-wide / subtree-root | 205 | 205 | 1.000 | 1.000 | 1.000 | 17,812 | 17,812 | 1.000 |
| ordered-wide / subtree-leaf | 114 | 114 | 1.000 | 1.000 | 1.000 | 153 | 153 | 1.000 |
| ordered-chains32 / parent-last | 88 | 88 | 1.000 | 1.000 | 1.000 | 161 | 161 | 1.000 |
| ordered-chains32 / ancestors-last | 475 | 475 | 1.000 | 1.000 | 1.000 | 16,841 | 16,841 | 1.000 |
| ordered-chains32 / subtree-root | 205 | 205 | 1.000 | 1.000 | 1.000 | 17,812 | 17,812 | 1.000 |
| ordered-chains32 / subtree-leaf | 114 | 114 | 1.000 | 1.000 | 1.000 | 153 | 153 | 1.000 |
| ordered-wide / set-ink-pos | 371 | 371 | 1.000 | 1.000 | 1.000 | 452 | 452 | 1.000 |
| ordered-wide / set-ink-handle | 337 | 337 | 1.000 | 1.000 | 1.000 | 337 | 337 | 1.000 |
| ordered-wide / pos-of-handle | 24 | 24 | 1.000 | 1.000 | 1.000 | 24 | 24 | 1.000 |
| ordered-wide / touch-handle | 117 | 117 | 1.000 | 1.000 | 1.000 | 117 | 117 | 1.000 |
| ordered-wide / set-inks-handles64 | 18,664 | 18,664 | 1.000 | 1.000 | 1.000 | 35,713 | 35,713 | 1.000 |
| ordered-wide / insert8-front+pos64 | 5,927 | 5,927 | 1.000 | 1.000 | 1.000 | 377,123 | 377,121 | 1.000 |
| ordered-wide / insert8-front+posall | 4,432 | 4,432 | 1.000 | 1.000 | 1.000 | 785,609 | 785,608 | 1.000 |

## Spatial

| shape / operation | 1e3 base | 1e3 new | 1e3 | 1e4 | 1e5 | 1e6 base | 1e6 new | 1e6 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ordered-wide / cull-1-band | 353 | 353 | 1.000 | 1.000 | 1.000 | 53,792 | 53,792 | 1.000 |
| ordered-wide / cull-1-dispersed | 40,471 | 40,471 | 1.000 | 1.000 | 1.000 | 16,639,050 | 16,639,050 | 1.000 |
| ordered-wide / cull-10-band | 801 | 801 | 1.000 | 1.000 | 1.000 | 530,372 | 530,372 | 1.000 |
| ordered-wide / cull-10-dispersed | 82,835 | 82,835 | 1.000 | 1.000 | 1.000 | 16,776,202 | 16,776,202 | 1.000 |
| ordered-wide / cull-100-band | 5,519 | 5,519 | 1.000 | 1.000 | 1.000 | 5,296,180 | 5,296,180 | 1.000 |
| ordered-wide / cull-100-dispersed | 5,519 | 5,519 | 1.000 | 1.000 | 1.000 | 5,296,180 | 5,296,180 | 1.000 |
| ordered-wide / hit-present | 303 | 303 | 1.000 | 1.000 | 1.000 | 949 | 949 | 1.000 |
| ordered-wide / hit-gap | 286 | 286 | 1.000 | 1.000 | 1.000 | 790 | 790 | 1.000 |
| interleaved-wide / cull-1-band | 472 | 472 | 1.000 | 1.000 | 1.000 | 278,556 | 278,556 | 1.000 |
| interleaved-wide / cull-1-dispersed | 47,596 | 47,596 | 1.000 | 1.000 | 1.000 | 45,050,930 | 45,050,977 | 1.000 |
| interleaved-wide / cull-10-band | 805 | 805 | 1.000 | 1.000 | 1.000 | 755,080 | 755,080 | 1.000 |
| interleaved-wide / cull-10-dispersed | 88,056 | 88,056 | 1.000 | 1.000 | 1.000 | 87,272,177 | 87,273,278 | 1.000 |
| interleaved-wide / cull-100-band | 5,519 | 5,519 | 1.000 | 1.000 | 1.000 | 5,296,180 | 5,296,180 | 1.000 |
| interleaved-wide / cull-100-dispersed | 5,519 | 5,519 | 1.000 | 1.000 | 1.000 | 5,296,180 | 5,296,181 | 1.000 |
| interleaved-wide / hit-present | 277 | 277 | 1.000 | 1.000 | 1.000 | 1,298 | 1,298 | 1.000 |
| interleaved-wide / hit-gap | 286 | 286 | 1.000 | 1.000 | 1.000 | 97,103 | 97,103 | 1.000 |

## Cells above 1.02x

* fresh-core ordered-wide / move8-end N=1,000,000: 464,335 -> 489,206 (1.054x)

## Above 1.02x but inside the cell's own scatter (`~`)

The larger of the two tags' run-to-run spread and sample noise exceeds the excess: not a measured regression.

* warm-core ordered-wide / remove8-end N=1,000: 727 -> 771 (1.061x; scatter 7%)
* fresh-core ordered-wide / move8-front N=1,000,000: 100,912 -> 105,971 (1.050x; scatter 37%)
* warm-core ordered-wide / remove8-middle N=1,000: 1,017 -> 1,053 (1.035x; scatter 4%)
* fresh-core ordered-wide / insert64-front N=1,000,000: 465,005 -> 477,920 (1.028x; scatter 5%)
* fresh-core ordered-wide / move8-middle N=1,000,000: 465,189 -> 475,221 (1.022x; scatter 5%)

