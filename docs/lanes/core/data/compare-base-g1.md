# `g1` against `base`

new / base per cell (values as in the matrix). **bold**: above 1.02x; `~`: above 1.02x but inside the cell's own scatter. Cells under 200 instructions
are counter-dominated: `+k` is the absolute difference in instructions.

## Structure, fresh tree

| shape / operation | 1e3 base | 1e3 new | 1e3 | 1e4 | 1e5 | 1e6 base | 1e6 new | 1e6 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ordered-wide / insert8-front | 8,599 | 4,832 | 0.562 | 0.294 | 0.068 | 656,670 | 5,497 | 0.008 |
| ordered-wide / remove8-front | 1,556 | 1,611 | **1.035** | 0.998 | 0.991 | 2,516 | 2,180 | 0.866 |
| ordered-wide / move8-front | 3,701 | 3,618 | 0.978 | 0.957 | 0.954 | 404,617 | 6,856 | 0.017 |
| ordered-wide / insert8-middle | 8,195 | 4,447 | 0.543 | 0.297 | 0.072 | 528,887 | 4,639 | 0.009 |
| ordered-wide / remove8-middle | 1,156 | 1,133 | 0.980 | 0.972 | 0.960 | 1,867 | 1,656 | 0.887 |
| ordered-wide / move8-middle | 10,842 | 6,937 | 0.640 | 0.390 | 0.099 | 659,715 | 8,185 | 0.012 |
| ordered-wide / insert8-end | 1,058 | 1,036 | 0.979 | 0.967 | 0.969 | 400,475 | 3,635 | 0.009 |
| ordered-wide / remove8-end | 824 | 851 | **1.033** | **1.030** | 1.013 | 854 | 881 | **1.032** |
| ordered-wide / move8-end | 10,510 | 6,653 | 0.633 | 0.370 | 0.092 | 658,682 | 7,408 | 0.011 |
| ordered-wide / insert64-front | 11,522 | 7,554 | 0.656 | 0.389 | 0.102 | 653,176 | 8,219 | 0.013 |
| ordered-wide / remove64-front | 4,893 | 5,152 | **1.053** | 0.647 | 0.111 | 559,629 | 8,021 | 0.014 |
| ordered-wide / move64-front | 17,024 | 15,485 | 0.910 | 0.693 | 0.176 | 870,008 | 18,912 | 0.022 |
| ordered-wide / insert64-middle | 11,099 | 7,208 | 0.649 | 0.399 | 0.114 | 519,133 | 7,951 | 0.015 |
| ordered-wide / remove64-middle | 2,605 | 2,570 | 0.987 | 0.990 | 0.940 | 3,546 | 3,092 | 0.872 |
| ordered-wide / move64-middle | 20,428 | 15,462 | 0.757 | 0.549 | 0.187 | 656,525 | 16,706 | 0.025 |
| ordered-wide / insert64-end | 10,771 | 6,904 | 0.641 | 0.396 | 0.136 | 390,923 | 6,467 | 0.017 |
| ordered-wide / remove64-end | 2,259 | 2,316 | **1.025** | 0.542 | 0.084 | 299,396 | 1,837 | 0.006 |
| ordered-wide / move64-end | 15,342 | 13,506 | 0.880 | 0.639 | 0.149 | 865,589 | 14,596 | 0.017 |
| ordered-wide / append1 | 546 | 539 | 0.987 | 0.964 | 0.965 | 387,535 | 3,445 | 0.009 |
| ordered-wide / translate64 | 1,457 | 1,467 | 1.007 | 0.981 | 0.986 | 1,540 | 1,527 | 0.992 |
| ordered-wide / translate-half | 2,503 | 2,071 | 0.827 | 0.463 | 0.331 | 865,068 | 272,654 | 0.315 |
| ordered-wide / bulk-build | 98,071 | 93,402 | 0.952 | 0.979 | 0.978 | 84,536,973 | 83,449,180 | 0.987 |
| ordered-wide / build-append8 | 199,299 | 180,239 | 0.904 | 0.814 | 0.359 | 3,229,981,078 | 178,902,411 | 0.055 |
| ordered-wide / freeze-cold | 26,959 | 27,650 | **1.026** | 0.981 | 0.997 | 24,684,780 | 25,538,060 | 1.035~ |
| ordered-wide / freeze-idle | 1,497 | 754 | 0.504 | 0.131 | 0.016 | 472,435 | 754 | 0.002 |
| ordered-wide / freeze-one | 1,795 | 2,176 | **1.212** | **1.072** | **1.039** | 472,732 | 488,067 | **1.032** |
| ordered-wide / freeze-all | 6,436 | 6,817 | **1.059** | 1.008 | 1.004 | 5,003,983 | 5,019,318 | 1.003 |
| ordered-wide / parent-last | 229 | 222 | 0.969 | 0.967 | 0.228 | 15,900 | 743 | 0.047 |
| ordered-wide / ancestors-last | 295 | 288 | 0.976 | 0.967 | 0.254 | 15,965 | 806 | 0.050 |
| ordered-wide / subtree-root | 205 | 210 | **1.024** | 0.683 | 0.170 | 17,812 | 671 | 0.038 |
| ordered-wide / subtree-leaf | 114 | 116 | 1.018 (+2) | 0.918 (-14) | 0.922 (-14) | 153 | 155 | 1.013 (+2) |
| ordered-chains32 / parent-last | 88 | 77 | 0.875 (-11) | 0.777 (-27) | 0.814 (-27) | 161 | 150 | 0.932 (-11) |
| ordered-chains32 / ancestors-last | 475 | 474 | 0.998 | 1.001 | 0.503 | 16,841 | 1,712 | 0.102 |
| ordered-chains32 / subtree-root | 205 | 210 | **1.024** | 0.683 | 0.170 | 17,812 | 671 | 0.038 |
| ordered-chains32 / subtree-leaf | 114 | 116 | 1.018 (+2) | 0.918 (-14) | 0.922 (-14) | 153 | 155 | 1.013 (+2) |
| ordered-wide / set-ink-pos | 421 | 364 | 0.865 | 0.839 | 0.847 | 502 | 437 | 0.871 |
| ordered-wide / set-ink-handle | 387 | 342 | 0.884 | 0.884 | 0.884 | 387 | 342 | 0.884 |
| ordered-wide / pos-of-handle | 24 | 24 | 1.000 | 1.000 | 1.000 | 24 | 24 | 1.000 |
| ordered-wide / touch-handle | 131 | 117 | 0.893 (-14) | 0.893 (-14) | 0.893 (-14) | 131 | 117 | 0.893 (-14) |
| ordered-wide / set-inks-handles64 | 19,763 | 18,866 | 0.955 | 0.935 | 0.934 | 37,580 | 33,609 | 0.894 |
| ordered-wide / insert8-front+pos64 | 10,758 | 6,986 | 0.649 | 0.359 | 0.092 | 645,405 | 7,399 | 0.011 |
| ordered-wide / insert8-front+posall | 9,261 | 5,495 | 0.593 | 0.452 | 0.403 | 1,083,174 | 415,884 | 0.384 |

