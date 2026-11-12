# Partial round 6: native-exposure list and coarse table cells

Calling-thread instructions per operation: median [min, max] of three slopes, each from 8 and 32 operation/undo repeats. Oracle work excluded. R1 counts CPU frame submission/completion, not GPU instructions. Counts taken under concurrent load. A failed pixel cell is not a speed win.

## rows / fine / normal / default 

| rows | operation | R0 instructions | R1 instructions | exponent R0/R1 | ideal exponent | pixels / semantics |
|---:|---|---:|---:|---|---:|---|
| 10 | hover | 4,240 [4,240, 4,240] | 50,523 [47,210, 55,148] | 0.04 (within sampled bound) / -0.00 (within sampled bound) | 0 | pass |
| 100 | hover | 4,326 [4,326, 4,326] | 46,919 [46,533, 60,789] | 0.04 (within sampled bound) / -0.00 (within sampled bound) | 0 | pass |
| 100 | scroll | 9,390 [9,390, 9,390] | 59,917 [55,519, 62,716] | 0.42 (OVERFIT) / 0.12 (OVERFIT) | 0 | retained-fresh: 198048 differing bytes; first Some(1884161) |
| 1,000 | hover | 4,781 [4,781, 4,781] | 52,007 [50,024, 56,149] | 0.04 (within sampled bound) / -0.00 (within sampled bound) | 0 | pass |
| 1,000 | scroll | 17,782 [17,782, 17,782] | 66,401 [61,174, 69,285] | 0.42 (OVERFIT) / 0.12 (OVERFIT) | 0 | retained-fresh: 198048 differing bytes; first Some(1884161) |
| 5,000 | hover | 5,612 [5,612, 5,612] | 47,848 [46,488, 58,532] | 0.04 (within sampled bound) / -0.00 (within sampled bound) | 0 | pass |
| 5,000 | scroll | 50,052 [50,052, 50,052] | 97,661 [97,224, 99,957] | 0.42 (OVERFIT) / 0.12 (OVERFIT) | 0 | retained-fresh: 198048 differing bytes; first Some(1884161) |

## table / fine / normal / default 

