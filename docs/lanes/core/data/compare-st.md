# `st` against `f13`

new / base per cell (values as in the matrix). **bold**: above 1.02x; `~`: above 1.02x but inside the cell's own scatter. Cells under 200 instructions
are counter-dominated: `+k` is the absolute difference in instructions.

## Structure, fresh tree

| shape / operation | 1e3 base | 1e3 new | 1e3 | 1e4 | 1e5 | 1e6 base | 1e6 new | 1e6 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ordered-wide / insert8-front | 8,423 | 8,449 | 1.003 | 1.003 | 1.000 | 475,041 | 475,140 | 1.000 |
| ordered-wide / remove8-front | 1,479 | 1,487 | 1.005 | 1.003 | 1.005 | 2,438 | 2,446 | 1.003 |
| ordered-wide / move8-front | 3,472 | 3,504 | 1.009 | 1.008 | 1.009 | 105,971 | 93,249 | 0.880 |
| ordered-wide / insert8-middle | 7,975 | 7,995 | 1.003 | 1.003 | 1.003 | 288,813 | 275,779 | 0.955 |
| ordered-wide / remove8-middle | 1,053 | 1,062 | 1.009 | 1.003 | 1.006 | 1,764 | 1,772 | 1.005 |
| ordered-wide / move8-middle | 10,404 | 10,458 | 1.005 | 1.004 | 1.002 | 475,221 | 464,858 | 0.978 |
| ordered-wide / insert8-end | 1,066 | 1,072 | 1.006 | 1.005 | 1.005 | 89,648 | 89,559 | 0.999 |
| ordered-wide / remove8-end | 771 | 779 | 1.010 | 1.007 | 1.010 | 800 | 808 | 1.010 |
| ordered-wide / move8-end | 10,122 | 10,174 | 1.005 | 1.004 | 1.001 | 489,206 | 464,002 | 0.948 |
| ordered-wide / insert64-front | 11,317 | 11,338 | 1.002 | 1.002 | 1.000 | 477,920 | 465,115 | 0.973 |
| ordered-wide / remove64-front | 4,638 | 4,660 | 1.005 | 1.003 | 1.001 | 377,920 | 377,943 | 1.000 |
| ordered-wide / move64-front | 15,383 | 15,450 | 1.004 | 1.003 | 1.001 | 388,886 | 388,997 | 1.000 |
| ordered-wide / insert64-middle | 10,903 | 10,923 | 1.002 | 1.003 | 1.002 | 292,209 | 279,473 | 0.956 |
| ordered-wide / remove64-middle | 2,471 | 2,484 | 1.005 | 1.005 | 1.004 | 3,412 | 3,424 | 1.004 |
| ordered-wide / move64-middle | 18,883 | 18,972 | 1.005 | 1.002 | 1.002 | 460,996 | 486,598 | 1.056~ |
| ordered-wide / insert64-end | 10,533 | 10,555 | 1.002 | 1.003 | 1.004 | 93,564 | 105,564 | 1.128~ |
| ordered-wide / remove64-end | 1,941 | 1,953 | 1.006 | 1.006 | 1.006 | 1,066 | 1,076 | 1.009 |
| ordered-wide / move64-end | 13,622 | 13,709 | 1.006 | 1.003 | 1.001 | 384,205 | 384,447 | 1.001 |
| ordered-wide / append1 | 554 | 560 | 1.011 | 1.009 | 1.010 | 100,252 | 89,510 | 0.893 |
| ordered-wide / translate64 | 1,453 | 1,453 | 1.000 | 0.999 | 1.000 | 1,535 | 1,535 | 1.000 |
| ordered-wide / translate-half | 2,039 | 2,038 | 1.000 | 1.000 | 1.000 | 271,499 | 271,499 | 1.000 |
| ordered-wide / bulk-build | 98,775 | 99,141 | 1.004 | 1.002 | 1.002 | 86,054,883 | 86,524,647 | 1.005 |
| ordered-wide / build-append8 | 198,122 | 199,352 | 1.006 | 1.006 | 1.006 | 187,656,027 | 188,316,844 | 1.004 |
| ordered-wide / freeze-cold | 26,969 | 26,980 | 1.000 | 1.001 | 1.001 | 24,430,537 | 24,533,566 | 1.004 |
| ordered-wide / freeze-idle | 1,507 | 1,509 | 1.001 | 1.000 | 1.000 | 472,444 | 472,446 | 1.000 |
| ordered-wide / freeze-one | 1,805 | 1,807 | 1.001 | 1.000 | 1.000 | 472,742 | 472,744 | 1.000 |
| ordered-wide / freeze-all | 6,446 | 6,448 | 1.000 | 1.000 | 1.000 | 5,003,994 | 5,003,995 | 1.000 |
| ordered-wide / parent-last | 229 | 229 | 1.000 | 1.000 | 1.000 | 15,900 | 15,900 | 1.000 |
| ordered-wide / ancestors-last | 295 | 295 | 1.000 | 1.000 | 1.000 | 15,965 | 15,965 | 1.000 |
| ordered-wide / subtree-root | 205 | 205 | 1.000 | 1.000 | 1.000 | 17,812 | 17,812 | 1.000 |
| ordered-wide / subtree-leaf | 114 | 114 | 1.000 | 1.000 | 1.000 | 153 | 153 | 1.000 |
| ordered-chains32 / parent-last | 88 | 88 | 1.000 | 1.000 | 1.000 | 161 | 161 | 1.000 |
| ordered-chains32 / ancestors-last | 475 | 475 | 1.000 | 1.000 | 1.000 | 16,841 | 16,841 | 1.000 |
| ordered-chains32 / subtree-root | 205 | 205 | 1.000 | 1.000 | 1.000 | 17,812 | 17,812 | 1.000 |
| ordered-chains32 / subtree-leaf | 114 | 114 | 1.000 | 1.000 | 1.000 | 153 | 153 | 1.000 |
| ordered-wide / set-ink-pos | 371 | 371 | 1.000 | 1.000 | 1.000 | 452 | 452 | 1.000 |
| ordered-wide / set-ink-handle | 337 | 337 | 1.000 | 1.000 | 1.000 | 337 | 337 | 1.000 |
| ordered-wide / pos-of-handle | 24 | 24 | 1.000 | 1.000 | 1.000 | 24 | 24 | 1.000 |
| ordered-wide / touch-handle | 117 | 117 | 1.000 | 1.000 | 1.000 | 117 | 117 | 1.000 |
| ordered-wide / set-inks-handles64 | 18,664 | 18,668 | 1.000 | 1.000 | 1.000 | 35,713 | 35,717 | 1.000 |
| ordered-wide / insert8-front+pos64 | 10,581 | 10,606 | 1.002 | 1.003 | 1.001 | 459,428 | 463,802 | 1.010 |
| ordered-wide / insert8-front+posall | 9,086 | 9,110 | 1.003 | 1.003 | 1.000 | 885,277 | 901,575 | 1.018 |

