# `f3` against `a`

new / base per cell (values as in the matrix). **bold**: above 1.02x; `~`: above 1.02x but inside the cell's own scatter. Cells under 200 instructions
are counter-dominated: `+k` is the absolute difference in instructions.

## Structure, fresh tree

| shape / operation | 1e3 base | 1e3 new | 1e3 | 1e4 | 1e5 | 1e6 base | 1e6 new | 1e6 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ordered-wide / insert8-front | 8,366 | 8,424 | 1.007 | 1.006 | 1.000 | 454,350 | 475,040 | 1.046~ |
| ordered-wide / remove8-front | 1,556 | 1,478 | 0.950 | 0.958 | 0.955 | 2,516 | 2,438 | 0.969 |
| ordered-wide / move8-front | 3,701 | 3,471 | 0.938 | 0.944 | 0.943 | 106,146 | 100,912 | 0.951 |
| ordered-wide / insert8-middle | 7,950 | 7,975 | 1.003 | 1.007 | 1.001 | 276,001 | 288,813 | 1.046~ |
| ordered-wide / remove8-middle | 1,156 | 1,053 | 0.911 | 0.943 | 0.934 | 1,867 | 1,764 | 0.945 |
| ordered-wide / move8-middle | 10,646 | 10,406 | 0.977 | 0.991 | 0.996 | 478,052 | 465,189 | 0.973 |
| ordered-wide / insert8-end | 1,058 | 1,066 | 1.008 | 1.007 | 1.007 | 102,609 | 89,435 | 0.872 |
| ordered-wide / remove8-end | 824 | 771 | 0.936 | 0.949 | 0.938 | 854 | 800 | 0.937 |
| ordered-wide / move8-end | 10,314 | 10,122 | 0.981 | 0.993 | 0.996 | 476,996 | 464,335 | 0.973 |
| ordered-wide / insert64-front | 11,322 | 11,317 | 1.000 | 1.003 | 1.000 | 477,982 | 465,005 | 0.973 |
| ordered-wide / remove64-front | 4,717 | 4,638 | 0.983 | 0.990 | 0.998 | 377,999 | 377,919 | 1.000 |
| ordered-wide / move64-front | 16,566 | 15,375 | 0.928 | 0.939 | 0.978 | 389,989 | 388,886 | 0.997 |
| ordered-wide / insert64-middle | 10,856 | 10,903 | 1.004 | 1.004 | 1.000 | 292,190 | 292,246 | 1.000 |
| ordered-wide / remove64-middle | 2,603 | 2,471 | 0.949 | 0.953 | 0.957 | 3,546 | 3,412 | 0.962 |
| ordered-wide / move64-middle | 20,195 | 18,913 | 0.937 | 0.951 | 0.981 | 487,850 | 486,634 | 0.998 |
| ordered-wide / insert64-end | 10,471 | 10,533 | 1.006 | 1.006 | 1.006 | 105,480 | 105,532 | 1.000 |
| ordered-wide / remove64-end | 1,984 | 1,941 | 0.978 | 0.978 | 0.978 | 1,058 | 1,066 | 1.008 |
| ordered-wide / move64-end | 14,805 | 13,656 | 0.922 | 0.933 | 0.976 | 385,523 | 384,352 | 0.997 |
| ordered-wide / append1 | 545 | 554 | 1.017 | 1.014 | 1.013 | 102,288 | 102,279 | 1.000 |
| ordered-wide / translate64 | 1,457 | 1,453 | 0.997 | 0.997 | 0.997 | 1,540 | 1,535 | 0.997 |
| ordered-wide / translate-half | 2,503 | 2,485 | 0.993 | 0.985 | 0.982 | 865,068 | 849,441 | 0.982 |
| ordered-wide / bulk-build | 97,861 | 98,808 | 1.010 | 1.011 | 1.011 | 85,514,120 | 86,406,710 | 1.010 |
| ordered-wide / build-append8 | 196,372 | 198,128 | 1.009 | 1.009 | 1.009 | 186,784,546 | 187,579,566 | 1.004 |
| ordered-wide / freeze-cold | 26,951 | 26,978 | 1.001 | 1.000 | 1.000 | 24,581,794 | 24,544,574 | 0.998 |
| ordered-wide / freeze-idle | 1,497 | 1,507 | 1.007 | 1.002 | 1.000 | 472,434 | 472,444 | 1.000 |
| ordered-wide / freeze-one | 1,795 | 1,805 | 1.006 | 1.001 | 1.000 | 472,733 | 472,742 | 1.000 |
| ordered-wide / freeze-all | 6,436 | 6,446 | 1.002 | 1.000 | 1.000 | 5,003,983 | 5,003,993 | 1.000 |
| ordered-wide / parent-last | 229 | 229 | 1.000 | 1.000 | 1.000 | 15,900 | 15,900 | 1.000 |
| ordered-wide / ancestors-last | 295 | 295 | 1.000 | 1.000 | 1.000 | 15,965 | 15,965 | 1.000 |
| ordered-wide / subtree-root | 205 | 205 | 1.000 | 1.000 | 1.000 | 17,812 | 17,812 | 1.000 |
| ordered-wide / subtree-leaf | 114 | 114 | 1.000 | 1.000 | 1.000 | 153 | 153 | 1.000 |
| ordered-chains32 / parent-last | 88 | 88 | 1.000 | 1.000 | 1.000 | 161 | 161 | 1.000 |
| ordered-chains32 / ancestors-last | 475 | 475 | 1.000 | 1.000 | 1.000 | 16,841 | 16,841 | 1.000 |
| ordered-chains32 / subtree-root | 205 | 205 | 1.000 | 1.000 | 1.000 | 17,812 | 17,812 | 1.000 |
| ordered-chains32 / subtree-leaf | 114 | 114 | 1.000 | 1.000 | 1.000 | 153 | 153 | 1.000 |
| ordered-wide / set-ink-pos | 421 | 371 | 0.881 | 0.890 | 0.895 | 502 | 452 | 0.900 |
| ordered-wide / set-ink-handle | 387 | 337 | 0.871 | 0.871 | 0.871 | 387 | 337 | 0.871 |
| ordered-wide / pos-of-handle | 24 | 24 | 1.000 | 1.000 | 1.000 | 24 | 24 | 1.000 |
| ordered-wide / touch-handle | 131 | 117 | 0.893 (-14) | 0.893 (-14) | 0.893 (-14) | 131 | 117 | 0.893 (-14) |
| ordered-wide / set-inks-handles64 | 19,763 | 18,664 | 0.944 | 0.924 | 0.936 | 37,580 | 35,713 | 0.950 |
| ordered-wide / insert8-front+pos64 | 10,559 | 10,581 | 1.002 | 1.001 | 1.001 | 457,881 | 474,630 | 1.037~ |
| ordered-wide / insert8-front+posall | 9,064 | 9,086 | 1.002 | 1.003 | 1.001 | 898,433 | 872,736 | 0.971 |