| rows | operation | R0 instructions | R1 instructions | exponent R0/R1 | ideal exponent | pixels / semantics |
|---:|---|---:|---:|---|---:|---|
| 10 | select | 11,582 [11,400, 12,162] | 60,077 [39,773, 60,273] | 0.01 (within sampled bound) / 0.03 (within sampled bound) | 0 | pass |
| 10 | update10 | 20,998 [20,998, 21,031] | 70,732 [66,354, 74,818] | 0.50 (within sampled bound) / 0.33 (within sampled bound) | 1 | pass |
| 10 | swap | 29,540 [29,422, 29,541] | 82,040 [80,345, 85,627] | 0.25 (OVERFIT) / 0.14 (OVERFIT) | 0 | pass |
| 10 | remove | 59,949 [59,517, 60,581] | 112,015 [109,847, 114,038] | 0.15 (OVERFIT) / 0.10 (OVERFIT) | 0 | pass |
| 10 | insert | 59,494 [59,348, 59,712] | 112,721 [111,301, 113,907] | 0.15 (OVERFIT) / 0.09 (OVERFIT) | 0 | pass |
| 10 | replace | 367,696 [358,786, 369,017] | 417,796 [416,111, 430,058] | 0.53 (within sampled bound) / 0.51 (within sampled bound) | 1 | pass |
| 10 | append | 1,336,426 [1,304,820, 1,336,952] | 1,376,570 [1,364,245, 1,393,301] | -0.04 (within sampled bound) / -0.04 (within sampled bound) | 0 | pass |
| 10 | truncate | 1,330,308 [1,323,814, 1,339,841] | 1,366,303 [1,362,677, 1,371,126] | -0.04 (within sampled bound) / -0.04 (within sampled bound) | 0 | pass |
| 10 | clear | 218,126 [211,144, 222,131] | 268,300 [260,081, 269,759] | 0.50 (within sampled bound) / 0.47 (within sampled bound) | 1 | pass |
| 10 | replace-select | 198,861 [197,359, 204,390] | 243,578 [241,288, 245,639] | 0.52 (OVERFIT) / 0.48 (OVERFIT) | 0 | pass |
| 100 | select | 11,632 [11,632, 11,703] | 62,190 [57,909, 64,344] | 0.01 (within sampled bound) / 0.03 (within sampled bound) | 0 | pass |
| 100 | update10 | 79,461 [79,359, 79,799] | 129,125 [129,070, 134,444] | 0.50 (within sampled bound) / 0.33 (within sampled bound) | 1 | pass |
| 100 | swap | 54,031 [53,687, 55,400] | 111,061 [106,771, 112,902] | 0.25 (OVERFIT) / 0.14 (OVERFIT) | 0 | pass |
| 100 | remove | 83,519 [83,382, 84,110] | 142,762 [139,021, 149,087] | 0.15 (OVERFIT) / 0.10 (OVERFIT) | 0 | pass |
| 100 | insert | 63,987 [63,870, 66,296] | 118,112 [115,635, 122,923] | 0.15 (OVERFIT) / 0.09 (OVERFIT) | 0 | pass |
| 100 | replace | 1,066,703 [1,058,513, 1,067,492] | 1,112,348 [1,104,859, 1,129,715] | 0.53 (within sampled bound) / 0.51 (within sampled bound) | 1 | pass |
| 100 | append | 1,047,540 [1,047,083, 1,051,444] | 1,102,551 [1,097,571, 1,109,356] | -0.04 (within sampled bound) / -0.04 (within sampled bound) | 0 | pass |
| 100 | truncate | 1,047,248 [1,047,040, 1,047,395] | 1,099,900 [1,099,885, 1,104,194] | -0.04 (within sampled bound) / -0.04 (within sampled bound) | 0 | pass |
| 100 | clear | 569,105 [566,569, 571,519] | 614,752 [608,043, 620,900] | 0.50 (within sampled bound) / 0.47 (within sampled bound) | 1 | pass |
| 100 | replace-select | 547,540 [547,225, 547,659] | 607,719 [599,837, 610,280] | 0.52 (OVERFIT) / 0.48 (OVERFIT) | 0 | pass |
| 1,000 | select | 12,330 [12,327, 12,338] | 59,680 [57,626, 61,053] | 0.01 (within sampled bound) / 0.03 (within sampled bound) | 0 | pass |
| 1,000 | update10 | 167,555 [167,555, 167,558] | 216,797 [216,074, 226,171] | 0.50 (within sampled bound) / 0.33 (within sampled bound) | 1 | pass |
| 1,000 | swap | 73,436 [72,733, 73,514] | 129,442 [128,044, 135,105] | 0.25 (OVERFIT) / 0.14 (OVERFIT) | 0 | pass |
| 1,000 | remove | 102,272 [102,021, 102,788] | 154,005 [149,730, 159,186] | 0.15 (OVERFIT) / 0.10 (OVERFIT) | 0 | pass |
| 1,000 | insert | 83,295 [82,390, 83,388] | 131,983 [128,260, 132,384] | 0.15 (OVERFIT) / 0.09 (OVERFIT) | 0 | pass |
| 1,000 | replace | 2,881,136 [2,880,969, 2,888,547] | 2,921,426 [2,920,219, 2,935,965] | 0.53 (within sampled bound) / 0.51 (within sampled bound) | 1 | pass |
| 1,000 | append | 1,030,530 [1,030,437, 1,030,576] | 1,081,206 [1,079,028, 1,084,786] | -0.04 (within sampled bound) / -0.04 (within sampled bound) | 0 | pass |
| 1,000 | truncate | 1,029,533 [1,029,076, 1,029,562] | 1,081,594 [1,081,215, 1,085,970] | -0.04 (within sampled bound) / -0.04 (within sampled bound) | 0 | pass |
| 1,000 | clear | 1,480,030 [1,477,788, 1,480,052] | 1,520,970 [1,515,842, 1,532,972] | 0.50 (within sampled bound) / 0.47 (within sampled bound) | 1 | pass |
| 1,000 | replace-select | 1,463,449 [1,461,535, 1,466,741] | 1,505,911 [1,505,630, 1,509,041] | 0.52 (OVERFIT) / 0.48 (OVERFIT) | 0 | pass |
| 5,000 | select | 12,330 [12,330, 12,330] | 78,831 [57,413, 81,066] | 0.01 (within sampled bound) / 0.03 (within sampled bound) | 0 | pass |
| 5,000 | update10 | 561,957 [560,592, 562,244] | 612,757 [611,362, 632,378] | 0.50 (within sampled bound) / 0.33 (within sampled bound) | 1 | pass |
| 5,000 | swap | 157,773 [156,066, 158,802] | 208,360 [203,102, 224,489] | 0.25 (OVERFIT) / 0.14 (OVERFIT) | 0 | pass |
| 5,000 | remove | 163,774 [163,176, 166,214] | 217,265 [214,808, 218,406] | 0.15 (OVERFIT) / 0.10 (OVERFIT) | 0 | pass |
| 5,000 | insert | 162,846 [161,297, 165,107] | 216,321 [214,317, 217,014] | 0.15 (OVERFIT) / 0.09 (OVERFIT) | 0 | pass |
| 5,000 | replace | 10,959,652 [10,956,186, 10,984,003] | 11,012,809 [11,008,095, 11,030,593] | 0.53 (within sampled bound) / 0.51 (within sampled bound) | 1 | pass |
| 5,000 | append | 1,032,388 [1,030,430, 1,034,141] | 1,071,826 [1,071,176, 1,085,914] | -0.04 (within sampled bound) / -0.04 (within sampled bound) | 0 | pass |
| 5,000 | truncate | 1,036,542 [1,035,583, 1,037,520] | 1,077,146 [1,075,068, 1,084,004] | -0.04 (within sampled bound) / -0.04 (within sampled bound) | 0 | pass |
| 5,000 | clear | 5,529,082 [5,525,131, 5,547,556] | 5,567,143 [5,562,093, 5,579,982] | 0.50 (within sampled bound) / 0.47 (within sampled bound) | 1 | pass |
| 5,000 | replace-select | 5,505,996 [5,501,851, 5,520,874] | 5,580,618 [5,559,544, 5,582,688] | 0.52 (OVERFIT) / 0.48 (OVERFIT) | 0 | pass |

