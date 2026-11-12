# `c` against `a`

new / base per cell (values as in the matrix). **bold**: above 1.02x; `~`: above 1.02x but inside the cell's own scatter. Cells under 200 instructions
are counter-dominated: `+k` is the absolute difference in instructions.

## Structure, fresh tree

| shape / operation | 1e3 base | 1e3 new | 1e3 | 1e4 | 1e5 | 1e6 base | 1e6 new | 1e6 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ordered-wide / insert8-front | 8,366 | 8,265 | 0.988 | 0.911 | 0.746 | 454,350 | 347,173 | 0.764 |
| ordered-wide / remove8-front | 1,556 | 1,557 | 1.001 | 1.001 | 1.001 | 2,516 | 2,516 | 1.000 |
| ordered-wide / move8-front | 3,701 | 3,702 | 1.000 | 1.001 | 1.001 | 106,146 | 106,145 | 1.000 |
| ordered-wide / insert8-middle | 7,950 | 7,875 | 0.991 | 0.953 | 0.808 | 276,001 | 218,441 | 0.791 |
| ordered-wide / remove8-middle | 1,156 | 1,157 | 1.001 | 1.001 | 1.001 | 1,867 | 1,868 | 1.001 |
| ordered-wide / move8-middle | 10,646 | 10,509 | 0.987 | 0.926 | 0.758 | 478,052 | 337,434 | 0.706 |
| ordered-wide / insert8-end | 1,058 | 1,058 | 1.000 | 1.000 | 1.000 | 102,609 | 89,442 | 0.872 |
| ordered-wide / remove8-end | 824 | 824 | 1.000 | 1.000 | 1.000 | 854 | 854 | 1.000 |
| ordered-wide / move8-end | 10,314 | 10,176 | 0.987 | 0.920 | 0.755 | 476,996 | 323,344 | 0.678 |
| ordered-wide / insert64-front | 11,322 | 11,188 | 0.988 | 0.926 | 0.759 | 477,982 | 324,268 | 0.678 |
| ordered-wide / remove64-front | 4,717 | 4,589 | 0.973 | 0.829 | 0.664 | 377,999 | 237,392 | 0.628 |
| ordered-wide / move64-front | 16,566 | 16,436 | 0.992 | 0.928 | 0.740 | 389,989 | 249,377 | 0.639 |
| ordered-wide / insert64-middle | 10,856 | 10,781 | 0.993 | 0.962 | 0.823 | 292,190 | 208,875 | 0.715 |
| ordered-wide / remove64-middle | 2,603 | 2,606 | 1.001 | 1.000 | 1.000 | 3,546 | 3,547 | 1.000 |
| ordered-wide / move64-middle | 20,195 | 20,095 | 0.995 | 0.948 | 0.793 | 487,850 | 334,366 | 0.685 |
| ordered-wide / insert64-end | 10,471 | 10,468 | 1.000 | 1.007 | 1.000 | 105,480 | 92,552 | 0.877 |
| ordered-wide / remove64-end | 1,984 | 1,986 | 1.001 | 1.000 | 1.000 | 1,058 | 1,060 | 1.002 |
| ordered-wide / move64-end | 14,805 | 14,756 | 0.997 | 0.921 | 0.729 | 385,523 | 244,914 | 0.635 |
| ordered-wide / append1 | 545 | 546 | 1.002 | 1.000 | 0.998 | 102,288 | 89,229 | 0.872 |
| ordered-wide / translate64 | 1,457 | 1,457 | 1.000 | 1.000 | 1.000 | 1,540 | 1,540 | 1.000 |
| ordered-wide / translate-half | 2,503 | 2,503 | 1.000 | 1.000 | 1.000 | 865,068 | 865,068 | 1.000 |
| ordered-wide / bulk-build | 97,861 | 97,884 | 1.000 | 1.000 | 1.000 | 85,514,120 | 85,437,766 | 0.999 |
| ordered-wide / build-append8 | 196,372 | 196,378 | 1.000 | 1.000 | 1.000 | 186,784,546 | 186,710,614 | 1.000 |
| ordered-wide / freeze-cold | 26,951 | 26,960 | 1.000 | 1.000 | 1.000 | 24,581,794 | 24,552,367 | 0.999 |
| ordered-wide / freeze-idle | 1,497 | 1,497 | 1.000 | 1.004 | 1.000 | 472,434 | 472,434 | 1.000 |
| ordered-wide / freeze-one | 1,795 | 1,795 | 1.000 | 1.004 | 1.000 | 472,733 | 472,732 | 1.000 |
| ordered-wide / freeze-all | 6,436 | 6,436 | 1.000 | 1.000 | 1.000 | 5,003,983 | 5,003,983 | 1.000 |
| ordered-wide / parent-last | 229 | 229 | 1.000 | 1.000 | 1.000 | 15,900 | 15,900 | 1.000 |
| ordered-wide / ancestors-last | 295 | 295 | 1.000 | 1.000 | 1.000 | 15,965 | 15,965 | 1.000 |
| ordered-wide / subtree-root | 205 | 205 | 1.000 | 1.000 | 1.000 | 17,812 | 17,812 | 1.000 |
| ordered-wide / subtree-leaf | 114 | 114 | 1.000 | 1.000 | 1.000 | 153 | 153 | 1.000 |
| ordered-chains32 / parent-last | 88 | 88 | 1.000 | 1.000 | 1.000 | 161 | 161 | 1.000 |
| ordered-chains32 / ancestors-last | 475 | 475 | 1.000 | 1.000 | 1.000 | 16,841 | 16,841 | 1.000 |
| ordered-chains32 / subtree-root | 205 | 205 | 1.000 | 1.000 | 1.000 | 17,812 | 17,812 | 1.000 |
| ordered-chains32 / subtree-leaf | 114 | 114 | 1.000 | 1.000 | 1.000 | 153 | 153 | 1.000 |
| ordered-wide / set-ink-pos | 421 | 421 | 1.000 | 1.000 | 1.000 | 502 | 502 | 1.000 |
| ordered-wide / set-ink-handle | 387 | 394 | 1.018 | 1.018 | 1.018 | 387 | 394 | 1.018 |
| ordered-wide / pos-of-handle | 24 | 29 | **1.208 (+5)** | **1.208 (+5)** | **1.208 (+5)** | 24 | 29 | **1.208 (+5)** |
| ordered-wide / touch-handle | 131 | 132 | 1.008 (+1) | 1.008 (+1) | 1.008 (+1) | 131 | 132 | 1.008 (+1) |
| ordered-wide / set-inks-handles64 | 19,763 | 19,765 | 1.000 | 1.008 | 1.007 | 37,580 | 37,774 | 1.005 |
| ordered-wide / insert8-front+pos64 | 10,559 | 11,459 | **1.085** | **1.038** | 0.795 | 457,881 | 319,686 | 0.698 |
| ordered-wide / insert8-front+posall | 9,064 | 9,496 | **1.048** | **1.243** | **1.466** | 898,433 | 1,347,191 | **1.499** |