## Structure, warm columns

| shape / operation | 1e3 base | 1e3 new | 1e3 | 1e4 | 1e5 | 1e6 base | 1e6 new | 1e6 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ordered-wide / insert8-front | 3,961 | 3,215 | 0.812 | 0.438 | 0.068 | 556,844 | 4,472 | 0.008 |
| ordered-wide / remove8-front | 1,556 | 1,611 | **1.035** | 0.998 | 0.991 | 2,516 | 2,180 | 0.866 |
| ordered-wide / move8-front | 3,700 | 3,616 | 0.977 | 0.957 | 0.953 | 304,701 | 5,831 | 0.019 |
| ordered-wide / insert8-middle | 3,557 | 2,830 | 0.796 | 0.469 | 0.072 | 428,952 | 3,614 | 0.008 |
| ordered-wide / remove8-middle | 1,155 | 1,134 | 0.982 | 0.972 | 0.959 | 1,867 | 1,656 | 0.887 |
| ordered-wide / move8-middle | 6,202 | 5,316 | 0.857 | 0.561 | 0.106 | 559,879 | 7,169 | 0.013 |
| ordered-wide / insert8-end | 1,057 | 1,037 | 0.981 | 0.968 | 0.969 | 301,021 | 2,609 | 0.009 |
| ordered-wide / remove8-end | 824 | 851 | **1.033** | **1.030** | 1.011 | 854 | 881 | **1.032** |
| ordered-wide / move8-end | 5,870 | 5,034 | 0.858 | 0.539 | 0.097 | 558,856 | 6,393 | 0.011 |
| ordered-wide / insert64-front | 6,884 | 5,936 | 0.862 | 0.560 | 0.109 | 559,765 | 7,194 | 0.013 |
| ordered-wide / remove64-front | 4,714 | 4,973 | **1.055** | 0.640 | 0.108 | 559,450 | 7,840 | 0.014 |
| ordered-wide / move64-front | 16,845 | 15,303 | 0.908 | 0.690 | 0.174 | 869,756 | 18,674 | 0.021 |
| ordered-wide / insert64-middle | 6,461 | 5,590 | 0.865 | 0.599 | 0.126 | 432,357 | 6,926 | 0.016 |
| ordered-wide / remove64-middle | 2,605 | 2,570 | 0.987 | 0.990 | 0.940 | 3,546 | 3,092 | 0.872 |
| ordered-wide / move64-middle | 15,789 | 13,843 | 0.877 | 0.710 | 0.212 | 569,674 | 15,845 | 0.028 |
| ordered-wide / insert64-end | 6,141 | 5,286 | 0.861 | 0.624 | 0.163 | 303,999 | 5,442 | 0.018 |
| ordered-wide / remove64-end | 2,080 | 2,136 | **1.027** | 0.524 | 0.079 | 299,219 | 1,656 | 0.006 |
| ordered-wide / move64-end | 15,163 | 13,326 | 0.879 | 0.636 | 0.148 | 865,398 | 14,323 | 0.017 |
| ordered-wide / append1 | 546 | 538 | 0.985 | 0.962 | 0.965 | 300,808 | 2,420 | 0.008 |
| ordered-wide / translate64 | 1,456 | 1,468 | 1.008 | 0.981 | 0.986 | 1,540 | 1,527 | 0.992 |
| ordered-wide / translate-half | 2,502 | 2,071 | 0.828 | 0.463 | 0.331 | 865,068 | 272,654 | 0.315 |
| ordered-wide / bulk-build | 98,065 | 93,412 | 0.953 | 0.979 | 0.978 | 85,693,944 | 83,083,438 | 0.970 |
| ordered-wide / build-append8 | 199,292 | 180,231 | 0.904 | 0.814 | 0.359 | 3,229,385,391 | 178,022,784 | 0.055 |
| ordered-wide / freeze-cold | 26,960 | 27,650 | **1.026** | 0.981 | 0.997 | 25,510,010 | 24,773,138 | 0.971 |
| ordered-wide / freeze-idle | 1,496 | 753 | 0.503 | 0.131 | 0.016 | 472,434 | 754 | 0.002 |
| ordered-wide / freeze-one | 1,795 | 2,176 | **1.212** | **1.069** | **1.039** | 472,732 | 489,674 | **1.036** |
| ordered-wide / freeze-all | 6,436 | 6,817 | **1.059** | 1.008 | 1.004 | 5,003,983 | 5,022,532 | 1.004 |
| ordered-wide / parent-last | 229 | 222 | 0.969 | 0.967 | 0.228 | 15,900 | 743 | 0.047 |
| ordered-wide / ancestors-last | 295 | 288 | 0.976 | 0.967 | 0.254 | 15,965 | 806 | 0.050 |
| ordered-wide / subtree-root | 205 | 210 | **1.024** | 0.683 | 0.170 | 17,812 | 671 | 0.038 |
| ordered-wide / subtree-leaf | 114 | 116 | 1.018 (+2) | 0.918 (-14) | 0.922 (-14) | 153 | 155 | 1.013 (+2) |
| ordered-chains32 / parent-last | 88 | 77 | 0.875 (-11) | 0.777 (-27) | 0.814 (-27) | 161 | 150 | 0.932 (-11) |
| ordered-chains32 / ancestors-last | 475 | 474 | 0.998 | 1.001 | 0.503 | 16,841 | 1,712 | 0.102 |
| ordered-chains32 / subtree-root | 205 | 210 | **1.024** | 0.683 | 0.170 | 17,812 | 671 | 0.038 |
| ordered-chains32 / subtree-leaf | 114 | 116 | 1.018 (+2) | 0.918 (-14) | 0.922 (-14) | 153 | 155 | 1.013 (+2) |
| ordered-wide / set-ink-pos | 421 | 364 | 0.865 | 0.839 | 0.847 | 502 | 437 | 0.871 |
| ordered-wide / set-ink-handle | 387 | 342 | 0.884 | 0.884 | 0.884 | 387 | 342 | 0.884 |
| ordered-wide / pos-of-handle | 24 | 24 | 1.000 | 1.000 | 1.000 | 24 | 24 | 1.000 |
| ordered-wide / touch-handle | 131 | 117 | 0.893 (-14) | 0.893 (-14) | 0.893 (-14) | 131 | 117 | 0.893 (-14) |
| ordered-wide / set-inks-handles64 | 19,763 | 18,866 | 0.955 | 0.935 | 0.934 | 37,580 | 33,609 | 0.894 |
| ordered-wide / insert8-front+pos64 | 6,106 | 5,354 | 0.877 | 0.536 | 0.097 | 558,752 | 6,380 | 0.011 |
| ordered-wide / insert8-front+posall | 4,609 | 3,863 | 0.838 | 0.622 | 0.453 | 967,236 | 414,866 | 0.429 |