## table / fine / plain / default 

| rows | operation | R0 instructions | R1 instructions | exponent R0/R1 | ideal exponent | pixels / semantics |
|---:|---|---:|---:|---|---:|---|
| 10 | select | 12,132 [12,132, 12,350] | 64,663 [58,496, 65,694] | 0.01 (within sampled bound) / -0.01 (within sampled bound) | 0 | pass |
| 10 | update10 | 21,551 [21,334, 21,656] | 68,948 [66,383, 74,736] | 0.50 (within sampled bound) / 0.33 (within sampled bound) | 1 | pass |
| 10 | swap | 24,208 [24,088, 24,304] | 71,659 [64,587, 80,421] | 0.28 (OVERFIT) / 0.16 (OVERFIT) | 0 | pass |
| 10 | remove | 63,533 [63,531, 63,664] | 112,784 [110,115, 114,027] | 0.16 (OVERFIT) / 0.10 (within sampled bound) | 0 | pass |
| 10 | insert | 64,192 [63,846, 64,197] | 114,820 [108,495, 116,411] | 0.15 (OVERFIT) / 0.11 (OVERFIT) | 0 | pass |
| 10 | replace | 357,585 [354,842, 360,275] | 400,007 [397,197, 402,549] | 0.53 (within sampled bound) / 0.52 (within sampled bound) | 1 | pass |
| 10 | append | 1,321,334 [1,318,558, 1,321,514] | 1,380,234 [1,360,164, 1,381,057] | -0.04 (within sampled bound) / -0.03 (within sampled bound) | 0 | pass |
| 10 | truncate | 1,324,778 [1,321,041, 1,328,922] | 1,377,660 [1,355,510, 1,386,584] | -0.04 (within sampled bound) / -0.03 (within sampled bound) | 0 | pass |
| 10 | clear | 218,876 [212,643, 219,328] | 262,394 [257,579, 271,206] | 0.50 (within sampled bound) / 0.47 (within sampled bound) | 1 | pass |
| 10 | replace-select | 195,978 [194,255, 198,024] | 244,180 [242,802, 244,520] | 0.52 (OVERFIT) / 0.48 (OVERFIT) | 0 | pass |
| 100 | select | 12,248 [12,248, 12,248] | 65,186 [61,470, 76,484] | 0.01 (within sampled bound) / -0.01 (within sampled bound) | 0 | pass |
| 100 | update10 | 79,844 [79,788, 80,138] | 124,712 [121,906, 124,834] | 0.50 (within sampled bound) / 0.33 (within sampled bound) | 1 | pass |
| 100 | swap | 54,119 [53,903, 54,188] | 102,858 [100,624, 112,425] | 0.28 (OVERFIT) / 0.16 (OVERFIT) | 0 | pass |
| 100 | remove | 93,319 [93,097, 95,044] | 146,882 [134,584, 147,754] | 0.16 (OVERFIT) / 0.10 (within sampled bound) | 0 | pass |
| 100 | insert | 80,846 [80,383, 81,006] | 134,819 [129,232, 139,420] | 0.15 (OVERFIT) / 0.11 (OVERFIT) | 0 | pass |
| 100 | replace | 1,037,817 [1,031,977, 1,040,974] | 1,093,720 [1,092,396, 1,102,565] | 0.53 (within sampled bound) / 0.52 (within sampled bound) | 1 | pass |
| 100 | append | 1,063,647 [1,062,017, 1,073,064] | 1,115,945 [1,111,583, 1,117,412] | -0.04 (within sampled bound) / -0.03 (within sampled bound) | 0 | pass |
| 100 | truncate | 1,057,524 [1,054,005, 1,058,034] | 1,110,993 [1,107,628, 1,117,243] | -0.04 (within sampled bound) / -0.03 (within sampled bound) | 0 | pass |
| 100 | clear | 564,069 [563,490, 564,357] | 627,362 [624,117, 632,008] | 0.50 (within sampled bound) / 0.47 (within sampled bound) | 1 | pass |
| 100 | replace-select | 537,398 [534,546, 537,424] | 590,643 [587,772, 604,317] | 0.52 (OVERFIT) / 0.48 (OVERFIT) | 0 | pass |
| 1,000 | select | 12,946 [12,946, 12,954] | 60,443 [60,243, 61,430] | 0.01 (within sampled bound) / -0.01 (within sampled bound) | 0 | pass |
| 1,000 | update10 | 167,832 [167,721, 167,942] | 218,157 [214,036, 220,114] | 0.50 (within sampled bound) / 0.33 (within sampled bound) | 1 | pass |
| 1,000 | swap | 72,385 [72,261, 73,421] | 121,706 [115,995, 122,494] | 0.28 (OVERFIT) / 0.16 (OVERFIT) | 0 | pass |
| 1,000 | remove | 110,722 [110,606, 110,832] | 167,390 [156,013, 168,745] | 0.16 (OVERFIT) / 0.10 (within sampled bound) | 0 | pass |
| 1,000 | insert | 98,827 [98,790, 101,748] | 155,861 [149,266, 157,975] | 0.15 (OVERFIT) / 0.11 (OVERFIT) | 0 | pass |
| 1,000 | replace | 2,858,284 [2,857,502, 2,864,375] | 2,912,547 [2,910,050, 2,914,656] | 0.53 (within sampled bound) / 0.52 (within sampled bound) | 1 | pass |
| 1,000 | append | 1,048,031 [1,047,218, 1,050,785] | 1,097,868 [1,088,821, 1,100,824] | -0.04 (within sampled bound) / -0.03 (within sampled bound) | 0 | pass |
| 1,000 | truncate | 1,046,617 [1,046,355, 1,047,620] | 1,099,478 [1,098,786, 1,100,846] | -0.04 (within sampled bound) / -0.03 (within sampled bound) | 0 | pass |
| 1,000 | clear | 1,481,461 [1,479,604, 1,486,976] | 1,535,460 [1,535,403, 1,539,562] | 0.50 (within sampled bound) / 0.47 (within sampled bound) | 1 | pass |
| 1,000 | replace-select | 1,451,403 [1,444,439, 1,451,953] | 1,493,924 [1,485,353, 1,510,915] | 0.52 (OVERFIT) / 0.48 (OVERFIT) | 0 | pass |
| 5,000 | select | 12,946 [12,946, 12,946] | 60,101 [58,625, 62,731] | 0.01 (within sampled bound) / -0.01 (within sampled bound) | 0 | pass |
| 5,000 | update10 | 561,524 [561,159, 564,005] | 613,184 [610,503, 618,938] | 0.50 (within sampled bound) / 0.33 (within sampled bound) | 1 | pass |
| 5,000 | swap | 155,836 [153,568, 156,625] | 212,390 [187,887, 223,664] | 0.28 (OVERFIT) / 0.16 (OVERFIT) | 0 | pass |
| 5,000 | remove | 181,101 [179,539, 185,192] | 215,804 [213,538, 238,143] | 0.16 (OVERFIT) / 0.10 (within sampled bound) | 0 | pass |
| 5,000 | insert | 180,366 [179,504, 184,872] | 232,896 [228,969, 246,252] | 0.15 (OVERFIT) / 0.11 (OVERFIT) | 0 | pass |
| 5,000 | replace | 10,951,302 [10,937,601, 10,979,217] | 11,019,390 [11,003,645, 11,029,299] | 0.53 (within sampled bound) / 0.52 (within sampled bound) | 1 | pass |
| 5,000 | append | 1,047,895 [1,047,444, 1,049,665] | 1,102,133 [1,094,916, 1,105,804] | -0.04 (within sampled bound) / -0.03 (within sampled bound) | 0 | pass |
| 5,000 | truncate | 1,049,374 [1,043,319, 1,056,004] | 1,100,443 [1,100,228, 1,105,874] | -0.04 (within sampled bound) / -0.03 (within sampled bound) | 0 | pass |
| 5,000 | clear | 5,530,917 [5,524,449, 5,542,017] | 5,581,573 [5,579,805, 5,588,129] | 0.50 (within sampled bound) / 0.47 (within sampled bound) | 1 | pass |
| 5,000 | replace-select | 5,493,752 [5,488,251, 5,496,399] | 5,565,286 [5,545,633, 5,587,499] | 0.52 (OVERFIT) / 0.48 (OVERFIT) | 0 | pass |

