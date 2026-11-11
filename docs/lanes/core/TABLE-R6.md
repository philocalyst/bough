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
| ordered-wide / insert8-front | 8,605 | 16,483 | **0.28** | 73,925 | **0.65** | 821,534 | **1.05** | 0 | 21.2% | 9.2% | K + log N |
| ordered-wide / remove8-front | 1,556 | 1,850 | 0.08 | 1,722 | -0.03 | 2,516 | 0.16 | 0 | 1.8% | 0.1% | K + log N |
| ordered-wide / move8-front | 3,698 | 4,058 | 0.04 | 3,979 | -0.01 | 385,874 | **1.99** | 0 | 11.5% | 10.0% | K + log N |
| ordered-wide / insert8-middle | 8,202 | 15,657 | **0.28** | 61,001 | **0.59** | 516,071 | **0.93** | 0 | 4.2% | 6.9% | K + log N |
| ordered-wide / remove8-middle | 1,155 | 1,781 | 0.19 | 1,549 | -0.06 | 1,867 | 0.08 | 0 | 2.1% | 0.1% | K + log N |
| ordered-wide / move8-middle | 10,846 | 19,882 | **0.26** | 76,491 | **0.59** | 634,813 | **0.92** | 0 | 8.8% | 7.1% | K + log N |
| ordered-wide / insert8-end | 1,057 | 1,091 | 0.01 | 1,115 | 0.01 | 392,909 | **2.55** | 0 | 4.9% | 3.9% | K + log N |
| ordered-wide / remove8-end | 824 | 1,042 | 0.10 | 878 | -0.07 | 853 | -0.01 | 0 | 3.4% | 0.1% | K + log N |
| ordered-wide / move8-end | 10,515 | 19,144 | **0.26** | 75,822 | **0.60** | 647,621 | **0.93** | 0 | 5.5% | 5.2% | K + log N |
| ordered-wide / insert64-front | 11,529 | 19,907 | 0.24 | 76,725 | **0.59** | 666,426 | **0.94** | 0 | 8.9% | 9.9% | K + log N |
| ordered-wide / remove64-front | 4,892 | 9,974 | **0.31** | 59,967 | **0.78** | 582,239 | **0.99** | 0 | 5.8% | 6.1% | K + log N |
| ordered-wide / move64-front | 17,119 | 25,272 | 0.17 | 102,145 | **0.61** | 892,638 | **0.94** | 0 | 2.8% | 10.3% | K + log N |
| ordered-wide / insert64-middle | 11,107 | 18,604 | 0.22 | 64,187 | **0.54** | 532,778 | **0.92** | 0 | 4.5% | 3.6% | K + log N |
| ordered-wide / remove64-middle | 2,605 | 2,890 | 0.05 | 3,080 | 0.03 | 3,546 | 0.06 | 0 | 0.1% | 0.0% | K + log N |
| ordered-wide / move64-middle | 20,243 | 29,223 | 0.16 | 86,224 | **0.47** | 676,414 | **0.89** | 0 | 1.4% | 8.0% | K + log N |
| ordered-wide / insert64-end | 10,777 | 16,991 | 0.20 | 51,103 | **0.48** | 394,765 | **0.89** | 0 | 4.9% | 2.4% | K + log N |
| ordered-wide / remove64-end | 2,258 | 5,088 | **0.35** | 31,811 | **0.80** | 299,396 | **0.97** | 0 | 0.1% | 0.0% | K + log N |
| ordered-wide / move64-end | 15,153 | 23,316 | 0.19 | 99,710 | **0.63** | 865,511 | **0.94** | 0 | 10.5% | 0.8% | K + log N |
| ordered-wide / append1 | 546 | 578 | 0.02 | 603 | 0.02 | 394,950 | **2.82** | 0 | 6.6% | 5.7% | K + log N |
| ordered-wide / translate64 | 1,455 | 1,489 | 0.01 | 1,516 | 0.01 | 1,539 | 0.01 | 0 | 2.2% | 0.2% | K + log N |
| ordered-wide / translate-half | 2,502 | 10,553 | 0.63 | 88,274 | 0.92 | 865,068 | 0.99 | 1 | 0.3% | 0.0% | pages moved (N/128); 0 with lazy range tags |
| ordered-wide / bulk-build | 98,037 | 856,272 | 0.94 | 8,322,979 | 0.99 | 84,855,013 | 1.01 | 1 | 1.0% | 0.8% | N |
| ordered-wide / build-append8 | 199,251 | 2,133,923 | 1.03 | 48,737,778 | **1.36** | 3,229,145,339 | **1.82** | 1 | 0.1% | 0.0% | N |
| ordered-wide / freeze-cold | 26,946 | 229,677 | 0.93 | 2,159,700 | 0.97 | 24,561,364 | 1.06 | 1 | 7.3% | 5.8% | N |
| ordered-wide / freeze-idle | 1,494 | 5,741 | **0.58** | 48,148 | **0.92** | 472,435 | **0.99** | 0 | 105.2% | 51.1% | changed pages |
| ordered-wide / freeze-one | 1,794 | 6,041 | **0.53** | 48,446 | **0.90** | 472,733 | **0.99** | 0 | 7.5% | 0.7% | changed pages |
| ordered-wide / freeze-all | 6,435 | 51,572 | 0.90 | 501,716 | 0.99 | 5,003,984 | 1.00 | 1 | 0.6% | 0.6% | N |
| ordered-wide / parent-last | 229 | 364 | 0.20 | 1,800 | **0.69** | 15,900 | **0.95** | 0 | 0.0% | 0.1% | log N |
| ordered-wide / ancestors-last | 295 | 430 | 0.16 | 1,866 | **0.64** | 15,965 | **0.93** | 0 | 0.0% | 0.0% | log N |
| ordered-wide / subtree-root | 205 | 382 | **0.27** | 1,978 | **0.71** | 17,812 | **0.95** | 0 | 0.0% | 0.0% | log N |
| ordered-wide / subtree-leaf | 114 | 171 | 0.18 | 179 | 0.02 | 153 | -0.07 | 0 | 0.0% | 0.0% | log N |
| ordered-chains32 / parent-last | 88 | 121 | 0.14 | 145 | 0.08 | 161 | 0.05 | 0 | 0.0% | 0.0% | log N |
| ordered-chains32 / ancestors-last | 475 | 837 | 0.25 | 2,737 | **0.51** | 16,841 | **0.79** | 0 | 0.0% | 0.0% | log N |
| ordered-chains32 / subtree-root | 205 | 382 | **0.27** | 1,978 | **0.71** | 17,815 | **0.95** | 0 | 0.4% | 0.0% | log N |
| ordered-chains32 / subtree-leaf | 114 | 171 | 0.18 | 179 | 0.02 | 153 | -0.07 | 0 | 0.0% | 0.0% | log N |
| ordered-wide / set-ink-pos | 421 | 454 | 0.03 | 478 | 0.02 | 502 | 0.02 | 0 | 0.0% | 0.0% | log N |
| ordered-wide / set-ink-handle | 387 | 387 | 0.00 | 387 | 0.00 | 387 | 0.00 | 0 | 0.0% | 0.0% | log N |
| ordered-wide / pos-of-handle | 24 | 24 | 0.00 | 24 | 0.00 | 24 | 0.00 | 0 | 0.0% | 0.0% | log N |
| ordered-wide / touch-handle | 131 | 131 | 0.00 | 131 | 0.00 | 131 | 0.00 | 0 | 0.0% | 0.0% | log N |
| ordered-wide / set-inks-handles64 | 19,763 | 24,688 | 0.10 | 29,348 | 0.08 | 37,580 | 0.11 | 0 | 0.1% | 0.0% | 64 writes |
| ordered-wide / insert8-front+pos64 | 10,757 | 18,646 | 0.24 | 74,575 | **0.60** | 646,455 | **0.94** | 0 | 7.1% | 4.9% | K + log N |
| ordered-wide / insert8-front+posall | 9,252 | 20,565 | 0.35 | 115,234 | 0.75 | 1,059,159 | 0.96 | 1 | 2.7% | 3.0% | one lookup per page |

