# R6 instruction matrix: `g1`

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
| ordered-wide / insert8-front | 4,832 | 4,900 | 0.01 | 5,013 | 0.01 | 5,497 | 0.04 | 0 | 4.2% | 0.9% | K + log N |
| ordered-wide / remove8-front | 1,611 | 1,847 | 0.06 | 1,707 | -0.03 | 2,180 | 0.11 | 0 | 2.4% | 0.0% | K + log N |
| ordered-wide / move8-front | 3,618 | 3,887 | 0.03 | 3,798 | -0.01 | 6,856 | **0.26** | 0 | 1.8% | 0.1% | K + log N |
| ordered-wide / insert8-middle | 4,447 | 4,551 | 0.01 | 4,359 | -0.02 | 4,639 | 0.03 | 0 | 6.2% | 0.0% | K + log N |
| ordered-wide / remove8-middle | 1,133 | 1,732 | 0.18 | 1,487 | -0.07 | 1,656 | 0.05 | 0 | 4.1% | 0.1% | K + log N |
| ordered-wide / move8-middle | 6,937 | 7,619 | 0.04 | 7,554 | -0.00 | 8,185 | 0.03 | 0 | 0.6% | 0.1% | K + log N |
| ordered-wide / insert8-end | 1,036 | 1,055 | 0.01 | 1,080 | 0.01 | 3,635 | **0.53** | 0 | 5.0% | 0.1% | K + log N |
| ordered-wide / remove8-end | 851 | 1,073 | 0.10 | 889 | -0.08 | 881 | -0.00 | 0 | 0.6% | 0.0% | K + log N |
| ordered-wide / move8-end | 6,653 | 6,959 | 0.02 | 6,956 | -0.00 | 7,408 | 0.03 | 0 | 0.7% | 0.1% | K + log N |
| ordered-wide / insert64-front | 7,554 | 7,623 | 0.00 | 7,778 | 0.01 | 8,219 | 0.02 | 0 | 0.6% | 0.0% | K + log N |
| ordered-wide / remove64-front | 5,152 | 6,449 | 0.10 | 6,666 | 0.01 | 8,021 | 0.08 | 0 | 0.7% | 0.1% | K + log N |
| ordered-wide / move64-front | 15,485 | 17,286 | 0.05 | 17,943 | 0.02 | 18,912 | 0.02 | 0 | 1.3% | 1.8% | K + log N |
| ordered-wide / insert64-middle | 7,208 | 7,300 | 0.01 | 7,238 | -0.00 | 7,951 | 0.04 | 0 | 0.7% | 0.0% | K + log N |
| ordered-wide / remove64-middle | 2,570 | 2,861 | 0.05 | 2,894 | 0.00 | 3,092 | 0.03 | 0 | 0.4% | 0.0% | K + log N |
| ordered-wide / move64-middle | 15,462 | 15,778 | 0.01 | 16,049 | 0.01 | 16,706 | 0.02 | 0 | 1.3% | 0.7% | K + log N |
| ordered-wide / insert64-end | 6,904 | 6,554 | -0.02 | 6,895 | 0.02 | 6,467 | -0.03 | 0 | 1.7% | 0.0% | K + log N |
| ordered-wide / remove64-end | 2,316 | 2,756 | 0.08 | 2,672 | -0.01 | 1,837 | -0.16 | 0 | 0.2% | 0.0% | K + log N |
| ordered-wide / move64-end | 13,506 | 14,686 | 0.04 | 14,816 | 0.00 | 14,596 | -0.01 | 0 | 1.5% | 0.7% | K + log N |
| ordered-wide / append1 | 539 | 558 | 0.02 | 582 | 0.02 | 3,445 | **0.77** | 0 | 0.9% | 0.0% | K + log N |
| ordered-wide / translate64 | 1,467 | 1,463 | -0.00 | 1,495 | 0.01 | 1,527 | 0.01 | 0 | 0.3% | 0.1% | K + log N |
| ordered-wide / translate-half | 2,071 | 4,885 | 0.37 | 29,208 | 0.78 | 272,654 | 0.97 | 1 | 0.1% | 0.0% | pages moved (N/128); 0 with lazy range tags |
| ordered-wide / bulk-build | 93,402 | 838,053 | 0.95 | 8,140,832 | 0.99 | 83,449,180 | 1.01 | 1 | 1.2% | 0.7% | N |
| ordered-wide / build-append8 | 180,239 | 1,736,541 | 0.98 | 17,503,671 | 1.00 | 178,902,411 | 1.01 | 1 | 0.7% | 0.3% | N |
| ordered-wide / freeze-cold | 27,650 | 225,067 | 0.91 | 2,151,721 | 0.98 | 25,538,060 | 1.07 | 1 | 3.1% | 0.7% | N |
| ordered-wide / freeze-idle | 754 | 753 | -0.00 | 754 | 0.00 | 754 | 0.00 | 0 | 0.4% | 0.1% | changed pages |
| ordered-wide / freeze-one | 2,176 | 6,478 | **0.47** | 50,340 | **0.89** | 488,067 | **0.99** | 0 | 0.2% | 0.3% | changed pages |
| ordered-wide / freeze-all | 6,817 | 52,010 | 0.88 | 503,611 | 0.99 | 5,019,318 | 1.00 | 1 | 0.0% | 0.0% | N |
| ordered-wide / parent-last | 222 | 352 | 0.20 | 410 | 0.07 | 743 | **0.26** | 0 | 0.0% | 0.0% | log N |
| ordered-wide / ancestors-last | 288 | 416 | 0.16 | 474 | 0.06 | 806 | 0.23 | 0 | 0.0% | 0.0% | log N |
| ordered-wide / subtree-root | 210 | 261 | 0.09 | 336 | 0.11 | 671 | **0.30** | 0 | 0.0% | 0.0% | log N |
| ordered-wide / subtree-leaf | 116 | 157 | 0.13 | 165 | 0.02 | 155 | -0.03 | 0 | 0.0% | 0.0% | log N |
| ordered-chains32 / parent-last | 77 | 94 | 0.09 | 118 | 0.10 | 150 | 0.10 | 0 | 0.0% | 0.0% | log N |
| ordered-chains32 / ancestors-last | 474 | 838 | 0.25 | 1,376 | 0.22 | 1,712 | 0.09 | 0 | 0.0% | 0.0% | log N |
| ordered-chains32 / subtree-root | 210 | 261 | 0.09 | 336 | 0.11 | 671 | **0.30** | 0 | 0.0% | 0.0% | log N |
| ordered-chains32 / subtree-leaf | 116 | 157 | 0.13 | 165 | 0.02 | 155 | -0.03 | 0 | 0.0% | 0.0% | log N |
| ordered-wide / set-ink-pos | 364 | 381 | 0.02 | 405 | 0.03 | 437 | 0.03 | 0 | 0.0% | 0.0% | log N |
| ordered-wide / set-ink-handle | 342 | 342 | 0.00 | 342 | 0.00 | 342 | 0.00 | 0 | 0.0% | 0.0% | log N |
| ordered-wide / pos-of-handle | 24 | 24 | 0.00 | 24 | 0.00 | 24 | 0.00 | 0 | 0.0% | 0.0% | log N |
| ordered-wide / touch-handle | 117 | 117 | 0.00 | 117 | 0.00 | 117 | 0.00 | 0 | 0.0% | 0.0% | log N |
| ordered-wide / set-inks-handles64 | 18,866 | 23,073 | 0.09 | 27,401 | 0.07 | 33,609 | 0.09 | 0 | 0.0% | 0.0% | 64 writes |
| ordered-wide / state-set-handle | 95 | 95 | 0.00 | 95 | 0.00 | 95 | 0.00 | 0 | 0.0% | 0.0% | log N |
| ordered-wide / state-flip-turn | 215 | 215 | 0.00 | 215 | 0.00 | 215 | 0.00 | 0 | 0.0% | 0.0% | log N |
| ordered-wide / state-run-half | 633 | 4,850 | 0.88 | 47,061 | 0.99 | 469,040 | 1.00 | 1 | 0.0% | 0.0% | one word per page (N/128) |
| ordered-wide / state-any-none | 328 | 2,601 | **0.90** | 25,121 | **0.98** | 250,160 | **1.00** | 0 | 0.0% | 0.0% | log N with directory nodes; one test per page today |
| ordered-wide / state-any-one | 335 | 2,608 | **0.89** | 25,128 | **0.98** | 250,176 | **1.00** | 0 | 0.0% | 0.0% | log N with directory nodes; one test per page today |
| ordered-wide / insert8-front+pos64 | 6,986 | 6,632 | -0.02 | 6,918 | 0.02 | 7,399 | 0.03 | 0 | 0.2% | 2.5% | K + log N |
| ordered-wide / insert8-front+posall | 5,495 | 9,248 | 0.23 | 46,282 | 0.70 | 415,884 | 0.95 | 1 | 1.8% | 0.0% | one lookup per page |
| ordered-wide / insert64-front-worst | 29,851 | 31,484 | 0.02 | 59,056 | **0.27** | 351,111 | **0.77** | 0 | 2.8% | 6.1% | K + log N |
| ordered-wide / insert64-front-amortised | 7,930 | 8,081 | 0.01 | 8,699 | 0.03 | 15,538 | **0.25** | 0 | 0.3% | 0.8% | K + log N |

