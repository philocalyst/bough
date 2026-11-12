# `g1` against `st`

new / base per cell (values as in the matrix). **bold**: above 1.02x; `~`: above 1.02x but inside the cell's own scatter. Cells under 200 instructions
are counter-dominated: `+k` is the absolute difference in instructions.

## Structure, fresh tree

| shape / operation | 1e3 base | 1e3 new | 1e3 | 1e4 | 1e5 | 1e6 base | 1e6 new | 1e6 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ordered-wide / insert8-front | 8,449 | 4,832 | 0.572 | 0.330 | 0.091 | 475,140 | 5,497 | 0.012 |
| ordered-wide / remove8-front | 1,487 | 1,611 | **1.083** | **1.038** | **1.033** | 2,446 | 2,180 | 0.891 |
| ordered-wide / move8-front | 3,504 | 3,618 | **1.033** | 1.006 | 1.004 | 93,249 | 6,856 | 0.074 |
| ordered-wide / insert8-middle | 7,995 | 4,447 | 0.556 | 0.352 | 0.119 | 275,779 | 4,639 | 0.017 |
| ordered-wide / remove8-middle | 1,062 | 1,133 | **1.067** | **1.027** | **1.023** | 1,772 | 1,656 | 0.935 |
| ordered-wide / move8-middle | 10,458 | 6,937 | 0.663 | 0.435 | 0.131 | 464,858 | 8,185 | 0.018 |
| ordered-wide / insert8-end | 1,072 | 1,036 | 0.966 | 0.956 | 0.957 | 89,559 | 3,635 | 0.041 |
| ordered-wide / remove8-end | 779 | 851 | **1.092** | **1.077** | **1.069** | 808 | 881 | **1.090** |
| ordered-wide / move8-end | 10,174 | 6,653 | 0.654 | 0.414 | 0.122 | 464,002 | 7,408 | 0.016 |
| ordered-wide / insert64-front | 11,338 | 7,554 | 0.666 | 0.430 | 0.134 | 465,115 | 8,219 | 0.018 |
| ordered-wide / remove64-front | 4,660 | 5,152 | **1.106** | 0.796 | 0.160 | 377,943 | 8,021 | 0.021 |
| ordered-wide / move64-front | 15,450 | 15,485 | 1.002 | 0.908 | 0.337 | 388,997 | 18,912 | 0.049 |
| ordered-wide / insert64-middle | 10,923 | 7,208 | 0.660 | 0.461 | 0.182 | 279,473 | 7,951 | 0.028 |
| ordered-wide / remove64-middle | 2,484 | 2,570 | **1.035** | **1.034** | 0.978 | 3,424 | 3,092 | 0.903 |
| ordered-wide / move64-middle | 18,972 | 15,462 | 0.815 | 0.615 | 0.241 | 486,598 | 16,706 | 0.034 |
| ordered-wide / insert64-end | 10,555 | 6,904 | 0.654 | 0.479 | 0.329 | 105,564 | 6,467 | 0.061 |
| ordered-wide / remove64-end | 1,953 | 2,316 | **1.186** | **1.318** | **1.358** | 1,076 | 1,837 | **1.707** |
| ordered-wide / move64-end | 13,709 | 13,506 | 0.985 | 0.859 | 0.292 | 384,447 | 14,596 | 0.038 |
| ordered-wide / append1 | 560 | 539 | 0.963 | 0.943 | 0.943 | 89,510 | 3,445 | 0.038 |
| ordered-wide / translate64 | 1,453 | 1,467 | 1.010 | 0.985 | 0.989 | 1,535 | 1,527 | 0.995 |
| ordered-wide / translate-half | 2,038 | 2,071 | 1.016 | **1.024** | 1.005 | 271,499 | 272,654 | 1.004 |
| ordered-wide / bulk-build | 99,141 | 93,402 | 0.942 | 0.968 | 0.967 | 86,524,647 | 83,449,180 | 0.964 |
| ordered-wide / build-append8 | 199,352 | 180,239 | 0.904 | 0.935 | 0.943 | 188,316,844 | 178,902,411 | 0.950 |
| ordered-wide / freeze-cold | 26,980 | 27,650 | **1.025** | 0.980 | 0.996 | 24,533,566 | 25,538,060 | **1.041** |
| ordered-wide / freeze-idle | 1,509 | 754 | 0.500 | 0.131 | 0.016 | 472,446 | 754 | 0.002 |
| ordered-wide / freeze-one | 1,807 | 2,176 | **1.204** | **1.070** | **1.039** | 472,744 | 488,067 | **1.032** |
| ordered-wide / freeze-all | 6,448 | 6,817 | **1.057** | 1.008 | 1.004 | 5,003,995 | 5,019,318 | 1.003 |
| ordered-wide / parent-last | 229 | 222 | 0.969 | 0.967 | 0.228 | 15,900 | 743 | 0.047 |
| ordered-wide / ancestors-last | 295 | 288 | 0.976 | 0.967 | 0.254 | 15,965 | 806 | 0.050 |
| ordered-wide / subtree-root | 205 | 210 | **1.024** | 0.683 | 0.170 | 17,812 | 671 | 0.038 |
| ordered-wide / subtree-leaf | 114 | 116 | 1.018 (+2) | 0.918 (-14) | 0.922 (-14) | 153 | 155 | 1.013 (+2) |
| ordered-chains32 / parent-last | 88 | 77 | 0.875 (-11) | 0.777 (-27) | 0.814 (-27) | 161 | 150 | 0.932 (-11) |
| ordered-chains32 / ancestors-last | 475 | 474 | 0.998 | 1.001 | 0.503 | 16,841 | 1,712 | 0.102 |
| ordered-chains32 / subtree-root | 205 | 210 | **1.024** | 0.683 | 0.170 | 17,812 | 671 | 0.038 |
| ordered-chains32 / subtree-leaf | 114 | 116 | 1.018 (+2) | 0.918 (-14) | 0.922 (-14) | 153 | 155 | 1.013 (+2) |
| ordered-wide / set-ink-pos | 371 | 364 | 0.981 | 0.943 | 0.946 | 452 | 437 | 0.967 |
| ordered-wide / set-ink-handle | 337 | 342 | 1.015 | 1.015 | 1.015 | 337 | 342 | 1.015 |
| ordered-wide / pos-of-handle | 24 | 24 | 1.000 | 1.000 | 1.000 | 24 | 24 | 1.000 |
| ordered-wide / touch-handle | 117 | 117 | 1.000 | 1.000 | 1.000 | 117 | 117 | 1.000 |
| ordered-wide / set-inks-handles64 | 18,668 | 18,866 | 1.011 | 1.011 | 0.997 | 35,717 | 33,609 | 0.941 |
| ordered-wide / state-set-handle | 95 | 95 | 1.000 | 1.000 | 1.000 | 95 | 95 | 1.000 |
| ordered-wide / state-flip-turn | 215 | 215 | 1.000 | 1.000 | 1.000 | 215 | 215 | 1.000 |
| ordered-wide / state-run-half | 604 | 633 | **1.048** | **1.081** | **1.090** | 429,995 | 469,040 | **1.091** |
| ordered-wide / state-any-none | 259 | 328 | **1.266** | **1.411** | **1.449** | 172,044 | 250,160 | **1.454** |
| ordered-wide / state-any-one | 272 | 335 | **1.232** | **1.405** | **1.449** | 172,057 | 250,176 | **1.454** |
| ordered-wide / insert8-front+pos64 | 10,606 | 6,986 | 0.659 | 0.399 | 0.121 | 463,802 | 7,399 | 0.016 |
| ordered-wide / insert8-front+posall | 9,110 | 5,495 | 0.603 | 0.495 | 0.479 | 901,575 | 415,884 | 0.461 |

