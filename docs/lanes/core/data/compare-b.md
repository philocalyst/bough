# `b` against `a`

new / base per cell (values as in the matrix). **bold**: above 1.02x; `~`: above 1.02x but inside the cell's own scatter. Cells under 200 instructions
are counter-dominated: `+k` is the absolute difference in instructions.

## Structure, fresh tree

| shape / operation | 1e3 base | 1e3 new | 1e3 | 1e4 | 1e5 | 1e6 base | 1e6 new | 1e6 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ordered-wide / insert8-front | 8,366 | 8,422 | 1.007 | **1.064** | 0.973 | 454,350 | 416,046 | 0.916 |
| ordered-wide / remove8-front | 1,556 | 1,556 | 1.000 | 0.978 | 0.999 | 2,516 | 2,515 | 1.000 |
| ordered-wide / move8-front | 3,701 | 3,705 | 1.001 | 0.997 | 1.012 | 106,146 | 106,403 | 1.002 |
| ordered-wide / insert8-middle | 7,950 | 7,969 | 1.002 | **1.084** | 1.009 | 276,001 | 261,262 | 0.947 |
| ordered-wide / remove8-middle | 1,156 | 1,156 | 1.000 | 0.977 | 0.999 | 1,867 | 1,866 | 0.999 |
| ordered-wide / move8-middle | 10,646 | 10,668 | 1.002 | **1.050** | 0.974 | 478,052 | 431,644 | 0.903 |
| ordered-wide / insert8-end | 1,058 | 1,060 | 1.002 | **1.020** | **1.041** | 102,609 | 102,721 | 1.001 |
| ordered-wide / remove8-end | 824 | 823 | 0.999 | 0.961 | 0.998 | 854 | 852 | 0.998 |
| ordered-wide / move8-end | 10,314 | 10,335 | 1.002 | **1.052** | 0.974 | 476,996 | 433,270 | 0.908 |
| ordered-wide / insert64-front | 11,322 | 11,345 | 1.002 | **1.052** | 0.974 | 477,982 | 431,426 | 0.903 |
| ordered-wide / remove64-front | 4,717 | 4,792 | 1.016 | **1.029** | 0.927 | 377,999 | 340,983 | 0.902 |
| ordered-wide / move64-front | 16,566 | 16,664 | 1.006 | 1.016 | 0.946 | 389,989 | 353,267 | 0.906 |
| ordered-wide / insert64-middle | 10,856 | 10,875 | 1.002 | **1.067** | 1.009 | 292,190 | 270,605 | 0.926 |
| ordered-wide / remove64-middle | 2,603 | 2,488 | 0.956 | 0.986 | 0.990 | 3,546 | 3,554 | 1.002 |
| ordered-wide / move64-middle | 20,195 | 20,137 | 0.997 | **1.031** | 0.977 | 487,850 | 441,756 | 0.906 |
| ordered-wide / insert64-end | 10,471 | 10,490 | 1.002 | 1.011 | 1.007 | 105,480 | 93,440 | 0.886 |
| ordered-wide / remove64-end | 1,984 | 2,058 | **1.037** | **1.103** | **1.173** | 1,058 | 1,493 | **1.411** |
| ordered-wide / move64-end | 14,805 | 14,979 | 1.012 | **1.059** | 0.976 | 385,523 | 352,702 | 0.915 |
| ordered-wide / append1 | 545 | 548 | 1.006 | **1.038** | **1.076** | 102,288 | 102,603 | 1.003 |
| ordered-wide / translate64 | 1,457 | 1,428 | 0.980 | 1.005 | **1.037** | 1,540 | 1,644 | **1.068** |
| ordered-wide / translate-half | 2,503 | 2,474 | 0.988 | 1.000 | 1.000 | 865,068 | 864,196 | 0.999 |
| ordered-wide / bulk-build | 97,861 | 98,360 | 1.005 | 1.001 | 1.000 | 85,514,120 | 85,493,442 | 1.000 |
| ordered-wide / build-append8 | 196,372 | 197,419 | 1.005 | 1.010 | **1.029** | 186,784,546 | 190,949,378 | **1.022** |
| ordered-wide / freeze-cold | 26,951 | 27,354 | 1.015 | 1.002 | 1.000 | 24,581,794 | 24,593,640 | 1.000 |
| ordered-wide / freeze-idle | 1,497 | 1,532 | **1.023** | 1.007 | 1.001 | 472,434 | 472,650 | 1.000 |
| ordered-wide / freeze-one | 1,795 | 1,830 | 1.019 | 1.006 | 1.001 | 472,733 | 472,948 | 1.000 |
| ordered-wide / freeze-all | 6,436 | 6,471 | 1.005 | 1.001 | 1.000 | 5,003,983 | 5,004,199 | 1.000 |
| ordered-wide / parent-last | 229 | 229 | 1.000 | 1.000 | 1.000 | 15,900 | 15,900 | 1.000 |
| ordered-wide / ancestors-last | 295 | 295 | 1.000 | 1.000 | 1.000 | 15,965 | 15,965 | 1.000 |
| ordered-wide / subtree-root | 205 | 205 | 1.000 | 1.000 | 1.000 | 17,812 | 17,812 | 1.000 |
| ordered-wide / subtree-leaf | 114 | 114 | 1.000 | 1.000 | 1.000 | 153 | 153 | 1.000 |
| ordered-chains32 / parent-last | 88 | 88 | 1.000 | 1.000 | 1.000 | 161 | 161 | 1.000 |
| ordered-chains32 / ancestors-last | 475 | 475 | 1.000 | 1.000 | 1.000 | 16,841 | 16,841 | 1.000 |
| ordered-chains32 / subtree-root | 205 | 205 | 1.000 | 1.000 | 1.000 | 17,812 | 17,812 | 1.000 |
| ordered-chains32 / subtree-leaf | 114 | 114 | 1.000 | 1.000 | 1.000 | 153 | 153 | 1.000 |
| ordered-wide / set-ink-pos | 421 | 423 | 1.005 | 1.004 | 1.004 | 502 | 504 | 1.004 |
| ordered-wide / set-ink-handle | 387 | 389 | 1.005 | 1.005 | 1.005 | 387 | 389 | 1.005 |
| ordered-wide / pos-of-handle | 24 | 24 | 1.000 | 1.000 | 1.000 | 24 | 24 | 1.000 |
| ordered-wide / touch-handle | 131 | 131 | 1.000 | 1.000 | 1.000 | 131 | 131 | 1.000 |
| ordered-wide / set-inks-handles64 | 19,763 | 19,771 | 1.000 | **1.039** | **1.037** | 37,580 | 37,706 | 1.003 |
| ordered-wide / insert8-front+pos64 | 10,559 | 10,579 | 1.002 | **1.048** | 0.974 | 457,881 | 424,591 | 0.927 |
| ordered-wide / insert8-front+posall | 9,064 | 9,084 | 1.002 | **1.047** | 0.985 | 898,433 | 838,055 | 0.933 |

