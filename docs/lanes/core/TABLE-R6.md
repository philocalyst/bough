# R6 instruction matrix: `base`

Retired instructions per operation (`proc_pid_rusage`): median over three runs of the minimum of five samples
(deterministic code; the counter only ever adds: kernel work for page faults and interrupts is included).
`e` columns: growth exponent per decade (log10 of the ratio of adjacent N); **bold** = more than 0.25
above the ideal exponent. `spread` = worst (max - min) / median of the three per-run minima over the four N;
`noise` = worst (median of all 15 samples / value - 1) over the four N.
`fresh` = edit applied to a just-bulk-built tree (includes the one-time reallocation of the directory
columns, which a bulk build leaves at exact capacity); `warm` = the same after a split and its inverse
at the tail took that reallocation (`R6_WARM=1`). Queries and culls do not edit: one kind.

## Structure, fresh tree

| shape / operation | 1e3 | 1e4 | e | 1e5 | e | 1e6 | e | ideal e | spread | noise | ideal bound |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|
| ordered-wide / insert8-front | 8,599 | 16,653 | **0.29** | 73,485 | **0.64** | 656,670 | **0.95** | 0 | 2.7% | 2.0% | K + log N |
| ordered-wide / remove8-front | 1,556 | 1,851 | 0.08 | 1,722 | -0.03 | 2,516 | 0.16 | 0 | 0.1% | 0.0% | K + log N |
| ordered-wide / move8-front | 3,701 | 4,062 | 0.04 | 3,980 | -0.01 | 404,617 | **2.01** | 0 | 3.2% | 3.2% | K + log N |
| ordered-wide / insert8-middle | 8,195 | 15,327 | **0.27** | 60,814 | **0.60** | 528,887 | **0.94** | 0 | 0.5% | 2.4% | K + log N |
| ordered-wide / remove8-middle | 1,156 | 1,782 | 0.19 | 1,549 | -0.06 | 1,867 | 0.08 | 0 | 0.1% | 0.0% | K + log N |
| ordered-wide / move8-middle | 10,842 | 19,556 | **0.26** | 76,178 | **0.59** | 659,715 | **0.94** | 0 | 2.0% | 2.0% | K + log N |
| ordered-wide / insert8-end | 1,058 | 1,091 | 0.01 | 1,115 | 0.01 | 400,475 | **2.56** | 0 | 3.2% | 0.1% | K + log N |
| ordered-wide / remove8-end | 824 | 1,042 | 0.10 | 878 | -0.07 | 854 | -0.01 | 0 | 0.1% | 0.0% | K + log N |
| ordered-wide / move8-end | 10,510 | 18,816 | **0.25** | 75,507 | **0.60** | 658,682 | **0.94** | 0 | 1.9% | 2.0% | K + log N |
| ordered-wide / insert64-front | 11,522 | 19,577 | 0.23 | 76,408 | **0.59** | 653,176 | **0.93** | 0 | 4.0% | 3.0% | K + log N |
| ordered-wide / remove64-front | 4,893 | 9,975 | **0.31** | 59,967 | **0.78** | 559,629 | **0.97** | 0 | 0.7% | 0.0% | K + log N |
| ordered-wide / move64-front | 17,024 | 24,942 | 0.17 | 102,227 | **0.61** | 870,008 | **0.93** | 0 | 0.8% | 1.7% | K + log N |
| ordered-wide / insert64-middle | 11,099 | 18,276 | 0.22 | 63,754 | **0.54** | 519,133 | **0.91** | 0 | 2.5% | 2.5% | K + log N |
| ordered-wide / remove64-middle | 2,605 | 2,890 | 0.05 | 3,080 | 0.03 | 3,546 | 0.06 | 0 | 0.0% | 0.0% | K + log N |
| ordered-wide / move64-middle | 20,428 | 28,737 | 0.15 | 85,863 | **0.48** | 656,525 | **0.88** | 0 | 5.4% | 3.9% | K + log N |
| ordered-wide / insert64-end | 10,771 | 16,559 | 0.19 | 50,676 | **0.49** | 390,923 | **0.89** | 0 | 3.3% | 3.3% | K + log N |
| ordered-wide / remove64-end | 2,259 | 5,089 | **0.35** | 31,812 | **0.80** | 299,396 | **0.97** | 0 | 0.7% | 0.0% | K + log N |
| ordered-wide / move64-end | 15,342 | 22,995 | 0.18 | 99,745 | **0.64** | 865,589 | **0.94** | 0 | 1.2% | 0.2% | K + log N |
| ordered-wide / append1 | 546 | 579 | 0.03 | 603 | 0.02 | 387,535 | **2.81** | 0 | 0.0% | 0.0% | K + log N |
| ordered-wide / translate64 | 1,457 | 1,492 | 0.01 | 1,516 | 0.01 | 1,540 | 0.01 | 0 | 0.1% | 0.0% | K + log N |
| ordered-wide / translate-half | 2,503 | 10,554 | 0.62 | 88,274 | 0.92 | 865,068 | 0.99 | 1 | 1.4% | 0.0% | pages moved (N/128); 0 with lazy range tags |
| ordered-wide / bulk-build | 98,071 | 856,315 | 0.94 | 8,323,097 | 0.99 | 84,536,973 | 1.01 | 1 | 1.8% | 1.3% | N |
| ordered-wide / build-append8 | 199,299 | 2,134,435 | 1.03 | 48,734,024 | **1.36** | 3,229,981,078 | **1.82** | 1 | 0.4% | 0.0% | N |
| ordered-wide / freeze-cold | 26,959 | 229,444 | 0.93 | 2,158,874 | 0.97 | 24,684,780 | 1.06 | 1 | 3.9% | 4.0% | N |
| ordered-wide / freeze-idle | 1,497 | 5,743 | **0.58** | 48,148 | **0.92** | 472,435 | **0.99** | 0 | 2.3% | 0.0% | changed pages |
| ordered-wide / freeze-one | 1,795 | 6,042 | **0.53** | 48,446 | **0.90** | 472,732 | **0.99** | 0 | 1.0% | 0.0% | changed pages |
| ordered-wide / freeze-all | 6,436 | 51,573 | 0.90 | 501,717 | 0.99 | 5,003,983 | 1.00 | 1 | 0.1% | 0.0% | N |
| ordered-wide / parent-last | 229 | 364 | 0.20 | 1,800 | **0.69** | 15,900 | **0.95** | 0 | 0.0% | 0.0% | log N |
| ordered-wide / ancestors-last | 295 | 430 | 0.16 | 1,866 | **0.64** | 15,965 | **0.93** | 0 | 0.0% | 0.0% | log N |
| ordered-wide / subtree-root | 205 | 382 | **0.27** | 1,978 | **0.71** | 17,812 | **0.95** | 0 | 0.0% | 0.0% | log N |
| ordered-wide / subtree-leaf | 114 | 171 | 0.18 | 179 | 0.02 | 153 | -0.07 | 0 | 0.0% | 0.0% | log N |
| ordered-chains32 / parent-last | 88 | 121 | 0.14 | 145 | 0.08 | 161 | 0.05 | 0 | 0.0% | 0.0% | log N |
| ordered-chains32 / ancestors-last | 475 | 837 | 0.25 | 2,737 | **0.51** | 16,841 | **0.79** | 0 | 0.0% | 0.0% | log N |
| ordered-chains32 / subtree-root | 205 | 382 | **0.27** | 1,978 | **0.71** | 17,812 | **0.95** | 0 | 0.0% | 0.0% | log N |
| ordered-chains32 / subtree-leaf | 114 | 171 | 0.18 | 179 | 0.02 | 153 | -0.07 | 0 | 0.0% | 0.0% | log N |
| ordered-wide / set-ink-pos | 421 | 454 | 0.03 | 478 | 0.02 | 502 | 0.02 | 0 | 0.0% | 0.0% | log N |
| ordered-wide / set-ink-handle | 387 | 387 | 0.00 | 387 | 0.00 | 387 | 0.00 | 0 | 0.0% | 0.0% | log N |
| ordered-wide / pos-of-handle | 24 | 24 | 0.00 | 24 | 0.00 | 24 | 0.00 | 0 | 0.1% | 0.0% | log N |
| ordered-wide / touch-handle | 131 | 131 | 0.00 | 131 | 0.00 | 131 | 0.00 | 0 | 0.0% | 0.0% | log N |
| ordered-wide / set-inks-handles64 | 19,763 | 24,688 | 0.10 | 29,348 | 0.08 | 37,580 | 0.11 | 0 | 0.2% | 0.0% | 64 writes |
| ordered-wide / insert8-front+pos64 | 10,758 | 18,485 | 0.24 | 75,431 | **0.61** | 645,405 | **0.93** | 0 | 3.1% | 0.3% | K + log N |
| ordered-wide / insert8-front+posall | 9,261 | 20,458 | 0.34 | 114,742 | 0.75 | 1,083,174 | 0.97 | 1 | 1.3% | 2.4% | one lookup per page |