## table / coarse / normal / default 

| rows | operation | R0 instructions | R1 instructions | exponent R0/R1 | ideal exponent | pixels / semantics |
|---:|---|---:|---:|---|---:|---|
| 10 | select | 8,724 [8,724, 8,724] | 9,049 [9,012, 9,090] | 0.02 (within sampled bound) / 0.02 (within sampled bound) | 0 | retained-fresh: 79242 differing bytes; first Some(204800) |
| 10 | update10 | 5,068 [5,068, 5,280] | 5,212 [5,212, 5,413] | 0.71 (within sampled bound) / 0.71 (within sampled bound) | 1 | retained-fresh: 198 differing bytes; first Some(251320) |
| 10 | swap | 4,302 [4,302, 4,302] | 4,451 [4,423, 4,453] | 0.02 (within sampled bound) / 0.02 (within sampled bound) | 0 | retained-fresh: 7524 differing bytes; first Some(414880) |
| 10 | remove | 20,467 [20,465, 20,467] | 65,885 [64,992, 69,870] | 0.13 (OVERFIT) / 0.06 (within sampled bound) | 0 | retained-fresh: 34020 differing bytes; first Some(578612) |
| 10 | insert | 20,456 [20,455, 20,457] | 71,061 [69,369, 77,954] | 0.13 (OVERFIT) / 0.03 (OVERFIT) | 0 | retained-fresh: 37973 differing bytes; first Some(578612) |
| 10 | replace | 22,263 [22,155, 22,296] | 22,548 [22,109, 22,836] | 0.96 (within sampled bound) / 0.96 (within sampled bound) | 1 | retained-fresh: 36355 differing bytes; first Some(250932) |
| 10 | append | 871,928 [868,194, 872,492] | 920,049 [914,946, 922,506] | -0.00 (within sampled bound) / -0.00 (within sampled bound) | 0 | retained-fresh: 88362 differing bytes; first Some(1889332) |
| 10 | truncate | 873,199 [864,604, 882,252] | 923,898 [923,817, 924,194] | -0.00 (within sampled bound) / -0.00 (within sampled bound) | 0 | retained-fresh: 88730 differing bytes; first Some(1889332) |
| 10 | clear | 28,545 [28,542, 28,593] | 74,054 [73,684, 79,951] | 0.81 (within sampled bound) / 0.66 (within sampled bound) | 1 | retained-fresh: 66147 differing bytes; first Some(251152) |
| 10 | replace-select | 19,844 [19,792, 20,704] | 20,232 [20,223, 20,258] | 0.87 (OVERFIT) / 0.87 (OVERFIT) | 0 | assertion `left == right` failed |
| 100 | select | 8,786 [8,786, 8,786] | 8,872 [8,846, 9,435] | 0.02 (within sampled bound) / 0.02 (within sampled bound) | 0 | retained-fresh: 79242 differing bytes; first Some(204800) |
| 100 | update10 | 12,458 [12,449, 12,458] | 12,503 [12,228, 12,604] | 0.71 (within sampled bound) / 0.71 (within sampled bound) | 1 | retained-fresh: 594 differing bytes; first Some(251320) |
| 100 | swap | 4,346 [4,346, 4,346] | 4,529 [4,497, 4,533] | 0.02 (within sampled bound) / 0.02 (within sampled bound) | 0 | retained-fresh: 4002 differing bytes; first Some(414772) |
| 100 | remove | 21,552 [21,552, 21,552] | 76,517 [71,950, 80,735] | 0.13 (OVERFIT) / 0.06 (within sampled bound) | 0 | retained-fresh: 82957 differing bytes; first Some(578612) |
| 100 | insert | 9,861 [9,861, 9,861] | 65,214 [59,265, 66,311] | 0.13 (OVERFIT) / 0.03 (OVERFIT) | 0 | retained-fresh: 83498 differing bytes; first Some(578688) |
| 100 | replace | 174,248 [174,121, 174,336] | 174,772 [174,545, 175,020] | 0.96 (within sampled bound) / 0.96 (within sampled bound) | 1 | retained-fresh: 52310 differing bytes; first Some(251088) |
| 100 | append | 872,702 [872,251, 872,730] | 932,508 [922,215, 948,763] | -0.00 (within sampled bound) / -0.00 (within sampled bound) | 0 | pass |
| 100 | truncate | 872,542 [872,514, 872,578] | 928,624 [927,984, 929,272] | -0.00 (within sampled bound) / -0.00 (within sampled bound) | 0 | pass |
| 100 | clear | 104,977 [104,965, 105,623] | 156,295 [151,717, 158,625] | 0.81 (within sampled bound) / 0.66 (within sampled bound) | 1 | retained-fresh: 154509 differing bytes; first Some(251152) |
| 100 | replace-select | 95,949 [95,853, 96,014] | 96,470 [96,105, 100,352] | 0.87 (OVERFIT) / 0.87 (OVERFIT) | 0 | assertion `left == right` failed |
| 1,000 | select | 9,612 [9,594, 9,661] | 10,080 [9,632, 10,127] | 0.02 (within sampled bound) / 0.02 (within sampled bound) | 0 | retained-fresh: 79242 differing bytes; first Some(204800) |
| 1,000 | update10 | 86,228 [86,118, 86,228] | 86,330 [86,145, 88,768] | 0.71 (within sampled bound) / 0.71 (within sampled bound) | 1 | retained-fresh: 594 differing bytes; first Some(251320) |
| 1,000 | swap | 4,764 [4,729, 4,764] | 5,110 [4,496, 5,268] | 0.02 (within sampled bound) / 0.02 (within sampled bound) | 0 | retained-fresh: 3929 differing bytes; first Some(414772) |
| 1,000 | remove | 28,967 [28,922, 29,008] | 79,879 [78,699, 80,756] | 0.13 (OVERFIT) / 0.06 (within sampled bound) | 0 | retained-fresh: 82957 differing bytes; first Some(578612) |
| 1,000 | insert | 17,106 [17,104, 17,108] | 63,773 [60,572, 72,417] | 0.13 (OVERFIT) / 0.03 (OVERFIT) | 0 | retained-fresh: 83655 differing bytes; first Some(578688) |
| 1,000 | replace | 1,706,213 [1,704,038, 1,706,409] | 1,708,212 [1,705,723, 1,708,817] | 0.96 (within sampled bound) / 0.96 (within sampled bound) | 1 | retained-fresh: 47948 differing bytes; first Some(251088) |
| 1,000 | append | 864,867 [863,826, 869,730] | 904,838 [894,999, 906,957] | -0.00 (within sampled bound) / -0.00 (within sampled bound) | 0 | pass |
| 1,000 | truncate | 861,234 [861,233, 863,931] | 913,372 [912,116, 935,570] | -0.00 (within sampled bound) / -0.00 (within sampled bound) | 0 | pass |
| 1,000 | clear | 868,886 [868,034, 869,745] | 916,931 [915,023, 940,292] | 0.81 (within sampled bound) / 0.66 (within sampled bound) | 1 | retained-fresh: 154509 differing bytes; first Some(251152) |
| 1,000 | replace-select | 862,685 [862,586, 862,728] | 861,095 [860,913, 863,223] | 0.87 (OVERFIT) / 0.87 (OVERFIT) | 0 | assertion `left == right` failed |
| 5,000 | select | 9,638 [9,605, 9,638] | 9,703 [9,691, 9,897] | 0.02 (within sampled bound) / 0.02 (within sampled bound) | 0 | retained-fresh: 79242 differing bytes; first Some(204800) |
| 5,000 | update10 | 412,964 [412,193, 414,292] | 416,774 [416,369, 425,491] | 0.71 (within sampled bound) / 0.71 (within sampled bound) | 1 | retained-fresh: 594 differing bytes; first Some(251320) |
| 5,000 | swap | 4,794 [4,794, 4,794] | 5,058 [4,940, 5,383] | 0.02 (within sampled bound) / 0.02 (within sampled bound) | 0 | retained-fresh: 4042 differing bytes; first Some(414772) |
| 5,000 | remove | 47,225 [47,104, 48,678] | 101,628 [89,778, 104,794] | 0.13 (OVERFIT) / 0.06 (within sampled bound) | 0 | retained-fresh: 82957 differing bytes; first Some(578612) |
| 5,000 | insert | 47,015 [44,297, 47,138] | 93,292 [92,593, 95,115] | 0.13 (OVERFIT) / 0.03 (OVERFIT) | 0 | retained-fresh: 83261 differing bytes; first Some(578688) |
| 5,000 | replace | 8,524,164 [8,523,601, 8,551,346] | 8,533,856 [8,531,581, 8,538,670] | 0.96 (within sampled bound) / 0.96 (within sampled bound) | 1 | retained-fresh: 51865 differing bytes; first Some(251148) |
| 5,000 | append | 863,604 [862,410, 864,022] | 917,035 [912,966, 923,640] | -0.00 (within sampled bound) / -0.00 (within sampled bound) | 0 | pass |
| 5,000 | truncate | 870,029 [865,085, 872,099] | 914,774 [914,238, 917,122] | -0.00 (within sampled bound) / -0.00 (within sampled bound) | 0 | pass |
| 5,000 | clear | 4,297,809 [4,279,862, 4,316,732] | 4,350,571 [4,341,924, 4,354,060] | 0.81 (within sampled bound) / 0.66 (within sampled bound) | 1 | retained-fresh: 154509 differing bytes; first Some(251152) |
| 5,000 | replace-select | 4,286,371 [4,279,786, 4,289,376] | 4,294,241 [4,284,531, 4,294,885] | 0.87 (OVERFIT) / 0.87 (OVERFIT) | 0 | assertion `left == right` failed |