## Structure, warm columns

| shape / operation | 1e3 base | 1e3 new | 1e3 | 1e4 | 1e5 | 1e6 base | 1e6 new | 1e6 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ordered-wide / insert8-front | 3,728 | 3,784 | 1.015 | **1.120** | 0.963 | 375,195 | 341,693 | 0.911 |
| ordered-wide / remove8-front | 1,520 | 1,555 | 1.023~ | 0.978 | 0.999 | 2,516 | 2,515 | 1.000 |
| ordered-wide / move8-front | 3,699 | 3,706 | 1.002 | 0.997 | 1.012 | 6,364 | 6,586 | **1.035** |
| ordered-wide / insert8-middle | 3,275 | 3,331 | 1.017 | **1.190** | 1.016 | 188,961 | 174,052 | 0.921 |
| ordered-wide / remove8-middle | 1,117 | 1,156 | 1.035~ | 0.978 | 0.999 | 1,867 | 1,866 | 0.999 |
| ordered-wide / move8-middle | 6,005 | 6,028 | 1.004 | **1.082** | 0.965 | 378,226 | 344,646 | 0.911 |
| ordered-wide / insert8-end | 1,058 | 1,060 | 1.002 | **1.021** | **1.041** | 2,684 | 2,903 | **1.082** |
| ordered-wide / remove8-end | 824 | 823 | 0.999 | 0.962 | 0.998 | 854 | 852 | 0.998 |
| ordered-wide / move8-end | 5,672 | 5,696 | 1.004 | **1.088** | 0.964 | 377,207 | 343,627 | 0.911 |
| ordered-wide / insert64-front | 6,686 | 6,707 | 1.003 | **1.085** | 0.965 | 378,116 | 344,598 | 0.911 |
| ordered-wide / remove64-front | 4,537 | 4,613 | 1.017 | **1.030** | 0.927 | 377,822 | 340,804 | 0.902 |
| ordered-wide / move64-front | 16,378 | 16,521 | 1.009 | 1.016 | 0.946 | 389,811 | 353,104 | 0.906 |
| ordered-wide / insert64-middle | 6,216 | 6,237 | 1.003 | **1.121** | 1.014 | 192,365 | 177,457 | 0.923 |
| ordered-wide / remove64-middle | 2,605 | 2,488 | 0.955 | 0.986 | 0.990 | 3,546 | 3,554 | 1.002 |
| ordered-wide / move64-middle | 15,514 | 15,499 | 0.999 | **1.043** | 0.970 | 388,165 | 354,782 | 0.914 |
| ordered-wide / insert64-end | 5,837 | 5,860 | 1.004 | 1.013 | **1.024** | 5,663 | 5,881 | **1.038** |
| ordered-wide / remove64-end | 1,806 | 1,879 | **1.040** | **1.113** | **1.190** | 880 | 1,314 | **1.493** |
| ordered-wide / move64-end | 14,625 | 14,723 | 1.007 | **1.059** | 0.978 | 385,343 | 352,536 | 0.915 |
| ordered-wide / append1 | 546 | 548 | 1.004 | **1.040** | **1.076** | 2,471 | 2,691 | **1.089** |
| ordered-wide / translate64 | 1,457 | 1,428 | 0.980 | 1.005 | **1.037** | 1,540 | 1,644 | **1.068** |
| ordered-wide / translate-half | 2,502 | 2,474 | 0.989 | 1.000 | 1.000 | 865,068 | 864,196 | 0.999 |
| ordered-wide / bulk-build | 97,867 | 98,331 | 1.005 | 1.001 | 1.000 | 85,468,095 | 85,493,819 | 1.000 |
| ordered-wide / build-append8 | 196,502 | 197,376 | 1.004 | 1.010 | **1.029** | 186,209,576 | 190,913,378 | **1.025** |
| ordered-wide / freeze-cold | 26,956 | 27,351 | 1.015 | 1.002 | 1.000 | 24,565,679 | 24,521,711 | 0.998 |
| ordered-wide / freeze-idle | 1,497 | 1,532 | **1.023** | 1.007 | 1.001 | 472,434 | 472,651 | 1.000 |
| ordered-wide / freeze-one | 1,795 | 1,830 | 1.019 | 1.007 | 1.001 | 472,732 | 472,949 | 1.000 |
| ordered-wide / freeze-all | 6,436 | 6,471 | 1.005 | 1.001 | 1.000 | 5,003,983 | 5,004,199 | 1.000 |
| ordered-wide / parent-last | 229 | 229 | 1.000 | 1.000 | 1.000 | 15,900 | 15,900 | 1.000 |
| ordered-wide / ancestors-last | 295 | 295 | 1.000 | 1.000 | 1.000 | 15,965 | 15,965 | 1.000 |
| ordered-wide / subtree-root | 205 | 205 | 1.000 | 1.000 | 1.000 | 17,812 | 17,812 | 1.000 |
| ordered-wide / subtree-leaf | 114 | 114 | 1.000 | 1.000 | 1.000 | 153 | 153 | 1.000 |
| ordered-chains32 / parent-last | 88 | 88 | 1.000 | 1.000 | 1.000 | 161 | 161 | 1.000 |
| ordered-chains32 / ancestors-last | 475 | 475 | 1.000 | 1.000 | 1.000 | 16,841 | 16,841 | 1.000 |
| ordered-chains32 / subtree-root | 205 | 205 | 1.000 | 1.000 | 1.000 | 17,812 | 17,812 | 1.000 |
| ordered-chains32 / subtree-leaf | 114 | 114 | 1.000 | 1.000 | 1.000 | 153 | 153 | 1.000 |
| ordered-wide / set-ink-pos | 421 | 423 | 1.005 | 1.004 | 1.004 | 502 | 504 | 1.004 |
| ordered-wide / set-ink-handle | 387 | 389 | 1.005 | 1.005 | 1.005 | 387 | 389 | 1.005 |
| ordered-wide / pos-of-handle | 24 | 24 | 1.000 | 1.000 | 1.000 | 24 | 24 | 1.000 |
| ordered-wide / touch-handle | 131 | 131 | 1.000 | 1.000 | 1.000 | 131 | 131 | 1.000 |
| ordered-wide / set-inks-handles64 | 19,763 | 19,771 | 1.000 | **1.039** | **1.037** | 37,580 | 37,706 | 1.003 |
| ordered-wide / insert8-front+pos64 | 5,907 | 5,927 | 1.003 | **1.095** | 0.965 | 377,101 | 343,762 | 0.912 |
| ordered-wide / insert8-front+posall | 4,412 | 4,432 | 1.005 | **1.074** | 0.982 | 785,587 | 752,143 | 0.957 |