## Structure, warm columns

| shape / operation | 1e3 base | 1e3 new | 1e3 | 1e4 | 1e5 | 1e6 base | 1e6 new | 1e6 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ordered-wide / insert8-front | 3,728 | 3,745 | 1.005 | 1.003 | 1.000 | 375,195 | 375,213 | 1.000 |
| ordered-wide / remove8-front | 1,520 | 1,479 | 0.973 | 0.959 | 0.955 | 2,516 | 2,438 | 0.969 |
| ordered-wide / move8-front | 3,699 | 3,438 | 0.929 | 0.944 | 0.943 | 6,364 | 6,167 | 0.969 |
| ordered-wide / insert8-middle | 3,275 | 3,299 | 1.007 | 1.004 | 1.001 | 188,961 | 188,988 | 1.000 |
| ordered-wide / remove8-middle | 1,117 | 1,017 | 0.910 | 0.944 | 0.934 | 1,867 | 1,764 | 0.945 |
| ordered-wide / move8-middle | 6,005 | 5,730 | 0.954 | 0.976 | 0.994 | 378,226 | 377,985 | 0.999 |
| ordered-wide / insert8-end | 1,058 | 1,066 | 1.008 | 1.008 | 1.007 | 2,684 | 2,721 | 1.014 |
| ordered-wide / remove8-end | 824 | 727 | 0.882 | 0.950 | 0.938 | 854 | 800 | 0.937 |
| ordered-wide / move8-end | 5,672 | 5,483 | 0.967 | 0.979 | 0.995 | 377,207 | 377,014 | 0.999 |
| ordered-wide / insert64-front | 6,686 | 6,641 | 0.993 | 0.999 | 1.000 | 378,116 | 378,106 | 1.000 |
| ordered-wide / remove64-front | 4,537 | 4,457 | 0.982 | 0.990 | 0.998 | 377,822 | 377,740 | 1.000 |
| ordered-wide / move64-front | 16,378 | 15,166 | 0.926 | 0.938 | 0.977 | 389,811 | 388,637 | 0.997 |
| ordered-wide / insert64-middle | 6,216 | 6,227 | 1.002 | 0.999 | 1.000 | 192,365 | 192,467 | 1.001 |
| ordered-wide / remove64-middle | 2,605 | 2,427 | 0.932 | 0.953 | 0.957 | 3,546 | 3,412 | 0.962 |
| ordered-wide / move64-middle | 15,514 | 14,206 | 0.916 | 0.931 | 0.973 | 388,165 | 386,729 | 0.996 |
| ordered-wide / insert64-end | 5,837 | 5,901 | 1.011 | 1.001 | 1.019 | 5,663 | 5,722 | 1.010 |
| ordered-wide / remove64-end | 1,806 | 1,760 | 0.975 | 0.976 | 0.974 | 880 | 887 | 1.008 |
| ordered-wide / move64-end | 14,625 | 13,405 | 0.917 | 0.932 | 0.974 | 385,343 | 384,171 | 0.997 |
| ordered-wide / append1 | 546 | 554 | 1.015 | 1.016 | 1.013 | 2,471 | 2,510 | 1.016 |
| ordered-wide / translate64 | 1,457 | 1,453 | 0.997 | 0.997 | 0.997 | 1,540 | 1,535 | 0.997 |
| ordered-wide / translate-half | 2,502 | 2,485 | 0.993 | 0.985 | 0.982 | 865,068 | 849,441 | 0.982 |
| ordered-wide / bulk-build | 97,867 | 98,722 | 1.009 | 1.011 | 1.011 | 85,468,095 | 86,301,622 | 1.010 |
| ordered-wide / build-append8 | 196,502 | 198,204 | 1.009 | 1.009 | 1.009 | 186,209,576 | 187,616,452 | 1.008 |
| ordered-wide / freeze-cold | 26,956 | 26,970 | 1.001 | 1.000 | 1.000 | 24,565,679 | 24,512,328 | 0.998 |
| ordered-wide / freeze-idle | 1,497 | 1,507 | 1.007 | 1.002 | 1.000 | 472,434 | 472,444 | 1.000 |
| ordered-wide / freeze-one | 1,795 | 1,805 | 1.006 | 1.002 | 1.000 | 472,732 | 472,743 | 1.000 |
| ordered-wide / freeze-all | 6,436 | 6,446 | 1.002 | 1.000 | 1.000 | 5,003,983 | 5,003,993 | 1.000 |
| ordered-wide / parent-last | 229 | 229 | 1.000 | 1.000 | 1.000 | 15,900 | 15,900 | 1.000 |
| ordered-wide / ancestors-last | 295 | 295 | 1.000 | 1.000 | 1.000 | 15,965 | 15,965 | 1.000 |
| ordered-wide / subtree-root | 205 | 205 | 1.000 | 1.000 | 1.000 | 17,812 | 17,812 | 1.000 |
| ordered-wide / subtree-leaf | 114 | 114 | 1.000 | 1.000 | 1.000 | 153 | 153 | 1.000 |
| ordered-chains32 / parent-last | 88 | 88 | 1.000 | 1.000 | 1.000 | 161 | 161 | 1.000 |
| ordered-chains32 / ancestors-last | 475 | 475 | 1.000 | 1.000 | 1.000 | 16,841 | 16,841 | 1.000 |
| ordered-chains32 / subtree-root | 205 | 205 | 1.000 | 1.000 | 1.000 | 17,812 | 17,812 | 1.000 |
| ordered-chains32 / subtree-leaf | 114 | 114 | 1.000 | 1.000 | 1.000 | 153 | 153 | 1.000 |
| ordered-wide / set-ink-pos | 421 | 371 | 0.881 | 0.890 | 0.895 | 502 | 452 | 0.900 |
| ordered-wide / set-ink-handle | 387 | 337 | 0.871 | 0.871 | 0.871 | 387 | 337 | 0.871 |
| ordered-wide / pos-of-handle | 24 | 24 | 1.000 | 1.000 | 1.000 | 24 | 24 | 1.000 |
| ordered-wide / touch-handle | 131 | 117 | 0.893 (-14) | 0.893 (-14) | 0.893 (-14) | 131 | 117 | 0.893 (-14) |
| ordered-wide / set-inks-handles64 | 19,763 | 18,664 | 0.944 | 0.924 | 0.936 | 37,580 | 35,713 | 0.950 |
| ordered-wide / insert8-front+pos64 | 5,907 | 5,927 | 1.003 | 1.002 | 1.000 | 377,101 | 377,123 | 1.000 |
| ordered-wide / insert8-front+posall | 4,412 | 4,432 | 1.005 | 1.002 | 1.000 | 785,587 | 785,609 | 1.000 |