## Structure, warm columns

| shape / operation | 1e3 base | 1e3 new | 1e3 | 1e4 | 1e5 | 1e6 base | 1e6 new | 1e6 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ordered-wide / insert8-front | 3,810 | 3,215 | 0.844 | 0.550 | 0.099 | 375,241 | 4,472 | 0.012 |
| ordered-wide / remove8-front | 1,487 | 1,611 | **1.083** | **1.038** | **1.033** | 2,446 | 2,180 | 0.891 |
| ordered-wide / move8-front | 3,501 | 3,616 | **1.033** | 1.006 | 1.003 | 6,216 | 5,831 | 0.938 |
| ordered-wide / insert8-middle | 3,355 | 2,830 | 0.844 | 0.684 | 0.151 | 189,010 | 3,614 | 0.019 |
| ordered-wide / remove8-middle | 1,062 | 1,134 | **1.068** | **1.027** | **1.022** | 1,772 | 1,656 | 0.935 |
| ordered-wide / move8-middle | 5,817 | 5,316 | 0.914 | 0.678 | 0.151 | 378,037 | 7,169 | 0.019 |
| ordered-wide / insert8-end | 1,072 | 1,037 | 0.967 | 0.956 | 0.957 | 2,746 | 2,609 | 0.950 |
| ordered-wide / remove8-end | 779 | 851 | **1.092** | **1.077** | **1.067** | 807 | 881 | **1.092** |
| ordered-wide / move8-end | 5,534 | 5,034 | 0.910 | 0.657 | 0.139 | 377,067 | 6,393 | 0.017 |
| ordered-wide / insert64-front | 6,699 | 5,936 | 0.886 | 0.663 | 0.155 | 378,130 | 7,194 | 0.019 |
| ordered-wide / remove64-front | 4,481 | 4,973 | **1.110** | 0.792 | 0.156 | 377,764 | 7,840 | 0.021 |
| ordered-wide / move64-front | 15,263 | 15,303 | 1.003 | 0.910 | 0.335 | 388,833 | 18,674 | 0.048 |
| ordered-wide / insert64-middle | 6,284 | 5,590 | 0.890 | 0.778 | 0.247 | 192,488 | 6,926 | 0.036 |
| ordered-wide / remove64-middle | 2,484 | 2,570 | **1.035** | **1.034** | 0.978 | 3,424 | 3,092 | 0.903 |
| ordered-wide / move64-middle | 14,333 | 13,843 | 0.966 | 0.835 | 0.292 | 386,913 | 15,845 | 0.041 |
| ordered-wide / insert64-end | 5,924 | 5,286 | 0.892 | 0.936 | 0.943 | 5,746 | 5,442 | 0.947 |
| ordered-wide / remove64-end | 1,773 | 2,136 | **1.205** | **1.347** | **1.392** | 897 | 1,656 | **1.846** |
| ordered-wide / move64-end | 13,533 | 13,326 | 0.985 | 0.861 | 0.292 | 384,300 | 14,323 | 0.037 |
| ordered-wide / append1 | 559 | 538 | 0.962 | 0.939 | 0.943 | 2,533 | 2,420 | 0.955 |
| ordered-wide / translate64 | 1,452 | 1,468 | 1.011 | 0.984 | 0.989 | 1,535 | 1,527 | 0.995 |
| ordered-wide / translate-half | 2,039 | 2,071 | 1.016 | **1.024** | 1.005 | 271,499 | 272,654 | 1.004 |
| ordered-wide / bulk-build | 99,141 | 93,412 | 0.942 | 0.968 | 0.967 | 86,427,447 | 83,083,438 | 0.961 |
| ordered-wide / build-append8 | 199,350 | 180,231 | 0.904 | 0.935 | 0.943 | 189,464,368 | 178,022,784 | 0.940 |
| ordered-wide / freeze-cold | 26,984 | 27,650 | **1.025** | 0.981 | 0.996 | 24,554,834 | 24,773,138 | 1.009 |
| ordered-wide / freeze-idle | 1,509 | 753 | 0.499 | 0.131 | 0.016 | 472,446 | 754 | 0.002 |
| ordered-wide / freeze-one | 1,807 | 2,176 | **1.204** | **1.070** | **1.039** | 472,744 | 489,674 | **1.036** |
| ordered-wide / freeze-all | 6,448 | 6,817 | **1.057** | 1.008 | 1.004 | 5,003,995 | 5,022,532 | 1.004 |
| ordered-wide / parent-last | 229 | 222 | 0.969 | 0.967 | 0.228 | 15,900 | 743 | 0.047 |
| ordered-wide / ancestors-last | 295 | 288 | 0.976 | 0.967 | 0.254 | 15,965 | 806 | 0.050 |
| ordered-wide / subtree-root | 205 | 210 | **1.024** | 0.683 | 0.170 | 17,812 | 671 | 0.038 |
| ordered-wide / subtree-leaf | 114 | 116 | 1.018 (+2) | 0.918 (-14) | 0.922 (-14) | 153 | 155 | 1.013 (+2) |
| ordered-chains32 / parent-last | 88 | 77 | 0.875 (-11) | 0.777 (-27) | 0.814 (-27) | 161 | 150 | 0.932 (-11) |
| ordered-chains32 / ancestors-last | 475 | 474 | 0.998 | 1.001 | 0.503 | 16,841 | 1,712 | 0.102 |
| ordered-chains32 / subtree-root | 205 | 210 | **1.024** | 0.683 | 0.170 | 17,812 | 671 | 0.038 |
| ordered-chains32 / subtree-leaf | 114 | 116 | 1.018 (+2) | 0.918 (-14) | 0.922 (-14) | 153 | 155 | 1.013 (+2) |
| ordered-wide / set-ink-pos | 371 | 364 | 0.981 | 0.943 | 0.946 | 452 | 437 | 0.967 |
| ordered-wide / set-ink-handle | 337 | 342 | 1.015 | 1.015 | 1.015 | 337 | 342 | 1.015 |
| ordered-wide / pos-of-handle | 24 | 24 | 1.000 | 1.000 | 1.000 | 24 | 24 | 1.000 |
| ordered-wide / touch-handle | 117 | 117 | 1.000 | 1.000 | 1.000 | 117 | 117 | 1.000 |
| ordered-wide / set-inks-handles64 | 18,668 | 18,866 | 1.011 | 1.011 | 0.997 | 35,717 | 33,609 | 0.941 |
| ordered-wide / state-set-handle | 95 | 95 | 1.000 | 1.000 | 1.000 | 95 | 95 | 1.000 |
| ordered-wide / state-flip-turn | 215 | 215 | 1.000 | 1.000 | 1.000 | 215 | 215 | 1.000 |
| ordered-wide / state-run-half | 604 | 633 | **1.048** | **1.081** | **1.090** | 429,995 | 469,044 | **1.091** |
| ordered-wide / state-any-none | 259 | 328 | **1.266** | **1.411** | **1.449** | 172,044 | 250,160 | **1.454** |
| ordered-wide / state-any-one | 272 | 335 | **1.232** | **1.405** | **1.449** | 172,057 | 250,167 | **1.454** |
| ordered-wide / insert8-front+pos64 | 5,953 | 5,354 | 0.899 | 0.644 | 0.139 | 377,148 | 6,380 | 0.017 |
| ordered-wide / insert8-front+posall | 4,457 | 3,863 | 0.867 | 0.720 | 0.553 | 785,634 | 414,866 | 0.528 |