## Structure, warm columns

| shape / operation | 1e3 | 1e4 | e | 1e5 | e | 1e6 | e | ideal e | spread | noise | ideal bound |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|
| ordered-wide / insert8-front | 3,961 | 8,873 | **0.35** | 58,698 | **0.82** | 556,842 | **0.98** | 0 | 4.3% | 24.7% | K + log N |
| ordered-wide / remove8-front | 1,556 | 1,851 | 0.08 | 1,722 | -0.03 | 2,516 | 0.16 | 0 | 0.1% | 0.0% | K + log N |
| ordered-wide / move8-front | 3,700 | 4,060 | 0.04 | 3,979 | -0.01 | 304,701 | **1.88** | 0 | 0.1% | 122.5% | K + log N |
| ordered-wide / insert8-middle | 3,557 | 7,545 | **0.33** | 45,860 | **0.78** | 428,952 | **0.97** | 0 | 0.1% | 0.1% | K + log N |
| ordered-wide / remove8-middle | 1,156 | 1,782 | 0.19 | 1,549 | -0.06 | 1,867 | 0.08 | 0 | 0.2% | 0.0% | K + log N |
| ordered-wide / move8-middle | 6,202 | 11,770 | **0.28** | 61,387 | **0.72** | 559,875 | **0.96** | 0 | 0.1% | 0.4% | K + log N |
| ordered-wide / insert8-end | 1,058 | 1,090 | 0.01 | 1,115 | 0.01 | 301,020 | **2.43** | 0 | 0.3% | 0.1% | K + log N |
| ordered-wide / remove8-end | 823 | 1,042 | 0.10 | 878 | -0.07 | 854 | -0.01 | 0 | 0.3% | 0.1% | K + log N |
| ordered-wide / move8-end | 5,870 | 11,032 | **0.27** | 60,716 | **0.74** | 558,855 | **0.96** | 0 | 0.1% | 0.0% | K + log N |
| ordered-wide / insert64-front | 6,884 | 11,793 | 0.23 | 61,620 | **0.72** | 559,766 | **0.96** | 0 | 0.0% | 0.0% | K + log N |
| ordered-wide / remove64-front | 4,714 | 9,793 | **0.32** | 59,788 | **0.79** | 559,450 | **0.97** | 0 | 0.1% | 0.0% | K + log N |
| ordered-wide / move64-front | 16,941 | 24,912 | 0.17 | 101,982 | **0.61** | 869,755 | **0.93** | 0 | 1.0% | 1.1% | K + log N |
| ordered-wide / insert64-middle | 6,462 | 10,493 | 0.21 | 48,973 | **0.67** | 432,357 | **0.95** | 0 | 0.0% | 0.0% | K + log N |
| ordered-wide / remove64-middle | 2,605 | 2,890 | 0.05 | 3,080 | 0.03 | 3,545 | 0.06 | 0 | 0.1% | 0.0% | K + log N |
| ordered-wide / move64-middle | 15,790 | 20,935 | 0.12 | 70,927 | **0.53** | 569,474 | **0.90** | 0 | 1.3% | 1.7% | K + log N |
| ordered-wide / insert64-end | 6,141 | 8,886 | 0.16 | 35,888 | **0.61** | 303,997 | **0.93** | 0 | 0.4% | 0.0% | K + log N |
| ordered-wide / remove64-end | 2,080 | 4,909 | **0.37** | 31,633 | **0.81** | 299,218 | **0.98** | 0 | 0.2% | 0.0% | K + log N |
| ordered-wide / move64-end | 15,163 | 22,972 | 0.18 | 99,420 | **0.64** | 865,046 | **0.94** | 0 | 1.4% | 0.4% | K + log N |
| ordered-wide / append1 | 546 | 577 | 0.02 | 602 | 0.02 | 300,808 | **2.70** | 0 | 7.6% | 0.3% | K + log N |
| ordered-wide / translate64 | 1,457 | 1,491 | 0.01 | 1,516 | 0.01 | 1,540 | 0.01 | 0 | 2.5% | 0.1% | K + log N |
| ordered-wide / translate-half | 2,503 | 10,555 | 0.62 | 88,272 | 0.92 | 865,068 | 0.99 | 1 | 0.4% | 0.6% | pages moved (N/128); 0 with lazy range tags |
| ordered-wide / bulk-build | 97,939 | 856,254 | 0.94 | 8,322,841 | 0.99 | 84,676,512 | 1.01 | 1 | 1.5% | 1.3% | N |
| ordered-wide / build-append8 | 199,382 | 2,134,073 | 1.03 | 48,766,902 | **1.36** | 3,228,779,143 | **1.82** | 1 | 0.1% | 0.1% | N |
| ordered-wide / freeze-cold | 26,957 | 229,433 | 0.93 | 2,158,958 | 0.97 | 23,948,207 | 1.05 | 1 | 5.7% | 3.4% | N |
| ordered-wide / freeze-idle | 1,497 | 5,744 | **0.58** | 48,148 | **0.92** | 480,469 | **1.00** | 0 | 41.1% | 39.8% | changed pages |
| ordered-wide / freeze-one | 1,795 | 6,042 | **0.53** | 48,446 | **0.90** | 561,116 | **1.06** | 0 | 14.0% | 12.0% | changed pages |
| ordered-wide / freeze-all | 6,436 | 51,572 | 0.90 | 501,717 | 0.99 | 5,003,984 | 1.00 | 1 | 0.2% | 1.1% | N |
| ordered-wide / parent-last | 229 | 364 | 0.20 | 1,800 | **0.69** | 15,900 | **0.95** | 0 | 0.0% | 0.0% | log N |
| ordered-wide / ancestors-last | 295 | 430 | 0.16 | 1,866 | **0.64** | 15,965 | **0.93** | 0 | 0.0% | 0.0% | log N |
| ordered-wide / subtree-root | 205 | 382 | **0.27** | 1,978 | **0.71** | 17,812 | **0.95** | 0 | 0.0% | 0.0% | log N |
| ordered-wide / subtree-leaf | 114 | 171 | 0.18 | 179 | 0.02 | 153 | -0.07 | 0 | 0.0% | 0.0% | log N |
| ordered-chains32 / parent-last | 88 | 121 | 0.14 | 145 | 0.08 | 161 | 0.05 | 0 | 0.0% | 0.0% | log N |
| ordered-chains32 / ancestors-last | 475 | 837 | 0.25 | 2,737 | **0.51** | 16,841 | **0.79** | 0 | 0.0% | 0.0% | log N |
| ordered-chains32 / subtree-root | 205 | 382 | **0.27** | 1,978 | **0.71** | 17,812 | **0.95** | 0 | 0.0% | 0.0% | log N |
| ordered-chains32 / subtree-leaf | 114 | 171 | 0.18 | 179 | 0.02 | 153 | -0.07 | 0 | 0.0% | 0.0% | log N |
| ordered-wide / set-ink-pos | 421 | 454 | 0.03 | 478 | 0.02 | 502 | 0.02 | 0 | 0.0% | 0.0% | log N |
| ordered-wide / set-ink-handle | 387 | 387 | 0.00 | 387 | -0.00 | 387 | 0.00 | 0 | 0.0% | 0.0% | log N |
| ordered-wide / pos-of-handle | 24 | 24 | -0.00 | 24 | 0.00 | 24 | 0.00 | 0 | 0.0% | 0.0% | log N |
| ordered-wide / touch-handle | 131 | 131 | 0.00 | 131 | 0.00 | 131 | 0.00 | 0 | 0.0% | 0.0% | log N |
| ordered-wide / set-inks-handles64 | 19,763 | 24,688 | 0.10 | 29,348 | 0.08 | 37,580 | 0.11 | 0 | 0.1% | 0.0% | 64 writes |
| ordered-wide / insert8-front+pos64 | 6,106 | 10,779 | 0.25 | 60,604 | **0.75** | 558,752 | **0.96** | 0 | 0.0% | 0.0% | K + log N |
| ordered-wide / insert8-front+posall | 4,609 | 13,231 | 0.46 | 99,968 | 0.88 | 967,238 | 0.99 | 1 | 0.8% | 0.0% | one lookup per page |