## Structure, warm columns

| shape / operation | 1e3 | 1e4 | e | 1e5 | e | 1e6 | e | ideal e | spread | noise | ideal bound |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|
| ordered-wide / insert8-front | 3,215 | 3,890 | 0.08 | 3,999 | 0.01 | 4,472 | 0.05 | 0 | 0.0% | 0.0% | K + log N |
| ordered-wide / remove8-front | 1,611 | 1,847 | 0.06 | 1,707 | -0.03 | 2,180 | 0.11 | 0 | 0.1% | 0.0% | K + log N |
| ordered-wide / move8-front | 3,616 | 3,888 | 0.03 | 3,795 | -0.01 | 5,831 | 0.19 | 0 | 0.1% | 0.1% | K + log N |
| ordered-wide / insert8-middle | 2,830 | 3,540 | 0.10 | 3,302 | -0.03 | 3,614 | 0.04 | 0 | 0.0% | 0.0% | K + log N |
| ordered-wide / remove8-middle | 1,134 | 1,732 | 0.18 | 1,486 | -0.07 | 1,656 | 0.05 | 0 | 0.1% | 0.1% | K + log N |
| ordered-wide / move8-middle | 5,316 | 6,607 | 0.09 | 6,495 | -0.01 | 7,169 | 0.04 | 0 | 0.1% | 0.0% | K + log N |
| ordered-wide / insert8-end | 1,037 | 1,056 | 0.01 | 1,080 | 0.01 | 2,609 | **0.38** | 0 | 0.1% | 0.0% | K + log N |
| ordered-wide / remove8-end | 851 | 1,073 | 0.10 | 888 | -0.08 | 881 | -0.00 | 0 | 0.1% | 0.1% | K + log N |
| ordered-wide / move8-end | 5,034 | 5,948 | 0.07 | 5,897 | -0.00 | 6,393 | 0.04 | 0 | 0.1% | 0.0% | K + log N |
| ordered-wide / insert64-front | 5,936 | 6,612 | 0.05 | 6,721 | 0.01 | 7,194 | 0.03 | 0 | 0.0% | 0.0% | K + log N |
| ordered-wide / remove64-front | 4,973 | 6,270 | 0.10 | 6,484 | 0.01 | 7,840 | 0.08 | 0 | 0.1% | 0.0% | K + log N |
| ordered-wide / move64-front | 15,303 | 17,104 | 0.05 | 17,750 | 0.02 | 18,674 | 0.02 | 0 | 1.0% | 1.8% | K + log N |
| ordered-wide / insert64-middle | 5,590 | 6,290 | 0.05 | 6,181 | -0.01 | 6,926 | 0.05 | 0 | 0.0% | 0.0% | K + log N |
| ordered-wide / remove64-middle | 2,570 | 2,861 | 0.05 | 2,894 | 0.00 | 3,092 | 0.03 | 0 | 0.0% | 0.0% | K + log N |
| ordered-wide / move64-middle | 13,843 | 14,765 | 0.03 | 15,069 | 0.01 | 15,845 | 0.02 | 0 | 0.5% | 0.7% | K + log N |
| ordered-wide / insert64-end | 5,286 | 5,546 | 0.02 | 5,838 | 0.02 | 5,442 | -0.03 | 0 | 0.0% | 0.0% | K + log N |
| ordered-wide / remove64-end | 2,136 | 2,575 | 0.08 | 2,491 | -0.01 | 1,656 | -0.18 | 0 | 0.0% | 0.0% | K + log N |
| ordered-wide / move64-end | 13,326 | 14,504 | 0.04 | 14,713 | 0.01 | 14,323 | -0.01 | 0 | 1.1% | 0.8% | K + log N |
| ordered-wide / append1 | 538 | 557 | 0.02 | 582 | 0.02 | 2,420 | **0.62** | 0 | 0.2% | 0.2% | K + log N |
| ordered-wide / translate64 | 1,468 | 1,463 | -0.00 | 1,495 | 0.01 | 1,527 | 0.01 | 0 | 0.1% | 0.0% | K + log N |
| ordered-wide / translate-half | 2,071 | 4,886 | 0.37 | 29,208 | 0.78 | 272,654 | 0.97 | 1 | 0.0% | 0.0% | pages moved (N/128); 0 with lazy range tags |
| ordered-wide / bulk-build | 93,412 | 838,056 | 0.95 | 8,140,978 | 0.99 | 83,083,438 | 1.01 | 1 | 1.8% | 1.2% | N |
| ordered-wide / build-append8 | 180,231 | 1,736,547 | 0.98 | 17,504,047 | 1.00 | 178,022,784 | 1.01 | 1 | 0.6% | 0.4% | N |
| ordered-wide / freeze-cold | 27,650 | 225,066 | 0.91 | 2,151,941 | 0.98 | 24,773,138 | 1.06 | 1 | 5.7% | 3.7% | N |
| ordered-wide / freeze-idle | 753 | 754 | 0.00 | 754 | 0.00 | 754 | 0.00 | 0 | 0.1% | 0.1% | changed pages |
| ordered-wide / freeze-one | 2,176 | 6,479 | **0.47** | 50,339 | **0.89** | 489,674 | **0.99** | 0 | 0.3% | 0.4% | changed pages |
| ordered-wide / freeze-all | 6,817 | 52,009 | 0.88 | 503,610 | 0.99 | 5,022,532 | 1.00 | 1 | 0.1% | 0.1% | N |
| ordered-wide / parent-last | 222 | 352 | 0.20 | 410 | 0.07 | 743 | **0.26** | 0 | 0.0% | 0.0% | log N |
| ordered-wide / ancestors-last | 288 | 416 | 0.16 | 474 | 0.06 | 806 | 0.23 | 0 | 0.0% | 0.0% | log N |
| ordered-wide / subtree-root | 210 | 261 | 0.09 | 336 | 0.11 | 671 | **0.30** | 0 | 0.0% | 0.0% | log N |
| ordered-wide / subtree-leaf | 116 | 157 | 0.13 | 165 | 0.02 | 155 | -0.03 | 0 | 0.0% | 0.0% | log N |
| ordered-chains32 / parent-last | 77 | 94 | 0.09 | 118 | 0.10 | 150 | 0.10 | 0 | 0.0% | 0.0% | log N |
| ordered-chains32 / ancestors-last | 474 | 838 | 0.25 | 1,376 | 0.22 | 1,712 | 0.09 | 0 | 0.0% | 0.0% | log N |
| ordered-chains32 / subtree-root | 210 | 261 | 0.09 | 336 | 0.11 | 671 | **0.30** | 0 | 0.0% | 0.0% | log N |
| ordered-chains32 / subtree-leaf | 116 | 157 | 0.13 | 165 | 0.02 | 155 | -0.03 | 0 | 0.0% | 0.0% | log N |
| ordered-wide / set-ink-pos | 364 | 381 | 0.02 | 405 | 0.03 | 437 | 0.03 | 0 | 0.0% | 0.0% | log N |
| ordered-wide / set-ink-handle | 342 | 342 | 0.00 | 342 | 0.00 | 342 | 0.00 | 0 | 0.0% | 0.0% | log N |
| ordered-wide / pos-of-handle | 24 | 24 | 0.00 | 24 | 0.00 | 24 | -0.00 | 0 | 0.0% | 0.0% | log N |
| ordered-wide / touch-handle | 117 | 117 | 0.00 | 117 | 0.00 | 117 | 0.00 | 0 | 0.0% | 0.0% | log N |
| ordered-wide / set-inks-handles64 | 18,866 | 23,073 | 0.09 | 27,401 | 0.07 | 33,609 | 0.09 | 0 | 0.0% | 0.0% | 64 writes |
| ordered-wide / state-set-handle | 95 | 95 | 0.00 | 95 | 0.00 | 95 | 0.00 | 0 | 0.0% | 0.0% | log N |
| ordered-wide / state-flip-turn | 215 | 215 | 0.00 | 215 | 0.00 | 215 | 0.00 | 0 | 0.0% | 0.0% | log N |
| ordered-wide / state-run-half | 633 | 4,850 | 0.88 | 47,061 | 0.99 | 469,044 | 1.00 | 1 | 0.0% | 0.0% | one word per page (N/128) |
| ordered-wide / state-any-none | 328 | 2,601 | **0.90** | 25,121 | **0.98** | 250,160 | **1.00** | 0 | 0.0% | 0.0% | log N with directory nodes; one test per page today |
| ordered-wide / state-any-one | 335 | 2,608 | **0.89** | 25,128 | **0.98** | 250,167 | **1.00** | 0 | 0.0% | 0.0% | log N with directory nodes; one test per page today |
| ordered-wide / insert8-front+pos64 | 5,354 | 5,786 | 0.03 | 5,907 | 0.01 | 6,380 | 0.03 | 0 | 0.0% | 0.1% | K + log N |
| ordered-wide / insert8-front+posall | 3,863 | 8,236 | 0.33 | 45,270 | 0.74 | 414,866 | 0.96 | 1 | 0.0% | 0.0% | one lookup per page |
| ordered-wide / insert64-front-worst | 29,897 | 31,482 | 0.02 | 59,309 | **0.28** | 367,132 | **0.79** | 0 | 9.0% | 3.6% | K + log N |
| ordered-wide / insert64-front-amortised | 7,923 | 8,076 | 0.01 | 8,694 | 0.03 | 15,595 | **0.25** | 0 | 0.8% | 0.3% | K + log N |