## Spatial

| shape / operation | 1e3 base | 1e3 new | 1e3 | 1e4 | 1e5 | 1e6 base | 1e6 new | 1e6 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ordered-wide / cull-1-band | 369 | 353 | 0.957 | 0.951 | 0.932 | 57,860 | 53,792 | 0.930 |
| ordered-wide / cull-1-dispersed | 41,205 | 40,471 | 0.982 | 0.956 | 0.950 | 17,529,740 | 16,639,050 | 0.949 |
| ordered-wide / cull-10-band | 846 | 801 | 0.947 | 0.932 | 0.929 | 570,976 | 530,372 | 0.929 |
| ordered-wide / cull-10-dispersed | 82,857 | 82,835 | 1.000 | 0.967 | 0.953 | 17,668,106 | 16,776,202 | 0.950 |
| ordered-wide / cull-100-band | 5,928 | 5,519 | 0.931 | 0.929 | 0.929 | 5,702,180 | 5,296,180 | 0.929 |
| ordered-wide / cull-100-dispersed | 5,928 | 5,519 | 0.931 | 0.929 | 0.929 | 5,702,180 | 5,296,180 | 0.929 |
| ordered-wide / hit-present | 325 | 303 | 0.932 | 0.917 | 0.923 | 988 | 949 | 0.961 |
| ordered-wide / hit-gap | 306 | 286 | 0.935 | 0.947 | 0.952 | 813 | 790 | 0.972 |
| interleaved-wide / cull-1-band | 525 | 472 | 0.899 | 0.850 | 0.839 | 332,305 | 278,556 | 0.838 |
| interleaved-wide / cull-1-dispersed | 48,299 | 47,596 | 0.985 | 0.984 | 0.984 | 45,776,125 | 45,050,930 | 0.984 |
| interleaved-wide / cull-10-band | 867 | 805 | 0.928 | 0.896 | 0.893 | 845,780 | 755,080 | 0.893 |
| interleaved-wide / cull-10-dispersed | 88,079 | 88,056 | 1.000 | 1.000 | 1.000 | 87,288,896 | 87,272,177 | 1.000 |
| interleaved-wide / cull-100-band | 5,928 | 5,519 | 0.931 | 0.929 | 0.929 | 5,702,180 | 5,296,180 | 0.929 |
| interleaved-wide / cull-100-dispersed | 5,928 | 5,519 | 0.931 | 0.929 | 0.929 | 5,702,180 | 5,296,180 | 0.929 |
| interleaved-wide / hit-present | 299 | 277 | 0.926 | 0.822 | 0.820 | 1,527 | 1,298 | 0.850 |
| interleaved-wide / hit-gap | 306 | 286 | 0.935 | 0.824 | 0.787 | 123,870 | 97,103 | 0.784 |

## Cells above 1.02x

None.

## Above 1.02x but inside the cell's own scatter (`~`)

The larger of the two tags' run-to-run spread and sample noise exceeds the excess: not a measured regression.

* fresh-core ordered-wide / insert8-middle N=1,000,000: 276,001 -> 288,813 (1.046x; scatter 5%)
* fresh-core ordered-wide / insert8-front N=1,000,000: 454,350 -> 475,040 (1.046x; scatter 20%)
* fresh-core ordered-wide / insert8-front+pos64 N=1,000,000: 457,881 -> 474,630 (1.037x; scatter 9%)