## Spatial

| shape / operation | 1e3 base | 1e3 new | 1e3 | 1e4 | 1e5 | 1e6 base | 1e6 new | 1e6 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ordered-wide / cull-1-band | 369 | 375 | 1.016 | 0.934 | 0.937 | 57,860 | 53,784 | 0.930 |
| ordered-wide / cull-1-dispersed | 41,205 | 40,361 | 0.980 | 0.962 | 0.959 | 17,529,740 | 16,799,559 | 0.958 |
| ordered-wide / cull-10-band | 846 | 823 | 0.973 | 0.929 | 0.929 | 570,976 | 530,580 | 0.929 |
| ordered-wide / cull-10-dispersed | 82,857 | 81,924 | 0.989 | 0.970 | 0.961 | 17,668,106 | 16,936,072 | 0.959 |
| ordered-wide / cull-100-band | 5,928 | 5,541 | 0.935 | 0.927 | 0.929 | 5,702,180 | 5,298,813 | 0.929 |
| ordered-wide / cull-100-dispersed | 5,928 | 5,541 | 0.935 | 0.927 | 0.929 | 5,702,180 | 5,298,813 | 0.929 |
| ordered-wide / hit-present | 325 | 342 | **1.052** | 0.888 | 0.959 | 988 | 918 | 0.929 |
| ordered-wide / hit-gap | 306 | 319 | **1.042** | 0.897 | 0.978 | 813 | 756 | 0.930 |
| interleaved-wide / cull-1-band | 525 | 494 | 0.941 | 0.824 | 0.845 | 332,305 | 281,188 | 0.846 |
| interleaved-wide / cull-1-dispersed | 48,299 | 47,424 | 0.982 | 0.980 | 0.981 | 45,776,127 | 44,885,040 | 0.981 |
| interleaved-wide / cull-10-band | 867 | 827 | 0.954 | 0.885 | 0.895 | 845,780 | 757,713 | 0.896 |
| interleaved-wide / cull-10-dispersed | 88,079 | 87,119 | 0.989 | 0.989 | 0.989 | 87,288,896 | 86,307,942 | 0.989 |
| interleaved-wide / cull-100-band | 5,928 | 5,541 | 0.935 | 0.927 | 0.929 | 5,702,180 | 5,298,813 | 0.929 |
| interleaved-wide / cull-100-dispersed | 5,928 | 5,541 | 0.935 | 0.927 | 0.929 | 5,702,180 | 5,298,813 | 0.929 |
| interleaved-wide / hit-present | 299 | 316 | **1.057** | 0.787 | 0.763 | 1,527 | 1,109 | 0.726 |
| interleaved-wide / hit-gap | 306 | 319 | **1.042** | 0.785 | 0.807 | 123,870 | 100,088 | 0.808 |

