# `a` against `base`

new / base per cell (values as in the matrix). **bold**: above 1.02x; `~`: above 1.02x but inside the cell's own scatter. Cells under 200 instructions
are counter-dominated: `+k` is the absolute difference in instructions.

## Structure, fresh tree

| shape / operation | 1e3 base | 1e3 new | 1e3 | 1e4 | 1e5 | 1e6 base | 1e6 new | 1e6 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ordered-wide / insert8-front | 8,605 | 8,402 | 0.976 | 0.894 | 0.754 | 821,534 | 509,445 | 0.620 |
| ordered-wide / remove8-front | 1,556 | 1,556 | 1.000 | 1.001 | 1.000 | 2,516 | 2,516 | 1.000 |
| ordered-wide / move8-front | 3,698 | 3,701 | 1.001 | 1.000 | 1.000 | 385,874 | 105,733 | 0.274 |
| ordered-wide / insert8-middle | 8,202 | 7,949 | 0.969 | 0.846 | 0.608 | 516,071 | 276,056 | 0.535 |
| ordered-wide / remove8-middle | 1,155 | 1,156 | 1.001 | 1.001 | 1.000 | 1,867 | 1,867 | 1.000 |
| ordered-wide / move8-middle | 10,846 | 10,645 | 0.981 | 0.907 | 0.764 | 634,813 | 480,156 | 0.756 |
| ordered-wide / insert8-end | 1,057 | 1,058 | 1.001 | 1.000 | 1.000 | 392,909 | 89,719 | 0.228 |
| ordered-wide / remove8-end | 824 | 824 | 1.000 | 1.000 | 1.000 | 853 | 854 | 1.001 |
| ordered-wide / move8-end | 10,515 | 10,313 | 0.981 | 0.904 | 0.761 | 647,621 | 484,004 | 0.747 |
| ordered-wide / insert64-front | 11,529 | 11,325 | 0.982 | 0.908 | 0.763 | 666,426 | 513,863 | 0.771 |
| ordered-wide / remove64-front | 4,892 | 4,717 | 0.964 | 0.818 | 0.697 | 582,239 | 426,550 | 0.733 |
| ordered-wide / move64-front | 17,119 | 16,566 | 0.968 | 0.810 | 0.531 | 892,638 | 451,482 | 0.506 |
| ordered-wide / insert64-middle | 11,107 | 10,855 | 0.977 | 0.870 | 0.626 | 532,778 | 292,675 | 0.549 |
| ordered-wide / remove64-middle | 2,605 | 2,605 | 1.000 | 1.000 | 1.000 | 3,546 | 3,546 | 1.000 |
| ordered-wide / move64-middle | 20,243 | 20,153 | 0.996 | 0.935 | 0.790 | 676,414 | 488,463 | 0.722 |
| ordered-wide / insert64-end | 10,777 | 10,470 | 0.972 | 0.824 | 0.416 | 394,765 | 112,287 | 0.284 |
| ordered-wide / remove64-end | 2,258 | 1,985 | 0.879 | 0.417 | 0.063 | 299,396 | 1,058 | 0.004 |
| ordered-wide / move64-end | 15,153 | 14,805 | 0.977 | 0.794 | 0.520 | 865,511 | 385,614 | 0.446 |
| ordered-wide / append1 | 546 | 546 | 1.000 | 1.002 | 1.000 | 394,950 | 102,681 | 0.260 |
| ordered-wide / translate64 | 1,455 | 1,457 | 1.001 | 1.002 | 1.000 | 1,539 | 1,540 | 1.001 |
| ordered-wide / translate-half | 2,502 | 2,503 | 1.000 | 1.000 | 1.000 | 865,068 | 865,068 | 1.000 |
| ordered-wide / bulk-build | 98,037 | 97,869 | 0.998 | 0.998 | 0.998 | 84,855,013 | 85,362,603 | 1.006 |
| ordered-wide / build-append8 | 199,251 | 196,380 | 0.986 | 0.858 | 0.376 | 3,229,145,339 | 185,962,886 | 0.058 |
| ordered-wide / freeze-cold | 26,946 | 26,950 | 1.000 | 1.000 | 1.000 | 24,561,364 | 24,610,772 | 1.002 |
| ordered-wide / freeze-idle | 1,494 | 1,497 | 1.002 | 1.001 | 1.000 | 472,435 | 504,574 | 1.068~ |
| ordered-wide / freeze-one | 1,794 | 1,795 | 1.001 | 1.000 | 1.000 | 472,733 | 570,759 | 1.207~ |
| ordered-wide / freeze-all | 6,435 | 6,436 | 1.000 | 1.000 | 1.000 | 5,003,984 | 5,134,150 | **1.026** |
| ordered-wide / parent-last | 229 | 229 | 1.000 | 1.000 | 1.000 | 15,900 | 15,900 | 1.000 |
| ordered-wide / ancestors-last | 295 | 295 | 1.000 | 1.000 | 1.000 | 15,965 | 15,965 | 1.000 |
| ordered-wide / subtree-root | 205 | 205 | 1.000 | 1.000 | 1.000 | 17,812 | 17,812 | 1.000 |
| ordered-wide / subtree-leaf | 114 | 114 | 1.000 | 1.000 | 1.000 | 153 | 153 | 1.000 |
| ordered-chains32 / parent-last | 88 | 88 | 1.000 | 1.000 | 1.000 | 161 | 161 | 1.000 |
| ordered-chains32 / ancestors-last | 475 | 475 | 1.000 | 1.000 | 1.000 | 16,841 | 16,841 | 1.000 |
| ordered-chains32 / subtree-root | 205 | 205 | 1.000 | 1.000 | 1.000 | 17,815 | 17,812 | 1.000 |
| ordered-chains32 / subtree-leaf | 114 | 114 | 1.000 | 1.000 | 1.000 | 153 | 153 | 1.000 |
| ordered-wide / set-ink-pos | 421 | 421 | 1.000 | 1.000 | 1.000 | 502 | 502 | 1.000 |
| ordered-wide / set-ink-handle | 387 | 387 | 1.000 | 1.000 | 1.000 | 387 | 387 | 1.000 |
| ordered-wide / pos-of-handle | 24 | 24 | 1.000 | 1.000 | 1.000 | 24 | 24 | 1.000 |
| ordered-wide / touch-handle | 131 | 131 | 1.000 | 1.000 | 1.000 | 131 | 131 | 1.000 |
| ordered-wide / set-inks-handles64 | 19,763 | 19,763 | 1.000 | 1.000 | 1.000 | 37,580 | 37,580 | 1.000 |
| ordered-wide / insert8-front+pos64 | 10,757 | 10,559 | 0.982 | 0.901 | 0.756 | 646,455 | 463,845 | 0.718 |
| ordered-wide / insert8-front+posall | 9,252 | 9,064 | 0.980 | 0.910 | 0.842 | 1,059,159 | 905,212 | 0.855 |