## Spatial

| shape / operation | 1e3 | 1e4 | e | 1e5 | e | 1e6 | e | ideal e | spread | noise | ideal bound |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|
| ordered-wide / cull-1-band | 375 | 843 | 0.35 | 5,676 | 0.83 | 53,784 | 0.98 | 1 | 0.0% | 0.0% | emitted records (output grows with N) out=10/100/1000/10000 |
| ordered-wide / cull-1-dispersed | 40,361 | 192,584 | 0.68 | 1,707,732 | 0.95 | 16,799,559 | 0.99 | 1 | 0.0% | 0.0% | emitted records (output grows with N) out=11/100/1000/10000 |
| ordered-wide / cull-10-band | 823 | 5,598 | 0.83 | 53,314 | 0.98 | 530,580 | 1.00 | 1 | 0.0% | 0.0% | emitted records (output grows with N) out=100/1000/10000/100000 |
| ordered-wide / cull-10-dispersed | 81,924 | 271,967 | 0.52 | 1,844,245 | 0.83 | 16,936,072 | 0.96 | 1 | 0.0% | 0.0% | emitted records (output grows with N) out=103/1000/10000/100000 |
| ordered-wide / cull-100-band | 5,541 | 53,199 | 0.98 | 530,073 | 1.00 | 5,298,813 | 1.00 | 1 | 0.0% | 0.0% | emitted records (output grows with N) out=999/9999/99999/999999 |
| ordered-wide / cull-100-dispersed | 5,541 | 53,199 | 0.98 | 530,073 | 1.00 | 5,298,813 | 1.00 | 1 | 0.0% | 0.0% | emitted records (output grows with N) out=999/9999/99999/999999 |
| ordered-wide / hit-present | 342 | 395 | 0.06 | 463 | 0.07 | 918 | **0.30** | 0 | 0.0% | 0.0% | log N |
| ordered-wide / hit-gap | 319 | 339 | 0.03 | 407 | 0.08 | 756 | **0.27** | 0 | 0.0% | 0.0% | log N |
| interleaved-wide / cull-1-band | 494 | 3,033 | 0.79 | 28,331 | 0.97 | 281,188 | 1.00 | 1 | 0.0% | 0.0% | emitted records (output grows with N) out=8/99/999/9999 |
| interleaved-wide / cull-1-dispersed | 47,424 | 449,050 | 0.98 | 4,488,565 | 1.00 | 44,885,040 | 1.00 | 1 | 0.0% | 0.0% | emitted records (output grows with N) out=100/100/1000/10000 |
| interleaved-wide / cull-10-band | 827 | 7,788 | 0.97 | 75,963 | 0.99 | 757,713 | 1.00 | 1 | 0.0% | 0.0% | emitted records (output grows with N) out=75/999/9999/99999 |
| interleaved-wide / cull-10-dispersed | 87,119 | 863,279 | 1.00 | 8,630,855 | 1.00 | 86,307,942 | 1.00 | 1 | 0.0% | 0.0% | emitted records (output grows with N) out=999/1000/10000/100000 |
| interleaved-wide / cull-100-band | 5,541 | 53,199 | 0.98 | 530,073 | 1.00 | 5,298,813 | 1.00 | 1 | 0.0% | 0.0% | emitted records (output grows with N) out=999/9999/99999/999999 |
| interleaved-wide / cull-100-dispersed | 5,541 | 53,199 | 0.98 | 530,073 | 1.00 | 5,298,813 | 1.00 | 1 | 0.0% | 0.0% | emitted records (output grows with N) out=999/9999/99999/999999 |
| interleaved-wide / hit-present | 316 | 1,154 | **0.56** | 1,354 | 0.07 | 1,109 | -0.09 | 0 | 0.0% | 0.0% | log N |
| interleaved-wide / hit-gap | 319 | 1,157 | **0.56** | 10,256 | **0.95** | 100,088 | **0.99** | 0 | 0.0% | 0.0% | log N |

