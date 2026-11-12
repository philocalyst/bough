# `a` against `base`

new / base per cell (values as in the matrix). **bold**: above 1.02x; `~`: above 1.02x but inside the cell's own scatter. Cells under 200 instructions
are counter-dominated: `+k` is the absolute difference in instructions.

## Structure, fresh tree

| shape / operation | 1e3 base | 1e3 new | 1e3 | 1e4 | 1e5 | 1e6 base | 1e6 new | 1e6 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ordered-wide / insert8-front | 8,599 | 8,366 | 0.973 | 0.883 | 0.753 | 656,670 | 454,350 | 0.692 |
| ordered-wide / remove8-front | 1,556 | 1,556 | 1.000 | 1.000 | 1.000 | 2,516 | 2,516 | 1.000 |
| ordered-wide / move8-front | 3,701 | 3,701 | 1.000 | 1.000 | 1.000 | 404,617 | 106,146 | 0.262 |
| ordered-wide / insert8-middle | 8,195 | 7,950 | 0.970 | 0.836 | 0.603 | 528,887 | 276,001 | 0.522 |
| ordered-wide / remove8-middle | 1,156 | 1,156 | 1.000 | 1.000 | 1.000 | 1,867 | 1,867 | 1.000 |
| ordered-wide / move8-middle | 10,842 | 10,646 | 0.982 | 0.900 | 0.761 | 659,715 | 478,052 | 0.725 |
| ordered-wide / insert8-end | 1,058 | 1,058 | 1.000 | 1.000 | 1.000 | 400,475 | 102,609 | 0.256 |
| ordered-wide / remove8-end | 824 | 824 | 1.000 | 1.000 | 1.000 | 854 | 854 | 1.000 |
| ordered-wide / move8-end | 10,510 | 10,314 | 0.981 | 0.897 | 0.759 | 658,682 | 476,996 | 0.724 |
| ordered-wide / insert64-front | 11,522 | 11,322 | 0.983 | 0.901 | 0.762 | 653,176 | 477,982 | 0.732 |
| ordered-wide / remove64-front | 4,893 | 4,717 | 0.964 | 0.818 | 0.697 | 559,629 | 377,999 | 0.675 |
| ordered-wide / move64-front | 17,024 | 16,566 | 0.973 | 0.810 | 0.531 | 870,008 | 389,989 | 0.448 |
| ordered-wide / insert64-middle | 11,099 | 10,856 | 0.978 | 0.862 | 0.623 | 519,133 | 292,190 | 0.563 |
| ordered-wide / remove64-middle | 2,605 | 2,603 | 0.999 | 1.000 | 1.000 | 3,546 | 3,546 | 1.000 |
| ordered-wide / move64-middle | 20,428 | 20,195 | 0.989 | 0.936 | 0.789 | 656,525 | 487,850 | 0.743 |
| ordered-wide / insert64-end | 10,771 | 10,471 | 0.972 | 0.819 | 0.411 | 390,923 | 105,480 | 0.270 |
| ordered-wide / remove64-end | 2,259 | 1,984 | 0.878 | 0.417 | 0.063 | 299,396 | 1,058 | 0.004 |
| ordered-wide / move64-end | 15,342 | 14,805 | 0.965 | 0.794 | 0.520 | 865,589 | 385,523 | 0.445 |
| ordered-wide / append1 | 546 | 545 | 0.998 | 1.000 | 1.000 | 387,535 | 102,288 | 0.264 |
| ordered-wide / translate64 | 1,457 | 1,457 | 1.000 | 1.000 | 1.000 | 1,540 | 1,540 | 1.000 |
| ordered-wide / translate-half | 2,503 | 2,503 | 1.000 | 1.000 | 1.000 | 865,068 | 865,068 | 1.000 |
| ordered-wide / bulk-build | 98,071 | 97,861 | 0.998 | 0.998 | 0.998 | 84,536,973 | 85,514,120 | 1.012 |
| ordered-wide / build-append8 | 199,299 | 196,372 | 0.985 | 0.858 | 0.376 | 3,229,981,078 | 186,784,546 | 0.058 |
| ordered-wide / freeze-cold | 26,959 | 26,951 | 1.000 | 1.000 | 1.000 | 24,684,780 | 24,581,794 | 0.996 |
| ordered-wide / freeze-idle | 1,497 | 1,497 | 1.000 | 1.000 | 1.000 | 472,435 | 472,434 | 1.000 |
| ordered-wide / freeze-one | 1,795 | 1,795 | 1.000 | 1.000 | 1.000 | 472,732 | 472,733 | 1.000 |
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
| ordered-wide / set-ink-handle | 387 | 387 | 1.000 | 1.000 | 1.000 | 387 | 387 | 1.000 |
| ordered-wide / pos-of-handle | 24 | 24 | 1.000 | 1.000 | 1.000 | 24 | 24 | 1.000 |
| ordered-wide / touch-handle | 131 | 131 | 1.000 | 1.000 | 1.000 | 131 | 131 | 1.000 |
| ordered-wide / set-inks-handles64 | 19,763 | 19,763 | 1.000 | 1.000 | 1.000 | 37,580 | 37,580 | 1.000 |
| ordered-wide / insert8-front+pos64 | 10,758 | 10,559 | 0.982 | 0.894 | 0.759 | 645,405 | 457,881 | 0.709 |
| ordered-wide / insert8-front+posall | 9,261 | 9,064 | 0.979 | 0.910 | 0.841 | 1,083,174 | 898,433 | 0.829 |