## Spatial

| shape / operation | 1e3 | 1e4 | e | 1e5 | e | 1e6 | e | ideal e | spread | noise | ideal bound |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|
| ordered-wide / cull-1-band | 369 | 903 | 0.39 | 6,060 | 0.83 | 57,860 | 0.98 | 1 | 0.0% | 0.0% | emitted records (output grows with N) out=10/100/1000/10000 |
| ordered-wide / cull-1-dispersed | 41,205 | 200,236 | 0.69 | 1,781,100 | 0.95 | 17,530,978 | 0.99 | 1 | 0.1% | 0.0% | emitted records (output grows with N) out=11/100/1000/10000 |
| ordered-wide / cull-10-band | 846 | 6,023 | 0.85 | 57,371 | 0.98 | 570,976 | 1.00 | 1 | 0.0% | 0.0% | emitted records (output grows with N) out=100/1000/10000/100000 |
| ordered-wide / cull-10-dispersed | 82,857 | 280,487 | 0.53 | 1,919,466 | 0.84 | 17,673,114 | 0.96 | 1 | 0.1% | 0.0% | emitted records (output grows with N) out=103/1000/10000/100000 |
| ordered-wide / cull-100-band | 5,928 | 57,366 | 0.99 | 570,470 | 1.00 | 5,702,180 | 1.00 | 1 | 0.0% | 0.0% | emitted records (output grows with N) out=999/9999/99999/999999 |
| ordered-wide / cull-100-dispersed | 5,928 | 57,366 | 0.99 | 570,470 | 1.00 | 5,702,180 | 1.00 | 1 | 0.4% | 0.0% | emitted records (output grows with N) out=999/9999/99999/999999 |
| ordered-wide / hit-present | 325 | 445 | 0.14 | 483 | 0.04 | 988 | **0.31** | 0 | 0.0% | 0.0% | log N |
| ordered-wide / hit-gap | 306 | 378 | 0.09 | 416 | 0.04 | 813 | **0.29** | 0 | 0.0% | 0.0% | log N |
| interleaved-wide / cull-1-band | 525 | 3,682 | 0.85 | 33,512 | 0.96 | 332,305 | 1.00 | 1 | 0.0% | 0.0% | emitted records (output grows with N) out=8/99/999/9999 |
| interleaved-wide / cull-1-dispersed | 48,299 | 458,093 | 0.98 | 4,577,738 | 1.00 | 45,777,455 | 1.00 | 1 | 0.0% | 0.0% | emitted records (output grows with N) out=100/100/1000/10000 |
| interleaved-wide / cull-10-band | 867 | 8,802 | 1.01 | 84,830 | 0.98 | 845,780 | 1.00 | 1 | 0.0% | 0.0% | emitted records (output grows with N) out=75/999/9999/99999 |
| interleaved-wide / cull-10-dispersed | 88,079 | 873,209 | 1.00 | 8,728,898 | 1.00 | 87,297,494 | 1.00 | 1 | 0.1% | 0.0% | emitted records (output grows with N) out=999/1000/10000/100000 |
| interleaved-wide / cull-100-band | 5,928 | 57,366 | 0.99 | 570,470 | 1.00 | 5,703,189 | 1.00 | 1 | 0.1% | 0.1% | emitted records (output grows with N) out=999/9999/99999/999999 |
| interleaved-wide / cull-100-dispersed | 5,928 | 57,366 | 0.99 | 570,470 | 1.00 | 5,702,180 | 1.00 | 1 | 0.0% | 0.0% | emitted records (output grows with N) out=999/9999/99999/999999 |
| interleaved-wide / hit-present | 299 | 1,466 | **0.69** | 1,774 | 0.08 | 1,527 | -0.07 | 0 | 0.0% | 0.0% | log N |
| interleaved-wide / hit-gap | 306 | 1,473 | **0.68** | 12,713 | **0.94** | 123,873 | **0.99** | 0 | 0.1% | 0.1% | log N |