## Structure, warm columns

| shape / operation | 1e3 base | 1e3 new | 1e3 | 1e4 | 1e5 | 1e6 base | 1e6 new | 1e6 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ordered-wide / insert8-front | 3,784 | 3,810 | 1.007 | 1.004 | 1.001 | 375,213 | 375,241 | 1.000 |
| ordered-wide / remove8-front | 1,479 | 1,487 | 1.005 | 1.003 | 1.005 | 2,438 | 2,446 | 1.003 |
| ordered-wide / move8-front | 3,472 | 3,501 | 1.008 | 1.008 | 1.009 | 6,167 | 6,216 | 1.008 |
| ordered-wide / insert8-middle | 3,335 | 3,355 | 1.006 | 1.005 | 1.001 | 188,988 | 189,010 | 1.000 |
| ordered-wide / remove8-middle | 1,053 | 1,062 | 1.009 | 1.004 | 1.006 | 1,764 | 1,772 | 1.005 |
| ordered-wide / move8-middle | 5,764 | 5,817 | 1.009 | 1.005 | 1.001 | 377,985 | 378,037 | 1.000 |
| ordered-wide / insert8-end | 1,066 | 1,072 | 1.006 | 1.005 | 1.005 | 2,722 | 2,746 | 1.009 |
| ordered-wide / remove8-end | 771 | 779 | 1.010 | 1.007 | 1.010 | 799 | 807 | 1.010 |
| ordered-wide / move8-end | 5,482 | 5,534 | 1.009 | 1.006 | 1.001 | 377,015 | 377,067 | 1.000 |
| ordered-wide / insert64-front | 6,677 | 6,699 | 1.003 | 1.002 | 1.001 | 378,108 | 378,130 | 1.000 |
| ordered-wide / remove64-front | 4,457 | 4,481 | 1.005 | 1.003 | 1.001 | 377,740 | 377,764 | 1.000 |
| ordered-wide / move64-front | 15,407 | 15,263 | 0.991 | 1.004 | 1.004 | 388,704 | 388,833 | 1.000 |
| ordered-wide / insert64-middle | 6,263 | 6,284 | 1.003 | 1.003 | 1.001 | 192,467 | 192,488 | 1.000 |
| ordered-wide / remove64-middle | 2,471 | 2,484 | 1.005 | 1.004 | 1.004 | 3,412 | 3,424 | 1.004 |
| ordered-wide / move64-middle | 14,206 | 14,333 | 1.009 | 1.004 | 1.002 | 386,729 | 386,913 | 1.000 |
| ordered-wide / insert64-end | 5,901 | 5,924 | 1.004 | 1.004 | 1.004 | 5,721 | 5,746 | 1.004 |
| ordered-wide / remove64-end | 1,760 | 1,773 | 1.007 | 1.007 | 1.008 | 887 | 897 | 1.011 |
| ordered-wide / move64-end | 13,405 | 13,533 | 1.010 | 1.004 | 1.003 | 383,897 | 384,300 | 1.001 |
| ordered-wide / append1 | 554 | 559 | 1.009 | 1.010 | 1.010 | 2,510 | 2,533 | 1.009 |
| ordered-wide / translate64 | 1,453 | 1,452 | 0.999 | 1.000 | 1.000 | 1,535 | 1,535 | 1.000 |
| ordered-wide / translate-half | 2,039 | 2,039 | 1.000 | 1.000 | 1.000 | 271,499 | 271,499 | 1.000 |
| ordered-wide / bulk-build | 98,778 | 99,141 | 1.004 | 1.002 | 1.002 | 86,430,627 | 86,427,447 | 1.000 |
| ordered-wide / build-append8 | 198,127 | 199,350 | 1.006 | 1.006 | 1.006 | 188,161,440 | 189,464,368 | 1.007 |
| ordered-wide / freeze-cold | 26,972 | 26,984 | 1.000 | 1.000 | 1.001 | 24,600,455 | 24,554,834 | 0.998 |
| ordered-wide / freeze-idle | 1,507 | 1,509 | 1.001 | 1.000 | 1.000 | 472,444 | 472,446 | 1.000 |
| ordered-wide / freeze-one | 1,805 | 1,807 | 1.001 | 1.000 | 1.000 | 472,742 | 472,744 | 1.000 |
| ordered-wide / freeze-all | 6,446 | 6,448 | 1.000 | 1.000 | 1.000 | 5,003,994 | 5,003,995 | 1.000 |
| ordered-wide / parent-last | 229 | 229 | 1.000 | 1.000 | 1.000 | 15,900 | 15,900 | 1.000 |
| ordered-wide / ancestors-last | 295 | 295 | 1.000 | 1.000 | 1.000 | 15,965 | 15,965 | 1.000 |
| ordered-wide / subtree-root | 205 | 205 | 1.000 | 1.000 | 1.000 | 17,812 | 17,812 | 1.000 |
| ordered-wide / subtree-leaf | 114 | 114 | 1.000 | 1.000 | 1.000 | 153 | 153 | 1.000 |
| ordered-chains32 / parent-last | 88 | 88 | 1.000 | 1.000 | 1.000 | 161 | 161 | 1.000 |
| ordered-chains32 / ancestors-last | 475 | 475 | 1.000 | 1.000 | 1.000 | 16,841 | 16,841 | 1.000 |
| ordered-chains32 / subtree-root | 205 | 205 | 1.000 | 1.000 | 1.000 | 17,812 | 17,812 | 1.000 |
| ordered-chains32 / subtree-leaf | 114 | 114 | 1.000 | 1.000 | 1.000 | 153 | 153 | 1.000 |
| ordered-wide / set-ink-pos | 371 | 371 | 1.000 | 1.000 | 1.000 | 452 | 452 | 1.000 |
| ordered-wide / set-ink-handle | 337 | 337 | 1.000 | 1.000 | 1.000 | 337 | 337 | 1.000 |
| ordered-wide / pos-of-handle | 24 | 24 | 1.000 | 1.000 | 1.000 | 24 | 24 | 1.000 |
| ordered-wide / touch-handle | 117 | 117 | 1.000 | 1.000 | 1.000 | 117 | 117 | 1.000 |
| ordered-wide / set-inks-handles64 | 18,664 | 18,668 | 1.000 | 1.000 | 1.000 | 35,713 | 35,717 | 1.000 |
| ordered-wide / insert8-front+pos64 | 5,927 | 5,953 | 1.004 | 1.003 | 1.001 | 377,121 | 377,148 | 1.000 |
| ordered-wide / insert8-front+posall | 4,432 | 4,457 | 1.006 | 1.002 | 1.000 | 785,608 | 785,634 | 1.000 |