## Structure, warm columns

| shape / operation | 1e3 base | 1e3 new | 1e3 | 1e4 | 1e5 | 1e6 base | 1e6 new | 1e6 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ordered-wide / insert8-front | 3,961 | 3,764 | 0.950 | 0.793 | 0.690 | 556,842 | 457,498 | 0.822 |
| ordered-wide / remove8-front | 1,556 | 1,555 | 0.999 | 1.000 | 1.000 | 2,516 | 2,516 | 1.000 |
| ordered-wide / move8-front | 3,700 | 3,699 | 1.000 | 1.000 | 1.000 | 304,701 | 6,364 | 0.021 |
| ordered-wide / insert8-middle | 3,557 | 3,311 | 0.931 | 0.680 | 0.477 | 428,952 | 188,961 | 0.441 |
| ordered-wide / remove8-middle | 1,156 | 1,156 | 1.000 | 1.000 | 1.000 | 1,867 | 1,867 | 1.000 |
| ordered-wide / move8-middle | 6,202 | 6,006 | 0.968 | 0.844 | 0.704 | 559,875 | 402,533 | 0.719 |
| ordered-wide / insert8-end | 1,058 | 1,058 | 1.000 | 1.001 | 1.000 | 301,020 | 2,682 | 0.009 |
| ordered-wide / remove8-end | 823 | 824 | 1.001 | 1.000 | 1.000 | 854 | 854 | 1.000 |
| ordered-wide / move8-end | 5,870 | 5,675 | 0.967 | 0.834 | 0.701 | 558,855 | 380,445 | 0.681 |
| ordered-wide / insert64-front | 6,884 | 6,687 | 0.971 | 0.845 | 0.705 | 559,766 | 412,139 | 0.736 |
| ordered-wide / remove64-front | 4,714 | 4,538 | 0.963 | 0.815 | 0.696 | 559,450 | 377,822 | 0.675 |
| ordered-wide / move64-front | 16,941 | 16,592 | 0.979 | 0.808 | 0.529 | 869,755 | 418,998 | 0.482 |
| ordered-wide / insert64-middle | 6,462 | 6,217 | 0.962 | 0.770 | 0.510 | 432,357 | 192,401 | 0.445 |
| ordered-wide / remove64-middle | 2,605 | 2,605 | 1.000 | 1.000 | 1.000 | 3,545 | 3,546 | 1.000 |
| ordered-wide / move64-middle | 15,790 | 15,515 | 0.983 | 0.913 | 0.745 | 569,474 | 434,877 | 0.764 |
| ordered-wide / insert64-end | 6,141 | 5,840 | 0.951 | 0.663 | 0.169 | 303,997 | 5,663 | 0.019 |
| ordered-wide / remove64-end | 2,080 | 1,806 | 0.868 | 0.396 | 0.058 | 299,218 | 880 | 0.003 |
| ordered-wide / move64-end | 15,163 | 14,626 | 0.965 | 0.791 | 0.519 | 865,046 | 417,574 | 0.483 |
| ordered-wide / append1 | 546 | 546 | 1.000 | 1.003 | 1.002 | 300,808 | 2,473 | 0.008 |
| ordered-wide / translate64 | 1,457 | 1,457 | 1.000 | 1.001 | 1.000 | 1,540 | 1,540 | 1.000 |
| ordered-wide / translate-half | 2,503 | 2,502 | 1.000 | 1.000 | 1.000 | 865,068 | 865,104 | 1.000 |
| ordered-wide / bulk-build | 97,939 | 97,893 | 1.000 | 0.998 | 0.998 | 84,676,512 | 85,442,139 | 1.009 |
| ordered-wide / build-append8 | 199,382 | 196,418 | 0.985 | 0.858 | 0.375 | 3,228,779,143 | 186,687,765 | 0.058 |
| ordered-wide / freeze-cold | 26,957 | 26,960 | 1.000 | 1.000 | 1.000 | 23,948,207 | 24,540,651 | 1.025~ |
| ordered-wide / freeze-idle | 1,497 | 1,497 | 1.000 | 1.003 | 1.000 | 480,469 | 609,029 | 1.268~ |
| ordered-wide / freeze-one | 1,795 | 1,795 | 1.000 | 1.003 | 1.000 | 561,116 | 528,976 | 0.943 |
| ordered-wide / freeze-all | 6,436 | 6,436 | 1.000 | 1.000 | 1.000 | 5,003,984 | 5,003,983 | 1.000 |
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
| ordered-wide / insert8-front+pos64 | 6,106 | 5,907 | 0.967 | 0.830 | 0.700 | 558,752 | 377,101 | 0.675 |
| ordered-wide / insert8-front+posall | 4,609 | 4,412 | 0.957 | 0.861 | 0.818 | 967,238 | 785,587 | 0.812 |