## Structure, warm columns

| shape / operation | 1e3 base | 1e3 new | 1e3 | 1e4 | 1e5 | 1e6 base | 1e6 new | 1e6 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ordered-wide / insert8-front | 3,728 | 3,626 | 0.973 | 0.801 | 0.653 | 375,195 | 234,577 | 0.625 |
| ordered-wide / remove8-front | 1,520 | 1,554 | 1.022~ | 1.001 | 1.001 | 2,516 | 2,517 | 1.000 |
| ordered-wide / move8-front | 3,699 | 3,702 | 1.001 | 1.000 | 1.000 | 6,364 | 6,363 | 1.000 |
| ordered-wide / insert8-middle | 3,275 | 3,237 | 0.988 | 0.864 | 0.679 | 188,961 | 118,651 | 0.628 |
| ordered-wide / remove8-middle | 1,117 | 1,157 | 1.036~ | 1.001 | 1.001 | 1,867 | 1,868 | 1.001 |
| ordered-wide / move8-middle | 6,005 | 5,871 | 0.978 | 0.860 | 0.675 | 378,226 | 237,609 | 0.628 |
| ordered-wide / insert8-end | 1,058 | 1,058 | 1.000 | 1.001 | 1.000 | 2,684 | 2,682 | 0.999 |
| ordered-wide / remove8-end | 824 | 824 | 1.000 | 1.001 | 1.000 | 854 | 854 | 1.000 |
| ordered-wide / move8-end | 5,672 | 5,538 | 0.976 | 0.848 | 0.669 | 377,207 | 236,589 | 0.627 |
| ordered-wide / insert64-front | 6,686 | 6,550 | 0.980 | 0.859 | 0.676 | 378,116 | 237,500 | 0.628 |
| ordered-wide / remove64-front | 4,537 | 4,410 | 0.972 | 0.826 | 0.662 | 377,822 | 237,213 | 0.628 |
| ordered-wide / move64-front | 16,378 | 16,257 | 0.993 | 0.931 | 0.740 | 389,811 | 249,341 | 0.640 |
| ordered-wide / insert64-middle | 6,216 | 6,143 | 0.988 | 0.914 | 0.718 | 192,365 | 122,055 | 0.634 |
| ordered-wide / remove64-middle | 2,605 | 2,606 | 1.000 | 1.000 | 1.000 | 3,546 | 3,547 | 1.000 |
| ordered-wide / move64-middle | 15,514 | 15,457 | 0.996 | 0.927 | 0.734 | 388,165 | 247,549 | 0.638 |
| ordered-wide / insert64-end | 5,837 | 5,838 | 1.000 | 1.000 | 1.000 | 5,663 | 5,661 | 1.000 |
| ordered-wide / remove64-end | 1,806 | 1,807 | 1.001 | 1.001 | 1.001 | 880 | 881 | 1.001 |
| ordered-wide / move64-end | 14,625 | 14,576 | 0.997 | 0.923 | 0.728 | 385,343 | 244,876 | 0.635 |
| ordered-wide / append1 | 546 | 546 | 1.000 | 1.002 | 1.000 | 2,471 | 2,469 | 0.999 |
| ordered-wide / translate64 | 1,457 | 1,457 | 1.000 | 1.000 | 1.000 | 1,540 | 1,540 | 1.000 |
| ordered-wide / translate-half | 2,502 | 2,503 | 1.000 | 1.000 | 1.000 | 865,068 | 865,068 | 1.000 |
| ordered-wide / bulk-build | 97,867 | 97,891 | 1.000 | 1.000 | 1.000 | 85,468,095 | 85,537,723 | 1.001 |
| ordered-wide / build-append8 | 196,502 | 196,382 | 0.999 | 1.000 | 1.000 | 186,209,576 | 186,021,691 | 0.999 |
| ordered-wide / freeze-cold | 26,956 | 26,959 | 1.000 | 1.000 | 1.000 | 24,565,679 | 24,546,063 | 0.999 |
| ordered-wide / freeze-idle | 1,497 | 1,497 | 1.000 | 1.000 | 1.000 | 472,434 | 472,434 | 1.000 |
| ordered-wide / freeze-one | 1,795 | 1,795 | 1.000 | 1.000 | 1.000 | 472,732 | 472,732 | 1.000 |
| ordered-wide / freeze-all | 6,436 | 6,436 | 1.000 | 1.000 | 1.000 | 5,003,983 | 5,003,983 | 1.000 |
| ordered-wide / parent-last | 229 | 229 | 1.000 | 1.000 | 1.000 | 15,900 | 15,900 | 1.000 |
| ordered-wide / ancestors-last | 295 | 295 | 1.000 | 1.000 | 1.000 | 15,965 | 15,965 | 1.000 |
| ordered-wide / subtree-root | 205 | 205 | 1.000 | 1.000 | 1.000 | 17,812 | 17,812 | 1.000 |
| ordered-wide / subtree-leaf | 114 | 114 | 1.000 | 1.000 | 1.000 | 153 | 153 | 1.000 |
| ordered-chains32 / parent-last | 88 | 88 | 1.000 | 1.000 | 1.000 | 161 | 161 | 1.000 |
| ordered-chains32 / ancestors-last | 475 | 475 | 1.000 | 1.000 | 1.000 | 16,841 | 16,841 | 1.000 |
| ordered-chains32 / subtree-root | 205 | 205 | 1.000 | 1.000 | 1.000 | 17,812 | 17,812 | 1.000 |
| ordered-chains32 / subtree-leaf | 114 | 114 | 1.000 | 1.000 | 1.000 | 153 | 153 | 1.000 |
| ordered-wide / set-ink-pos | 421 | 421 | 1.000 | 1.000 | 1.000 | 502 | 502 | 1.000 |
| ordered-wide / set-ink-handle | 387 | 394 | 1.018 | 1.018 | 1.018 | 387 | 394 | 1.018 |
| ordered-wide / pos-of-handle | 24 | 29 | **1.208 (+5)** | **1.208 (+5)** | **1.208 (+5)** | 24 | 29 | **1.208 (+5)** |
| ordered-wide / touch-handle | 131 | 132 | 1.008 (+1) | 1.008 (+1) | 1.008 (+1) | 131 | 132 | 1.008 (+1) |
| ordered-wide / set-inks-handles64 | 19,763 | 19,765 | 1.000 | 1.008 | 1.007 | 37,580 | 37,774 | 1.005 |
| ordered-wide / insert8-front+pos64 | 5,907 | 6,806 | **1.152** | **1.109** | 0.725 | 377,101 | 238,866 | 0.633 |
| ordered-wide / insert8-front+posall | 4,412 | 4,844 | **1.098** | **1.393** | **1.549** | 785,587 | 1,234,770 | **1.572** |