## Spatial

| shape / operation | 1e3 base | 1e3 new | 1e3 | 1e4 | 1e5 | 1e6 base | 1e6 new | 1e6 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ordered-wide / cull-1-band | 369 | 363 | 0.984 | 0.987 | 0.979 | 57,860 | 56,610 | 0.978 |
| ordered-wide / cull-1-dispersed | 41,205 | 41,155 | 0.999 | 1.010 | 1.012 | 17,529,740 | 17,748,412 | 1.012 |
| ordered-wide / cull-10-band | 846 | 829 | 0.980 | 0.979 | 0.978 | 570,976 | 558,432 | 0.978 |
| ordered-wide / cull-10-dispersed | 82,857 | 82,896 | 1.000 | 1.010 | 1.012 | 17,668,106 | 17,888,630 | 1.012 |
| ordered-wide / cull-100-band | 5,928 | 5,799 | 0.978 | 0.978 | 0.978 | 5,702,180 | 5,576,688 | 0.978 |
| ordered-wide / cull-100-dispersed | 5,928 | 5,799 | 0.978 | 0.978 | 0.978 | 5,702,180 | 5,576,688 | 0.978 |
| ordered-wide / hit-present | 325 | 328 | 1.009 | 0.982 | 0.983 | 988 | 979 | 0.991 |
| ordered-wide / hit-gap | 306 | 308 | 1.007 | 0.976 | 0.978 | 813 | 804 | 0.989 |
| interleaved-wide / cull-1-band | 525 | 518 | 0.987 | 0.980 | 0.988 | 332,305 | 328,212 | 0.988 |
| interleaved-wide / cull-1-dispersed | 48,299 | 48,176 | 0.997 | 0.997 | 0.997 | 45,776,125 | 45,656,167 | 0.997 |
| interleaved-wide / cull-10-band | 867 | 851 | 0.982 | 0.979 | 0.982 | 845,780 | 830,388 | 0.982 |
| interleaved-wide / cull-10-dispersed | 88,079 | 88,041 | 1.000 | 1.000 | 1.000 | 87,288,896 | 87,257,707 | 1.000 |
| interleaved-wide / cull-100-band | 5,928 | 5,799 | 0.978 | 0.978 | 0.978 | 5,702,180 | 5,576,688 | 0.978 |
| interleaved-wide / cull-100-dispersed | 5,928 | 5,799 | 0.978 | 0.978 | 0.978 | 5,702,180 | 5,576,688 | 0.978 |
| interleaved-wide / hit-present | 299 | 302 | 1.010 | 0.994 | 1.000 | 1,527 | 1,523 | 0.997 |
| interleaved-wide / hit-gap | 306 | 308 | 1.007 | 0.993 | 0.998 | 123,870 | 123,609 | 0.998 |