## Spatial

| shape / operation | 1e3 base | 1e3 new | 1e3 | 1e4 | 1e5 | 1e6 base | 1e6 new | 1e6 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ordered-wide / cull-1-band | 353 | 353 | 1.000 | 1.000 | 1.000 | 53,792 | 53,792 | 1.000 |
| ordered-wide / cull-1-dispersed | 40,471 | 40,471 | 1.000 | 1.000 | 1.000 | 16,639,050 | 16,639,050 | 1.000 |
| ordered-wide / cull-10-band | 801 | 801 | 1.000 | 1.000 | 1.000 | 530,372 | 530,372 | 1.000 |
| ordered-wide / cull-10-dispersed | 82,835 | 82,835 | 1.000 | 1.000 | 1.000 | 16,776,202 | 16,776,202 | 1.000 |
| ordered-wide / cull-100-band | 5,519 | 5,519 | 1.000 | 1.000 | 1.000 | 5,296,180 | 5,296,180 | 1.000 |
| ordered-wide / cull-100-dispersed | 5,519 | 5,519 | 1.000 | 1.000 | 1.000 | 5,296,180 | 5,296,180 | 1.000 |
| ordered-wide / hit-present | 303 | 303 | 1.000 | 1.000 | 1.000 | 949 | 949 | 1.000 |
| ordered-wide / hit-gap | 286 | 286 | 1.000 | 1.000 | 1.000 | 790 | 790 | 1.000 |
| interleaved-wide / cull-1-band | 472 | 472 | 1.000 | 1.000 | 1.000 | 278,556 | 278,555 | 1.000 |
| interleaved-wide / cull-1-dispersed | 47,596 | 47,596 | 1.000 | 1.000 | 1.000 | 45,050,977 | 45,050,979 | 1.000 |
| interleaved-wide / cull-10-band | 805 | 805 | 1.000 | 1.000 | 1.000 | 755,080 | 755,080 | 1.000 |
| interleaved-wide / cull-10-dispersed | 88,056 | 88,056 | 1.000 | 1.000 | 1.000 | 87,273,278 | 87,273,372 | 1.000 |
| interleaved-wide / cull-100-band | 5,519 | 5,519 | 1.000 | 1.000 | 1.000 | 5,296,180 | 5,296,180 | 1.000 |
| interleaved-wide / cull-100-dispersed | 5,519 | 5,519 | 1.000 | 1.000 | 1.000 | 5,296,181 | 5,296,180 | 1.000 |
| interleaved-wide / hit-present | 277 | 277 | 1.000 | 1.000 | 1.000 | 1,298 | 1,298 | 1.000 |
| interleaved-wide / hit-gap | 286 | 286 | 1.000 | 1.000 | 1.000 | 97,103 | 97,103 | 1.000 |

## Cells above 1.02x

None.

## Above 1.02x but inside the cell's own scatter (`~`)

The larger of the two tags' run-to-run spread and sample noise exceeds the excess: not a measured regression.

* fresh-core ordered-wide / insert64-end N=1,000,000: 93,564 -> 105,564 (1.128x; scatter 27%)
* fresh-core ordered-wide / move64-middle N=1,000,000: 460,996 -> 486,598 (1.056x; scatter 8%)