## Spatial

| shape / operation | 1e3 base | 1e3 new | 1e3 | 1e4 | 1e5 | 1e6 base | 1e6 new | 1e6 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ordered-wide / cull-1-band | 369 | 369 | 1.000 | 1.000 | 1.000 | 57,860 | 57,860 | 1.000 |
| ordered-wide / cull-1-dispersed | 41,205 | 41,205 | 1.000 | 1.000 | 1.000 | 17,529,740 | 17,529,740 | 1.000 |
| ordered-wide / cull-10-band | 846 | 846 | 1.000 | 1.000 | 1.000 | 570,976 | 570,976 | 1.000 |
| ordered-wide / cull-10-dispersed | 82,857 | 82,857 | 1.000 | 1.000 | 1.000 | 17,668,106 | 17,668,106 | 1.000 |
| ordered-wide / cull-100-band | 5,928 | 5,928 | 1.000 | 1.000 | 1.000 | 5,702,180 | 5,702,180 | 1.000 |
| ordered-wide / cull-100-dispersed | 5,928 | 5,928 | 1.000 | 1.000 | 1.000 | 5,702,180 | 5,702,180 | 1.000 |
| ordered-wide / hit-present | 325 | 325 | 1.000 | 1.000 | 1.000 | 988 | 988 | 1.000 |
| ordered-wide / hit-gap | 306 | 306 | 1.000 | 1.000 | 1.000 | 813 | 813 | 1.000 |
| interleaved-wide / cull-1-band | 525 | 525 | 1.000 | 1.000 | 1.000 | 332,305 | 332,305 | 1.000 |
| interleaved-wide / cull-1-dispersed | 48,299 | 48,299 | 1.000 | 1.000 | 1.000 | 45,776,125 | 45,776,196 | 1.000 |
| interleaved-wide / cull-10-band | 867 | 867 | 1.000 | 1.000 | 1.000 | 845,780 | 845,780 | 1.000 |
| interleaved-wide / cull-10-dispersed | 88,079 | 88,079 | 1.000 | 1.000 | 1.000 | 87,288,896 | 87,288,897 | 1.000 |
| interleaved-wide / cull-100-band | 5,928 | 5,928 | 1.000 | 1.000 | 1.000 | 5,702,180 | 5,702,180 | 1.000 |
| interleaved-wide / cull-100-dispersed | 5,928 | 5,928 | 1.000 | 1.000 | 1.000 | 5,702,180 | 5,702,180 | 1.000 |
| interleaved-wide / hit-present | 299 | 299 | 1.000 | 1.000 | 1.000 | 1,527 | 1,527 | 1.000 |
| interleaved-wide / hit-gap | 306 | 306 | 1.000 | 1.000 | 1.000 | 123,870 | 123,870 | 1.000 |