## Structure, warm columns

| shape / operation | 1e3 | 1e4 | e | 1e5 | e | 1e6 | e | ideal e | spread | noise | ideal bound |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|
| ordered-wide / insert8-front | 3,961 | 8,878 | **0.35** | 58,698 | **0.82** | 556,844 | **0.98** | 0 | 0.1% | 0.0% | K + log N |
| ordered-wide / remove8-front | 1,556 | 1,851 | 0.08 | 1,722 | -0.03 | 2,516 | 0.16 | 0 | 0.1% | 0.0% | K + log N |
| ordered-wide / move8-front | 3,700 | 4,062 | 0.04 | 3,981 | -0.01 | 304,701 | **1.88** | 0 | 0.2% | 0.0% | K + log N |
| ordered-wide / insert8-middle | 3,557 | 7,551 | **0.33** | 45,897 | **0.78** | 428,952 | **0.97** | 0 | 0.1% | 0.0% | K + log N |
| ordered-wide / remove8-middle | 1,155 | 1,782 | 0.19 | 1,549 | -0.06 | 1,867 | 0.08 | 0 | 0.2% | 0.1% | K + log N |
| ordered-wide / move8-middle | 6,202 | 11,780 | **0.28** | 61,391 | **0.72** | 559,879 | **0.96** | 0 | 0.1% | 0.0% | K + log N |
| ordered-wide / insert8-end | 1,057 | 1,091 | 0.01 | 1,115 | 0.01 | 301,021 | **2.43** | 0 | 0.2% | 0.1% | K + log N |
| ordered-wide / remove8-end | 824 | 1,042 | 0.10 | 878 | -0.07 | 854 | -0.01 | 0 | 0.1% | 0.0% | K + log N |
| ordered-wide / move8-end | 5,870 | 11,040 | **0.27** | 60,718 | **0.74** | 558,856 | **0.96** | 0 | 0.1% | 0.0% | K + log N |
| ordered-wide / insert64-front | 6,884 | 11,801 | 0.23 | 61,621 | **0.72** | 559,765 | **0.96** | 0 | 0.1% | 0.0% | K + log N |
| ordered-wide / remove64-front | 4,714 | 9,802 | **0.32** | 59,788 | **0.79** | 559,450 | **0.97** | 0 | 0.1% | 0.0% | K + log N |
| ordered-wide / move64-front | 16,845 | 24,771 | 0.17 | 101,986 | **0.61** | 869,756 | **0.93** | 0 | 1.2% | 1.7% | K + log N |
| ordered-wide / insert64-middle | 6,461 | 10,500 | 0.21 | 48,973 | **0.67** | 432,357 | **0.95** | 0 | 0.1% | 0.0% | K + log N |
| ordered-wide / remove64-middle | 2,605 | 2,890 | 0.05 | 3,080 | 0.03 | 3,546 | 0.06 | 0 | 0.1% | 0.0% | K + log N |
| ordered-wide / move64-middle | 15,789 | 20,795 | 0.12 | 71,055 | **0.53** | 569,674 | **0.90** | 0 | 0.5% | 0.0% | K + log N |
| ordered-wide / insert64-end | 6,141 | 8,887 | 0.16 | 35,890 | **0.61** | 303,999 | **0.93** | 0 | 0.0% | 0.0% | K + log N |
| ordered-wide / remove64-end | 2,080 | 4,910 | **0.37** | 31,633 | **0.81** | 299,219 | **0.98** | 0 | 0.0% | 0.0% | K + log N |
| ordered-wide / move64-end | 15,163 | 22,812 | 0.18 | 99,583 | **0.64** | 865,398 | **0.94** | 0 | 0.5% | 0.0% | K + log N |
| ordered-wide / append1 | 546 | 579 | 0.03 | 603 | 0.02 | 300,808 | **2.70** | 0 | 0.3% | 0.0% | K + log N |
| ordered-wide / translate64 | 1,456 | 1,492 | 0.01 | 1,516 | 0.01 | 1,540 | 0.01 | 0 | 0.1% | 0.1% | K + log N |
| ordered-wide / translate-half | 2,502 | 10,555 | 0.63 | 88,274 | 0.92 | 865,068 | 0.99 | 1 | 0.1% | 0.0% | pages moved (N/128); 0 with lazy range tags |
| ordered-wide / bulk-build | 98,065 | 856,196 | 0.94 | 8,322,969 | 0.99 | 85,693,944 | 1.01 | 1 | 1.2% | 0.1% | N |
| ordered-wide / build-append8 | 199,292 | 2,134,023 | 1.03 | 48,734,018 | **1.36** | 3,229,385,391 | **1.82** | 1 | 0.0% | 0.0% | N |
| ordered-wide / freeze-cold | 26,960 | 229,372 | 0.93 | 2,159,007 | 0.97 | 25,510,010 | 1.07 | 1 | 8.5% | 0.6% | N |
| ordered-wide / freeze-idle | 1,496 | 5,762 | **0.59** | 48,148 | **0.92** | 472,434 | **0.99** | 0 | 0.3% | 0.3% | changed pages |
| ordered-wide / freeze-one | 1,795 | 6,060 | **0.53** | 48,446 | **0.90** | 472,732 | **0.99** | 0 | 0.3% | 0.0% | changed pages |
| ordered-wide / freeze-all | 6,436 | 51,591 | 0.90 | 501,717 | 0.99 | 5,003,983 | 1.00 | 1 | 0.0% | 0.0% | N |
| ordered-wide / parent-last | 229 | 364 | 0.20 | 1,800 | **0.69** | 15,900 | **0.95** | 0 | 0.0% | 0.0% | log N |
| ordered-wide / ancestors-last | 295 | 430 | 0.16 | 1,866 | **0.64** | 15,965 | **0.93** | 0 | 0.0% | 0.0% | log N |
| ordered-wide / subtree-root | 205 | 382 | **0.27** | 1,978 | **0.71** | 17,812 | **0.95** | 0 | 0.0% | 0.0% | log N |
| ordered-wide / subtree-leaf | 114 | 171 | 0.18 | 179 | 0.02 | 153 | -0.07 | 0 | 0.0% | 0.0% | log N |
| ordered-chains32 / parent-last | 88 | 121 | 0.14 | 145 | 0.08 | 161 | 0.05 | 0 | 0.0% | 0.0% | log N |
| ordered-chains32 / ancestors-last | 475 | 837 | 0.25 | 2,737 | **0.51** | 16,841 | **0.79** | 0 | 0.0% | 0.0% | log N |
| ordered-chains32 / subtree-root | 205 | 382 | **0.27** | 1,978 | **0.71** | 17,812 | **0.95** | 0 | 0.0% | 0.0% | log N |
| ordered-chains32 / subtree-leaf | 114 | 171 | 0.18 | 179 | 0.02 | 153 | -0.07 | 0 | 0.0% | 0.0% | log N |
| ordered-wide / set-ink-pos | 421 | 454 | 0.03 | 478 | 0.02 | 502 | 0.02 | 0 | 0.0% | 0.0% | log N |
| ordered-wide / set-ink-handle | 387 | 387 | 0.00 | 387 | 0.00 | 387 | 0.00 | 0 | 0.0% | 0.0% | log N |
| ordered-wide / pos-of-handle | 24 | 24 | 0.00 | 24 | 0.00 | 24 | 0.00 | 0 | 0.0% | 0.0% | log N |
| ordered-wide / touch-handle | 131 | 131 | 0.00 | 131 | 0.00 | 131 | 0.00 | 0 | 0.0% | 0.0% | log N |
| ordered-wide / set-inks-handles64 | 19,763 | 24,688 | 0.10 | 29,348 | 0.08 | 37,580 | 0.11 | 0 | 0.0% | 0.0% | 64 writes |
| ordered-wide / insert8-front+pos64 | 6,106 | 10,786 | 0.25 | 60,606 | **0.75** | 558,752 | **0.96** | 0 | 0.1% | 0.0% | K + log N |
| ordered-wide / insert8-front+posall | 4,609 | 13,237 | 0.46 | 99,970 | 0.88 | 967,236 | 0.99 | 1 | 0.1% | 0.0% | one lookup per page |