## Cells above 1.02x

* warm-core ordered-wide / freeze-one N=1,000: 1,795 -> 2,176 (1.212x)
* fresh-core ordered-wide / freeze-one N=1,000: 1,795 -> 2,176 (1.212x)
* fresh-core ordered-wide / freeze-one N=10,000: 6,042 -> 6,478 (1.072x)
* warm-core ordered-wide / freeze-one N=10,000: 6,060 -> 6,479 (1.069x)
* warm-core ordered-wide / freeze-all N=1,000: 6,436 -> 6,817 (1.059x)
* fresh-core ordered-wide / freeze-all N=1,000: 6,436 -> 6,817 (1.059x)
* spatial interleaved-wide / hit-present N=1,000: 299 -> 316 (1.057x)
* warm-core ordered-wide / remove64-front N=1,000: 4,714 -> 4,973 (1.055x)
* fresh-core ordered-wide / remove64-front N=1,000: 4,893 -> 5,152 (1.053x)
* spatial ordered-wide / hit-present N=1,000: 325 -> 342 (1.052x)
* spatial ordered-wide / hit-gap N=1,000: 306 -> 319 (1.042x)
* spatial interleaved-wide / hit-gap N=1,000: 306 -> 319 (1.042x)
* fresh-core ordered-wide / freeze-one N=100,000: 48,446 -> 50,340 (1.039x)
* warm-core ordered-wide / freeze-one N=100,000: 48,446 -> 50,339 (1.039x)
* warm-core ordered-wide / freeze-one N=1,000,000: 472,732 -> 489,674 (1.036x)
* warm-core ordered-wide / remove8-front N=1,000: 1,556 -> 1,611 (1.035x)
* fresh-core ordered-wide / remove8-front N=1,000: 1,556 -> 1,611 (1.035x)
* warm-core ordered-wide / remove8-end N=1,000: 824 -> 851 (1.033x)
* fresh-core ordered-wide / remove8-end N=1,000: 824 -> 851 (1.033x)
* fresh-core ordered-wide / freeze-one N=1,000,000: 472,732 -> 488,067 (1.032x)
* warm-core ordered-wide / remove8-end N=1,000,000: 854 -> 881 (1.032x)
* fresh-core ordered-wide / remove8-end N=1,000,000: 854 -> 881 (1.032x)
* warm-core ordered-wide / remove8-end N=10,000: 1,042 -> 1,073 (1.030x)
* fresh-core ordered-wide / remove8-end N=10,000: 1,042 -> 1,073 (1.030x)
* warm-core ordered-wide / remove64-end N=1,000: 2,080 -> 2,136 (1.027x)
* fresh-core ordered-wide / freeze-cold N=1,000: 26,959 -> 27,650 (1.026x)
* warm-core ordered-wide / freeze-cold N=1,000: 26,960 -> 27,650 (1.026x)
* fresh-core ordered-wide / remove64-end N=1,000: 2,259 -> 2,316 (1.025x)
* warm-core ordered-wide / subtree-root N=1,000: 205 -> 210 (1.024x)
* warm-core ordered-chains32 / subtree-root N=1,000: 205 -> 210 (1.024x)
* fresh-core ordered-wide / subtree-root N=1,000: 205 -> 210 (1.024x)
* fresh-core ordered-chains32 / subtree-root N=1,000: 205 -> 210 (1.024x)

## Above 1.02x but inside the cell's own scatter (`~`)

The larger of the two tags' run-to-run spread and sample noise exceeds the excess: not a measured regression.

* fresh-core ordered-wide / freeze-cold N=1,000,000: 24,684,780 -> 25,538,060 (1.035x; scatter 4%)