## Spatial

| shape / operation | 1e3 base | 1e3 new | 1e3 | 1e4 | 1e5 | 1e6 base | 1e6 new | 1e6 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ordered-wide / cull-1-band | 369 | 369 | 1.000 | 1.000 | 1.000 | 57,860 | 57,860 | 1.000 |
| ordered-wide / cull-1-dispersed | 41,205 | 41,205 | 1.000 | 1.000 | 1.000 | 17,530,978 | 17,529,740 | 1.000 |
| ordered-wide / cull-10-band | 846 | 846 | 1.000 | 1.000 | 1.000 | 570,976 | 570,976 | 1.000 |
| ordered-wide / cull-10-dispersed | 82,857 | 82,857 | 1.000 | 1.000 | 1.000 | 17,673,114 | 17,668,106 | 1.000 |
| ordered-wide / cull-100-band | 5,928 | 5,928 | 1.000 | 1.000 | 1.000 | 5,702,180 | 5,702,180 | 1.000 |
| ordered-wide / cull-100-dispersed | 5,928 | 5,928 | 1.000 | 1.000 | 1.000 | 5,702,180 | 5,702,180 | 1.000 |
| ordered-wide / hit-present | 325 | 325 | 1.000 | 1.000 | 1.000 | 988 | 988 | 1.000 |
| ordered-wide / hit-gap | 306 | 306 | 1.000 | 1.000 | 1.000 | 813 | 813 | 1.000 |
| interleaved-wide / cull-1-band | 525 | 525 | 1.000 | 1.000 | 1.000 | 332,305 | 332,305 | 1.000 |
| interleaved-wide / cull-1-dispersed | 48,299 | 48,299 | 1.000 | 1.000 | 1.000 | 45,777,455 | 45,776,125 | 1.000 |
| interleaved-wide / cull-10-band | 867 | 867 | 1.000 | 1.000 | 1.000 | 845,780 | 845,780 | 1.000 |
| interleaved-wide / cull-10-dispersed | 88,079 | 88,079 | 1.000 | 1.000 | 1.000 | 87,297,494 | 87,288,896 | 1.000 |
| interleaved-wide / cull-100-band | 5,928 | 5,928 | 1.000 | 1.000 | 1.000 | 5,703,189 | 5,702,180 | 1.000 |
| interleaved-wide / cull-100-dispersed | 5,928 | 5,928 | 1.000 | 1.000 | 1.000 | 5,702,180 | 5,702,180 | 1.000 |
| interleaved-wide / hit-present | 299 | 299 | 1.000 | 1.000 | 1.000 | 1,527 | 1,527 | 1.000 |
| interleaved-wide / hit-gap | 306 | 306 | 1.000 | 1.000 | 1.000 | 123,873 | 123,870 | 1.000 |

## Cells above 1.02x

* fresh-core ordered-wide / freeze-all N=1,000,000: 5,003,984 -> 5,134,150 (1.026x)

## Above 1.02x but inside the cell's own scatter (`~`)

The larger of the two tags' run-to-run spread and sample noise exceeds the excess: not a measured regression.

* warm-core ordered-wide / freeze-idle N=1,000,000: 480,469 -> 609,029 (1.268x; scatter 41%)
* fresh-core ordered-wide / freeze-one N=1,000,000: 472,733 -> 570,759 (1.207x; scatter 35%)
* fresh-core ordered-wide / freeze-idle N=1,000,000: 472,435 -> 504,574 (1.068x; scatter 105%)
* warm-core ordered-wide / freeze-cold N=1,000,000: 23,948,207 -> 24,540,651 (1.025x; scatter 6%)