## table / coarse / plain / default 

| rows | operation | R0 instructions | R1 instructions | exponent R0/R1 | ideal exponent | pixels / semantics |
|---:|---|---:|---:|---|---:|---|
| 10 | select | 76,597 [76,597, 76,597] | 123,829 [122,850, 123,939] | 0.98 (OVERFIT) / 0.90 (OVERFIT) | 0 | pass |
| 10 | update10 | 53,095 [52,995, 53,369] | 100,304 [99,262, 103,241] | 0.98 (within sampled bound) / 0.88 (within sampled bound) | 1 | pass |
| 10 | swap | 70,664 [70,659, 70,664] | 114,500 [112,914, 122,182] | 0.88 (OVERFIT) / 0.81 (OVERFIT) | 0 | pass |
| 10 | remove | 217,723 [217,576, 221,249] | 268,557 [266,085, 268,575] | 1.01 (OVERFIT) / 0.98 (OVERFIT) | 0 | pass |
| 10 | insert | 239,234 [238,721, 239,483] | 291,631 [290,212, 292,091] | 1.00 (OVERFIT) / 0.95 (OVERFIT) | 0 | pass |
| 10 | replace | 222,846 [222,752, 225,823] | 272,158 [265,119, 281,883] | 0.99 (within sampled bound) / 0.95 (within sampled bound) | 1 | pass |
| 10 | append | 21,226,586 [21,038,908, 21,259,761] | 21,284,027 [21,146,598, 21,351,631] | 0.01 (within sampled bound) / 0.01 (within sampled bound) | 0 | pass |
| 10 | truncate | failed 101 | failed 101 | unsupported range / unsupported range | 0 | pass |
| 10 | clear | 250,766 [243,502, 254,953] | 301,601 [296,786, 306,359] | 0.92 (within sampled bound) / 0.84 (within sampled bound) | 1 | pass |
| 10 | replace-select | 190,535 [190,202, 190,666] | 241,936 [240,827, 244,646] | 0.98 (OVERFIT) / 0.93 (OVERFIT) | 0 | assertion `left == right` failed |
| 100 | select | 667,431 [667,381, 667,452] | 712,512 [712,165, 719,871] | 0.98 (OVERFIT) / 0.90 (OVERFIT) | 0 | pass |
| 100 | update10 | 471,495 [471,363, 472,524] | 524,983 [511,346, 535,412] | 0.98 (within sampled bound) / 0.88 (within sampled bound) | 1 | pass |
| 100 | swap | 366,536 [365,804, 374,092] | 413,290 [412,401, 421,744] | 0.88 (OVERFIT) / 0.81 (OVERFIT) | 0 | pass |
| 100 | remove | 2,329,003 [2,324,261, 2,333,601] | 2,395,697 [2,369,698, 2,398,099] | 1.01 (OVERFIT) / 0.98 (OVERFIT) | 0 | pass |
| 100 | insert | 2,333,526 [2,332,236, 2,336,580] | 2,384,905 [2,377,751, 2,406,190] | 1.00 (OVERFIT) / 0.95 (OVERFIT) | 0 | pass |
| 100 | replace | 2,081,794 [2,081,487, 2,082,119] | 2,143,369 [2,136,047, 2,154,138] | 0.99 (within sampled bound) / 0.95 (within sampled bound) | 1 | pass |
| 100 | append | 21,792,820 [21,635,049, 21,934,675] | 21,989,276 [21,842,402, 22,055,314] | 0.01 (within sampled bound) / 0.01 (within sampled bound) | 0 | pass |
| 100 | truncate | failed 101 | failed 101 | unsupported range / unsupported range | 0 | pass |
| 100 | clear | 2,073,249 [2,072,769, 2,076,905] | 2,090,262 [2,082,543, 2,095,435] | 0.92 (within sampled bound) / 0.84 (within sampled bound) | 1 | pass |
| 100 | replace-select | 1,717,245 [1,713,612, 1,718,360] | 1,770,289 [1,762,869, 1,771,186] | 0.98 (OVERFIT) / 0.93 (OVERFIT) | 0 | assertion `left == right` failed |
| 1,000 | select | 6,567,603 [6,566,205, 6,567,713] | 6,655,788 [6,635,347, 6,659,623] | 0.98 (OVERFIT) / 0.90 (OVERFIT) | 0 | pass |
| 1,000 | update10 | 4,662,665 [4,662,487, 4,669,592] | 4,712,265 [4,706,057, 4,719,843] | 0.98 (within sampled bound) / 0.88 (within sampled bound) | 1 | pass |
| 1,000 | swap | 3,311,480 [3,310,914, 3,316,708] | 3,388,153 [3,385,380, 3,415,718] | 0.88 (OVERFIT) / 0.81 (OVERFIT) | 0 | pass |
| 1,000 | remove | 23,482,462 [23,405,464, 23,511,044] | 23,552,672 [23,503,916, 23,583,802] | 1.01 (OVERFIT) / 0.98 (OVERFIT) | 0 | pass |
| 1,000 | insert | 23,459,923 [23,450,220, 23,481,522] | 23,595,873 [23,557,346, 23,610,260] | 1.00 (OVERFIT) / 0.95 (OVERFIT) | 0 | pass |
| 1,000 | replace | 21,040,084 [21,022,401, 21,059,106] | 21,146,697 [21,079,092, 21,148,886] | 0.99 (within sampled bound) / 0.95 (within sampled bound) | 1 | pass |
| 1,000 | append | failed 101 | failed 101 | 0.01 (within sampled bound) / 0.01 (within sampled bound) | 0 | pass |
| 1,000 | truncate | failed 101 | failed 101 | unsupported range / unsupported range | 0 | pass |
| 1,000 | clear | failed 101 | failed 101 | 0.92 (within sampled bound) / 0.84 (within sampled bound) | 1 | pass |
| 1,000 | replace-select | 17,083,654 [17,059,991, 17,085,714] | 17,203,777 [17,194,771, 17,230,527] | 0.98 (OVERFIT) / 0.93 (OVERFIT) | 0 | assertion `left == right` failed |
| 5,000 | select | 32,874,375 [32,831,183, 32,900,394] | 32,956,498 [32,945,952, 33,013,733] | 0.98 (OVERFIT) / 0.90 (OVERFIT) | 0 | pass |
| 5,000 | update10 | 23,336,384 [23,283,582, 23,370,386] | 23,389,770 [23,389,258, 23,512,323] | 0.98 (within sampled bound) / 0.88 (within sampled bound) | 1 | pass |
| 5,000 | swap | 16,465,534 [16,458,292, 16,500,082] | 16,582,656 [16,539,507, 16,632,674] | 0.88 (OVERFIT) / 0.81 (OVERFIT) | 0 | pass |
| 5,000 | remove | 116,538,216 [116,501,320, 116,686,096] | 116,649,646 [116,581,858, 116,848,827] | 1.01 (OVERFIT) / 0.98 (OVERFIT) | 0 | pass |

Gate failures remain recorded even when instruction samples succeed. Missing slopes mean the process failed or timed out; they are not zeros. See the main report for mechanisms and scope.