## Spatial

| shape / operation | 1e3 base | 1e3 new | 1e3 | 1e4 | 1e5 | 1e6 base | 1e6 new | 1e6 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ordered-wide / cull-1-band | 353 | 375 | **1.062** | 0.981 | 1.004 | 53,792 | 53,784 | 1.000 |
| ordered-wide / cull-1-dispersed | 40,471 | 40,361 | 0.997 | 1.006 | 1.009 | 16,639,050 | 16,799,559 | 1.010 |
| ordered-wide / cull-10-band | 801 | 823 | **1.027** | 0.997 | 1.000 | 530,372 | 530,580 | 1.000 |
| ordered-wide / cull-10-dispersed | 82,835 | 81,924 | 0.989 | 1.002 | 1.008 | 16,776,202 | 16,936,072 | 1.010 |
| ordered-wide / cull-100-band | 5,519 | 5,541 | 1.004 | 0.998 | 1.000 | 5,296,180 | 5,298,813 | 1.000 |
| ordered-wide / cull-100-dispersed | 5,519 | 5,541 | 1.004 | 0.998 | 1.000 | 5,296,180 | 5,298,813 | 1.000 |
| ordered-wide / hit-present | 303 | 342 | **1.129** | 0.968 | **1.038** | 949 | 918 | 0.967 |
| ordered-wide / hit-gap | 286 | 319 | **1.115** | 0.947 | **1.028** | 790 | 756 | 0.957 |
| interleaved-wide / cull-1-band | 472 | 494 | **1.047** | 0.969 | 1.007 | 278,555 | 281,188 | 1.009 |
| interleaved-wide / cull-1-dispersed | 47,596 | 47,424 | 0.996 | 0.996 | 0.996 | 45,050,979 | 44,885,040 | 0.996 |
| interleaved-wide / cull-10-band | 805 | 827 | **1.027** | 0.988 | 1.003 | 755,080 | 757,713 | 1.003 |
| interleaved-wide / cull-10-dispersed | 88,056 | 87,119 | 0.989 | 0.989 | 0.989 | 87,273,372 | 86,307,942 | 0.989 |
| interleaved-wide / cull-100-band | 5,519 | 5,541 | 1.004 | 0.998 | 1.000 | 5,296,180 | 5,298,813 | 1.000 |
| interleaved-wide / cull-100-dispersed | 5,519 | 5,541 | 1.004 | 0.998 | 1.000 | 5,296,180 | 5,298,813 | 1.000 |
| interleaved-wide / hit-present | 277 | 316 | **1.141** | 0.958 | 0.931 | 1,298 | 1,109 | 0.854 |
| interleaved-wide / hit-gap | 286 | 319 | **1.115** | 0.953 | **1.025** | 97,103 | 100,088 | **1.031** |