## Cells above 1.02x

* warm-core ordered-wide / remove64-end N=1,000,000: 880 -> 1,314 (1.493x)
* fresh-core ordered-wide / remove64-end N=1,000,000: 1,058 -> 1,493 (1.411x)
* warm-core ordered-wide / remove64-end N=100,000: 1,822 -> 2,169 (1.190x)
* warm-core ordered-wide / insert8-middle N=10,000: 5,128 -> 6,103 (1.190x)
* fresh-core ordered-wide / remove64-end N=100,000: 2,001 -> 2,348 (1.173x)
* warm-core ordered-wide / insert64-middle N=10,000: 8,075 -> 9,052 (1.121x)
* warm-core ordered-wide / insert8-front N=10,000: 7,031 -> 7,877 (1.120x)
* warm-core ordered-wide / remove64-end N=10,000: 1,944 -> 2,164 (1.113x)
* fresh-core ordered-wide / remove64-end N=10,000: 2,124 -> 2,343 (1.103x)
* warm-core ordered-wide / insert8-front+pos64 N=10,000: 8,938 -> 9,785 (1.095x)
* warm-core ordered-wide / append1 N=1,000,000: 2,471 -> 2,691 (1.089x)
* warm-core ordered-wide / move8-end N=10,000: 9,190 -> 9,999 (1.088x)
* warm-core ordered-wide / insert64-front N=10,000: 9,954 -> 10,800 (1.085x)
* fresh-core ordered-wide / insert8-middle N=10,000: 12,808 -> 13,884 (1.084x)
* warm-core ordered-wide / insert8-end N=1,000,000: 2,684 -> 2,903 (1.082x)
* warm-core ordered-wide / move8-middle N=10,000: 9,929 -> 10,739 (1.082x)
* warm-core ordered-wide / append1 N=100,000: 603 -> 649 (1.076x)
* fresh-core ordered-wide / append1 N=100,000: 603 -> 649 (1.076x)
* warm-core ordered-wide / insert8-front+posall N=10,000: 11,390 -> 12,235 (1.074x)
* warm-core ordered-wide / translate64 N=1,000,000: 1,540 -> 1,644 (1.068x)
* fresh-core ordered-wide / translate64 N=1,000,000: 1,540 -> 1,644 (1.068x)
* fresh-core ordered-wide / insert64-middle N=10,000: 15,755 -> 16,803 (1.067x)
* fresh-core ordered-wide / insert8-front N=10,000: 14,711 -> 15,658 (1.064x)
* warm-core ordered-wide / move64-end N=10,000: 17,999 -> 19,066 (1.059x)
* fresh-core ordered-wide / move64-end N=10,000: 18,252 -> 19,323 (1.059x)
* fresh-core ordered-wide / move8-end N=10,000: 16,871 -> 17,749 (1.052x)
* fresh-core ordered-wide / insert64-front N=10,000: 17,634 -> 18,551 (1.052x)
* fresh-core ordered-wide / move8-middle N=10,000: 17,609 -> 18,490 (1.050x)
* fresh-core ordered-wide / insert8-front+pos64 N=10,000: 16,534 -> 17,328 (1.048x)
* fresh-core ordered-wide / insert8-front+posall N=10,000: 18,618 -> 19,492 (1.047x)
* warm-core ordered-wide / move64-middle N=10,000: 18,932 -> 19,744 (1.043x)
* warm-core ordered-wide / insert8-end N=100,000: 1,115 -> 1,161 (1.041x)
* fresh-core ordered-wide / insert8-end N=100,000: 1,115 -> 1,161 (1.041x)
* warm-core ordered-wide / remove64-end N=1,000: 1,806 -> 1,879 (1.040x)
* warm-core ordered-wide / append1 N=10,000: 578 -> 601 (1.040x)
* warm-core ordered-wide / set-inks-handles64 N=10,000: 24,688 -> 25,646 (1.039x)
* fresh-core ordered-wide / set-inks-handles64 N=10,000: 24,688 -> 25,646 (1.039x)
* warm-core ordered-wide / insert64-end N=1,000,000: 5,663 -> 5,881 (1.038x)
* fresh-core ordered-wide / append1 N=10,000: 579 -> 601 (1.038x)
* fresh-core ordered-wide / remove64-end N=1,000: 1,984 -> 2,058 (1.037x)
* warm-core ordered-wide / set-inks-handles64 N=100,000: 29,348 -> 30,434 (1.037x)
* fresh-core ordered-wide / set-inks-handles64 N=100,000: 29,348 -> 30,434 (1.037x)
* warm-core ordered-wide / translate64 N=100,000: 1,516 -> 1,572 (1.037x)
* fresh-core ordered-wide / translate64 N=100,000: 1,516 -> 1,572 (1.037x)
* warm-core ordered-wide / move8-front N=1,000,000: 6,364 -> 6,586 (1.035x)
* fresh-core ordered-wide / move64-middle N=10,000: 26,890 -> 27,735 (1.031x)
* warm-core ordered-wide / remove64-front N=10,000: 7,975 -> 8,216 (1.030x)
* fresh-core ordered-wide / remove64-front N=10,000: 8,155 -> 8,394 (1.029x)
* warm-core ordered-wide / build-append8 N=100,000: 18,305,148 -> 18,839,403 (1.029x)
* fresh-core ordered-wide / build-append8 N=100,000: 18,305,785 -> 18,839,349 (1.029x)
* warm-core ordered-wide / build-append8 N=1,000,000: 186,209,576 -> 190,913,378 (1.025x)
* warm-core ordered-wide / insert64-end N=100,000: 6,052 -> 6,199 (1.024x)
* warm-core ordered-wide / freeze-idle N=1,000: 1,497 -> 1,532 (1.023x)
* fresh-core ordered-wide / freeze-idle N=1,000: 1,497 -> 1,532 (1.023x)
* fresh-core ordered-wide / build-append8 N=1,000,000: 186,784,546 -> 190,949,378 (1.022x)
* warm-core ordered-wide / insert8-end N=10,000: 1,090 -> 1,113 (1.021x)
* fresh-core ordered-wide / insert8-end N=10,000: 1,091 -> 1,113 (1.020x)

## Above 1.02x but inside the cell's own scatter (`~`)

The larger of the two tags' run-to-run spread and sample noise exceeds the excess: not a measured regression.

* warm-core ordered-wide / remove8-middle N=1,000: 1,117 -> 1,156 (1.035x; scatter 4%)
* warm-core ordered-wide / remove8-front N=1,000: 1,520 -> 1,555 (1.023x; scatter 2%)