## Spatial

| shape / operation | 1e3 | 1e4 | e | 1e5 | e | 1e6 | e | ideal e | spread | noise | ideal bound |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|
| ordered-wide / cull-1-band | 369 | 903 | 0.39 | 6,060 | 0.83 | 57,860 | 0.98 | 1 | 0.0% | 0.0% | emitted records (output grows with N) out=10/100/1000/10000 |
| ordered-wide / cull-1-dispersed | 41,205 | 200,236 | 0.69 | 1,781,100 | 0.95 | 17,529,740 | 0.99 | 1 | 0.0% | 0.0% | emitted records (output grows with N) out=11/100/1000/10000 |
| ordered-wide / cull-10-band | 846 | 6,023 | 0.85 | 57,371 | 0.98 | 570,976 | 1.00 | 1 | 0.0% | 0.0% | emitted records (output grows with N) out=100/1000/10000/100000 |
| ordered-wide / cull-10-dispersed | 82,857 | 280,487 | 0.53 | 1,919,466 | 0.84 | 17,668,106 | 0.96 | 1 | 0.0% | 0.0% | emitted records (output grows with N) out=103/1000/10000/100000 |
| ordered-wide / cull-100-band | 5,928 | 57,366 | 0.99 | 570,470 | 1.00 | 5,702,180 | 1.00 | 1 | 0.0% | 0.0% | emitted records (output grows with N) out=999/9999/99999/999999 |
| ordered-wide / cull-100-dispersed | 5,928 | 57,366 | 0.99 | 570,470 | 1.00 | 5,702,180 | 1.00 | 1 | 0.0% | 0.0% | emitted records (output grows with N) out=999/9999/99999/999999 |
| ordered-wide / hit-present | 325 | 445 | 0.14 | 483 | 0.04 | 988 | **0.31** | 0 | 0.0% | 0.0% | log N |
| ordered-wide / hit-gap | 306 | 378 | 0.09 | 416 | 0.04 | 813 | **0.29** | 0 | 0.0% | 0.0% | log N |
| interleaved-wide / cull-1-band | 525 | 3,682 | 0.85 | 33,512 | 0.96 | 332,305 | 1.00 | 1 | 0.0% | 0.0% | emitted records (output grows with N) out=8/99/999/9999 |
| interleaved-wide / cull-1-dispersed | 48,299 | 458,093 | 0.98 | 4,577,738 | 1.00 | 45,776,127 | 1.00 | 1 | 0.0% | 0.0% | emitted records (output grows with N) out=100/100/1000/10000 |
| interleaved-wide / cull-10-band | 867 | 8,802 | 1.01 | 84,830 | 0.98 | 845,780 | 1.00 | 1 | 0.0% | 0.0% | emitted records (output grows with N) out=75/999/9999/99999 |
| interleaved-wide / cull-10-dispersed | 88,079 | 873,209 | 1.00 | 8,728,898 | 1.00 | 87,288,896 | 1.00 | 1 | 0.0% | 0.0% | emitted records (output grows with N) out=999/1000/10000/100000 |
| interleaved-wide / cull-100-band | 5,928 | 57,366 | 0.99 | 570,470 | 1.00 | 5,702,180 | 1.00 | 1 | 0.0% | 0.0% | emitted records (output grows with N) out=999/9999/99999/999999 |
| interleaved-wide / cull-100-dispersed | 5,928 | 57,366 | 0.99 | 570,470 | 1.00 | 5,702,180 | 1.00 | 1 | 0.0% | 0.0% | emitted records (output grows with N) out=999/9999/99999/999999 |
| interleaved-wide / hit-present | 299 | 1,466 | **0.69** | 1,774 | 0.08 | 1,527 | -0.07 | 0 | 0.0% | 0.0% | log N |
| interleaved-wide / hit-gap | 306 | 1,473 | **0.68** | 12,713 | **0.94** | 123,870 | **0.99** | 0 | 0.0% | 0.0% | log N |