## Cells above 1.02x

* warm-core ordered-wide / remove64-end N=1,000,000: 897 -> 1,656 (1.846x)
* fresh-core ordered-wide / remove64-end N=1,000,000: 1,076 -> 1,837 (1.707x)
* fresh-core ordered-wide / state-any-none N=1,000,000: 172,044 -> 250,160 (1.454x)
* warm-core ordered-wide / state-any-none N=1,000,000: 172,044 -> 250,160 (1.454x)
* fresh-core ordered-wide / state-any-one N=1,000,000: 172,057 -> 250,176 (1.454x)
* warm-core ordered-wide / state-any-one N=1,000,000: 172,057 -> 250,167 (1.454x)
* warm-core ordered-wide / state-any-none N=100,000: 17,333 -> 25,121 (1.449x)
* fresh-core ordered-wide / state-any-none N=100,000: 17,333 -> 25,121 (1.449x)
* warm-core ordered-wide / state-any-one N=100,000: 17,346 -> 25,128 (1.449x)
* fresh-core ordered-wide / state-any-one N=100,000: 17,346 -> 25,128 (1.449x)
* warm-core ordered-wide / state-any-none N=10,000: 1,843 -> 2,601 (1.411x)
* fresh-core ordered-wide / state-any-none N=10,000: 1,843 -> 2,601 (1.411x)
* warm-core ordered-wide / state-any-one N=10,000: 1,856 -> 2,608 (1.405x)
* fresh-core ordered-wide / state-any-one N=10,000: 1,856 -> 2,608 (1.405x)
* warm-core ordered-wide / remove64-end N=100,000: 1,789 -> 2,491 (1.392x)
* fresh-core ordered-wide / remove64-end N=100,000: 1,968 -> 2,672 (1.358x)
* warm-core ordered-wide / remove64-end N=10,000: 1,911 -> 2,575 (1.347x)
* fresh-core ordered-wide / remove64-end N=10,000: 2,091 -> 2,756 (1.318x)
* warm-core ordered-wide / state-any-none N=1,000: 259 -> 328 (1.266x)
* fresh-core ordered-wide / state-any-none N=1,000: 259 -> 328 (1.266x)
* warm-core ordered-wide / state-any-one N=1,000: 272 -> 335 (1.232x)
* fresh-core ordered-wide / state-any-one N=1,000: 272 -> 335 (1.232x)
* warm-core ordered-wide / remove64-end N=1,000: 1,773 -> 2,136 (1.205x)
* warm-core ordered-wide / freeze-one N=1,000: 1,807 -> 2,176 (1.204x)
* fresh-core ordered-wide / freeze-one N=1,000: 1,807 -> 2,176 (1.204x)
* fresh-core ordered-wide / remove64-end N=1,000: 1,953 -> 2,316 (1.186x)
* spatial interleaved-wide / hit-present N=1,000: 277 -> 316 (1.141x)
* spatial ordered-wide / hit-present N=1,000: 303 -> 342 (1.129x)
* spatial ordered-wide / hit-gap N=1,000: 286 -> 319 (1.115x)
* spatial interleaved-wide / hit-gap N=1,000: 286 -> 319 (1.115x)
* warm-core ordered-wide / remove64-front N=1,000: 4,481 -> 4,973 (1.110x)
* fresh-core ordered-wide / remove64-front N=1,000: 4,660 -> 5,152 (1.106x)
* warm-core ordered-wide / remove8-end N=1,000: 779 -> 851 (1.092x)
* fresh-core ordered-wide / remove8-end N=1,000: 779 -> 851 (1.092x)
* warm-core ordered-wide / remove8-end N=1,000,000: 807 -> 881 (1.092x)
* warm-core ordered-wide / state-run-half N=1,000,000: 429,995 -> 469,044 (1.091x)
* fresh-core ordered-wide / state-run-half N=1,000,000: 429,995 -> 469,040 (1.091x)
* fresh-core ordered-wide / remove8-end N=1,000,000: 808 -> 881 (1.090x)
* warm-core ordered-wide / state-run-half N=100,000: 43,183 -> 47,061 (1.090x)
* fresh-core ordered-wide / state-run-half N=100,000: 43,183 -> 47,061 (1.090x)
* warm-core ordered-wide / remove8-front N=1,000: 1,487 -> 1,611 (1.083x)
* fresh-core ordered-wide / remove8-front N=1,000: 1,487 -> 1,611 (1.083x)
* warm-core ordered-wide / state-run-half N=10,000: 4,487 -> 4,850 (1.081x)
* fresh-core ordered-wide / state-run-half N=10,000: 4,487 -> 4,850 (1.081x)
* warm-core ordered-wide / remove8-end N=10,000: 996 -> 1,073 (1.077x)
* fresh-core ordered-wide / remove8-end N=10,000: 996 -> 1,073 (1.077x)
* warm-core ordered-wide / freeze-one N=10,000: 6,054 -> 6,479 (1.070x)
* fresh-core ordered-wide / freeze-one N=10,000: 6,054 -> 6,478 (1.070x)
* fresh-core ordered-wide / remove8-end N=100,000: 832 -> 889 (1.069x)
* warm-core ordered-wide / remove8-middle N=1,000: 1,062 -> 1,134 (1.068x)
* warm-core ordered-wide / remove8-end N=100,000: 832 -> 888 (1.067x)
* fresh-core ordered-wide / remove8-middle N=1,000: 1,062 -> 1,133 (1.067x)
* spatial ordered-wide / cull-1-band N=1,000: 353 -> 375 (1.062x)
* warm-core ordered-wide / freeze-all N=1,000: 6,448 -> 6,817 (1.057x)
* fresh-core ordered-wide / freeze-all N=1,000: 6,448 -> 6,817 (1.057x)
* warm-core ordered-wide / state-run-half N=1,000: 604 -> 633 (1.048x)
* fresh-core ordered-wide / state-run-half N=1,000: 604 -> 633 (1.048x)
* spatial interleaved-wide / cull-1-band N=1,000: 472 -> 494 (1.047x)
* fresh-core ordered-wide / freeze-cold N=1,000,000: 24,533,566 -> 25,538,060 (1.041x)
* fresh-core ordered-wide / freeze-one N=100,000: 48,458 -> 50,340 (1.039x)
* warm-core ordered-wide / freeze-one N=100,000: 48,458 -> 50,339 (1.039x)
* spatial ordered-wide / hit-present N=100,000: 446 -> 463 (1.038x)
* warm-core ordered-wide / remove8-front N=10,000: 1,780 -> 1,847 (1.038x)
* fresh-core ordered-wide / remove8-front N=10,000: 1,780 -> 1,847 (1.038x)
* warm-core ordered-wide / freeze-one N=1,000,000: 472,744 -> 489,674 (1.036x)
* warm-core ordered-wide / remove64-middle N=1,000: 2,484 -> 2,570 (1.035x)
* fresh-core ordered-wide / remove64-middle N=1,000: 2,484 -> 2,570 (1.035x)
* warm-core ordered-wide / remove64-middle N=10,000: 2,767 -> 2,861 (1.034x)
* fresh-core ordered-wide / remove64-middle N=10,000: 2,768 -> 2,861 (1.034x)
* warm-core ordered-wide / remove8-front N=100,000: 1,652 -> 1,707 (1.033x)
* fresh-core ordered-wide / remove8-front N=100,000: 1,652 -> 1,707 (1.033x)
* warm-core ordered-wide / move8-front N=1,000: 3,501 -> 3,616 (1.033x)
* fresh-core ordered-wide / move8-front N=1,000: 3,504 -> 3,618 (1.033x)
* fresh-core ordered-wide / freeze-one N=1,000,000: 472,744 -> 488,067 (1.032x)
* spatial interleaved-wide / hit-gap N=1,000,000: 97,103 -> 100,088 (1.031x)
* spatial ordered-wide / hit-gap N=100,000: 396 -> 407 (1.028x)
* spatial ordered-wide / cull-10-band N=1,000: 801 -> 823 (1.027x)
* spatial interleaved-wide / cull-10-band N=1,000: 805 -> 827 (1.027x)
* fresh-core ordered-wide / remove8-middle N=10,000: 1,686 -> 1,732 (1.027x)
* warm-core ordered-wide / remove8-middle N=10,000: 1,687 -> 1,732 (1.027x)
* spatial interleaved-wide / hit-gap N=100,000: 10,001 -> 10,256 (1.025x)
* fresh-core ordered-wide / freeze-cold N=1,000: 26,980 -> 27,650 (1.025x)
* warm-core ordered-wide / freeze-cold N=1,000: 26,984 -> 27,650 (1.025x)
* warm-core ordered-wide / subtree-root N=1,000: 205 -> 210 (1.024x)
* warm-core ordered-chains32 / subtree-root N=1,000: 205 -> 210 (1.024x)
* fresh-core ordered-wide / subtree-root N=1,000: 205 -> 210 (1.024x)
* fresh-core ordered-chains32 / subtree-root N=1,000: 205 -> 210 (1.024x)
* fresh-core ordered-wide / translate-half N=10,000: 4,769 -> 4,885 (1.024x)
* warm-core ordered-wide / translate-half N=10,000: 4,770 -> 4,886 (1.024x)
* fresh-core ordered-wide / remove8-middle N=100,000: 1,454 -> 1,487 (1.023x)
* warm-core ordered-wide / remove8-middle N=100,000: 1,454 -> 1,486 (1.022x)

## Above 1.02x but inside the cell's own scatter (`~`)

The larger of the two tags' run-to-run spread and sample noise exceeds the excess: not a measured regression.

None.

