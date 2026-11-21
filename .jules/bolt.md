## 2024-05-24 - Screentime DB O(n^2) interval grouping
**Learning:** Found `get_daily_totals_in_range` has an O(D * N) time complexity because it loops over days and for each day it loops over all rows fetched from DB. With 365 days and 10000 rows this takes ~2.5s locally. We can optimize it by looping over rows once and determining the day boundaries on the fly or mapping it to the appropriate days, resulting in O(N) which takes ~0.07s.
**Action:** Replace the nested loop with a dictionary map by precomputing day bounds and traversing rows once.