## Structure, warm columns

| shape / operation | 1e3 base | 1e3 new | 1e3 | 1e4 | 1e5 | 1e6 base | 1e6 new | 1e6 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ordered-wide / insert8-front | 3,961 | 3,728 | 0.941 | 0.792 | 0.690 | 556,844 | 375,195 | 0.674 |
| ordered-wide / remove8-front | 1,556 | 1,520 | 0.977 | 0.999 | 1.000 | 2,516 | 2,516 | 1.000 |
| ordered-wide / move8-front | 3,700 | 3,699 | 1.000 | 0.999 | 0.999 | 304,701 | 6,364 | 0.021 |
| ordered-wide / insert8-middle | 3,557 | 3,275 | 0.921 | 0.679 | 0.477 | 428,952 | 188,961 | 0.441 |
| ordered-wide / remove8-middle | 1,155 | 1,117 | 0.967 | 0.999 | 1.000 | 1,867 | 1,867 | 1.000 |
| ordered-wide / move8-middle | 6,202 | 6,005 | 0.968 | 0.843 | 0.704 | 559,879 | 378,226 | 0.676 |
| ordered-wide / insert8-end | 1,057 | 1,058 | 1.001 | 0.999 | 1.000 | 301,021 | 2,684 | 0.009 |
| ordered-wide / remove8-end | 824 | 824 | 1.000 | 0.999 | 1.000 | 854 | 854 | 1.000 |
| ordered-wide / move8-end | 5,870 | 5,672 | 0.966 | 0.832 | 0.701 | 558,856 | 377,207 | 0.675 |
| ordered-wide / insert64-front | 6,884 | 6,686 | 0.971 | 0.843 | 0.705 | 559,765 | 378,116 | 0.675 |
| ordered-wide / remove64-front | 4,714 | 4,537 | 0.962 | 0.814 | 0.696 | 559,450 | 377,822 | 0.675 |
| ordered-wide / move64-front | 16,845 | 16,378 | 0.972 | 0.806 | 0.530 | 869,756 | 389,811 | 0.448 |
| ordered-wide / insert64-middle | 6,461 | 6,216 | 0.962 | 0.769 | 0.510 | 432,357 | 192,365 | 0.445 |
| ordered-wide / remove64-middle | 2,605 | 2,605 | 1.000 | 1.000 | 1.000 | 3,546 | 3,546 | 1.000 |
| ordered-wide / move64-middle | 15,789 | 15,514 | 0.983 | 0.910 | 0.744 | 569,674 | 388,165 | 0.681 |
| ordered-wide / insert64-end | 6,141 | 5,837 | 0.950 | 0.663 | 0.169 | 303,999 | 5,663 | 0.019 |
| ordered-wide / remove64-end | 2,080 | 1,806 | 0.868 | 0.396 | 0.058 | 299,219 | 880 | 0.003 |
| ordered-wide / move64-end | 15,163 | 14,625 | 0.965 | 0.789 | 0.518 | 865,398 | 385,343 | 0.445 |
| ordered-wide / append1 | 546 | 546 | 1.000 | 0.998 | 1.000 | 300,808 | 2,471 | 0.008 |
| ordered-wide / translate64 | 1,456 | 1,457 | 1.001 | 1.000 | 1.000 | 1,540 | 1,540 | 1.000 |
| ordered-wide / translate-half | 2,502 | 2,502 | 1.000 | 1.000 | 1.000 | 865,068 | 865,068 | 1.000 |
| ordered-wide / bulk-build | 98,065 | 97,867 | 0.998 | 0.998 | 0.998 | 85,693,944 | 85,468,095 | 0.997 |
| ordered-wide / build-append8 | 199,292 | 196,502 | 0.986 | 0.858 | 0.376 | 3,229,385,391 | 186,209,576 | 0.058 |
| ordered-wide / freeze-cold | 26,960 | 26,956 | 1.000 | 1.000 | 1.000 | 25,510,010 | 24,565,679 | 0.963 |
| ordered-wide / freeze-idle | 1,496 | 1,497 | 1.001 | 0.997 | 1.000 | 472,434 | 472,434 | 1.000 |
| ordered-wide / freeze-one | 1,795 | 1,795 | 1.000 | 0.997 | 1.000 | 472,732 | 472,732 | 1.000 |
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
| ordered-wide / set-ink-handle | 387 | 387 | 1.000 | 1.000 | 1.000 | 387 | 387 | 1.000 |
| ordered-wide / pos-of-handle | 24 | 24 | 1.000 | 1.000 | 1.000 | 24 | 24 | 1.000 |
| ordered-wide / touch-handle | 131 | 131 | 1.000 | 1.000 | 1.000 | 131 | 131 | 1.000 |
| ordered-wide / set-inks-handles64 | 19,763 | 19,763 | 1.000 | 1.000 | 1.000 | 37,580 | 37,580 | 1.000 |
| ordered-wide / insert8-front+pos64 | 6,106 | 5,907 | 0.967 | 0.829 | 0.700 | 558,752 | 377,101 | 0.675 |
| ordered-wide / insert8-front+posall | 4,609 | 4,412 | 0.957 | 0.860 | 0.818 | 967,236 | 785,587 | 0.812 |

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
| interleaved-wide / cull-1-dispersed | 48,299 | 48,299 | 1.000 | 1.000 | 1.000 | 45,776,127 | 45,776,125 | 1.000 |
| interleaved-wide / cull-10-band | 867 | 867 | 1.000 | 1.000 | 1.000 | 845,780 | 845,780 | 1.000 |
| interleaved-wide / cull-10-dispersed | 88,079 | 88,079 | 1.000 | 1.000 | 1.000 | 87,288,896 | 87,288,896 | 1.000 |
| interleaved-wide / cull-100-band | 5,928 | 5,928 | 1.000 | 1.000 | 1.000 | 5,702,180 | 5,702,180 | 1.000 |
| interleaved-wide / cull-100-dispersed | 5,928 | 5,928 | 1.000 | 1.000 | 1.000 | 5,702,180 | 5,702,180 | 1.000 |
| interleaved-wide / hit-present | 299 | 299 | 1.000 | 1.000 | 1.000 | 1,527 | 1,527 | 1.000 |
| interleaved-wide / hit-gap | 306 | 306 | 1.000 | 1.000 | 1.000 | 123,870 | 123,870 | 1.000 |

## Cells above 1.02x

None.

## Above 1.02x but inside the cell's own scatter (`~`)

The larger of the two tags' run-to-run spread and sample noise exceeds the excess: not a measured regression.

None.

