#!/usr/bin/env python3

import time
print(time.time_ns() - (time.timezone - time.daylight * 3600) * 1_000_000_000)