## Cells above 1.02x

* warm-core ordered-wide / insert8-front+posall N=1,000,000: 785,587 -> 1,234,770 (1.572x)
* warm-core ordered-wide / insert8-front+posall N=100,000: 81,784 -> 126,678 (1.549x)
* fresh-core ordered-wide / insert8-front+posall N=1,000,000: 898,433 -> 1,347,191 (1.499x)
* fresh-core ordered-wide / insert8-front+posall N=100,000: 96,551 -> 141,513 (1.466x)
* warm-core ordered-wide / insert8-front+posall N=10,000: 11,390 -> 15,866 (1.393x)
* fresh-core ordered-wide / insert8-front+posall N=10,000: 18,618 -> 23,135 (1.243x)
* fresh-core ordered-wide / pos-of-handle N=100,000: 24 -> 29 (1.208x)
* warm-core ordered-wide / pos-of-handle N=1,000,000: 24 -> 29 (1.208x)
* warm-core ordered-wide / pos-of-handle N=100,000: 24 -> 29 (1.208x)
* warm-core ordered-wide / pos-of-handle N=10,000: 24 -> 29 (1.208x)
* warm-core ordered-wide / pos-of-handle N=1,000: 24 -> 29 (1.208x)
* fresh-core ordered-wide / pos-of-handle N=1,000,000: 24 -> 29 (1.208x)
* fresh-core ordered-wide / pos-of-handle N=10,000: 24 -> 29 (1.208x)
* fresh-core ordered-wide / pos-of-handle N=1,000: 24 -> 29 (1.208x)
* warm-core ordered-wide / insert8-front+pos64 N=1,000: 5,907 -> 6,806 (1.152x)
* warm-core ordered-wide / insert8-front+pos64 N=10,000: 8,938 -> 9,914 (1.109x)
* warm-core ordered-wide / insert8-front+posall N=1,000: 4,412 -> 4,844 (1.098x)
* fresh-core ordered-wide / insert8-front+pos64 N=1,000: 10,559 -> 11,459 (1.085x)
* fresh-core ordered-wide / insert8-front+posall N=1,000: 9,064 -> 9,496 (1.048x)
* fresh-core ordered-wide / insert8-front+pos64 N=10,000: 16,534 -> 17,165 (1.038x)

## Above 1.02x but inside the cell's own scatter (`~`)

The larger of the two tags' run-to-run spread and sample noise exceeds the excess: not a measured regression.

* warm-core ordered-wide / remove8-middle N=1,000: 1,117 -> 1,157 (1.036x; scatter 4%)
* warm-core ordered-wide / remove8-front N=1,000: 1,520 -> 1,554 (1.022x; scatter 2%)

