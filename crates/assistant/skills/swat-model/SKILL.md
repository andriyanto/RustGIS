---
name: swat-model
description: Open and look at a SWAT+ model (a TxtInOut folder) — what it simulates, which files and objects it has, what is missing, where a parameter is and what values it holds. Use for SWAT+ / SWATPlus / TxtInOut questions, "what's in my SWAT model", "where is CN2 / ESCO / AWC".
---

# A SWAT+ model

A SWAT+ project is a TxtInOut folder of text files, *file.cio* naming the others. RustGIS holds one
beside the map with the swat commands commands (`search_commands` "swat" lists them).

1. **Open it:** `swat.open` with the folder's `path` (it asks the user, since it reads files). If the
   user says the data is private or confidential, or it is a government agency's or a client's data,
   open it with `"confidential": true`: answers then carry counts and statistics, never a record's name
   or value. Only the user can lift that.
2. **Overview:** `swat.summary` gives the simulated period (*time.sim*), the number of each kind of object
   (*object.cnt*: HRUs, routing units, channels, aquifers, reservoirs) and missing files.
   `swat.check` lists missing input files, and values outside *cal_parms.cal*'s calibration ranges:
   those are the limits calibration may move a value within, so a value outside one isn't an error by
   itself (the SWAT+ Editor writes some defaults outside them). Mention them, don't call them faults.
3. **Finding a parameter:** `swat.find` with part of the name (`"cn"`, `"esco"`, `"awc"`, `"alpha"`). The
   same name can be in several files; say which file you mean. Common places:
   - *hydrology.hyd*: ESCO, EPCO, CN3_SWF, PERCO, LAT_TTIME, CAN_MAX…
   - *cntable.lum*: curve numbers by land use and soil group (CN2 is calibrated through it, or through
     *calibration.cal*).
   - *soils.sol*: soil layers (AWC, K, BD, depth), one soil owning several layers.
   - *aquifer.aqu*: ALPHA_BF, GW_DELAY (GW_FLO, DEP_BOT…), REVAP.
   - *hyd-sed-lte.cha*: SWAT-DEG channel width, depth, slope, Manning's n.
   - *hru-data.hru*: each HRU's topography, hydrology, soil and land use pointers.
4. **Reading:** `swat.get` with `file` and `column`, narrowed by `match` (record names, `*` and `?`) or
   `where` (`{"column": "lu_mgt", "match": "agrl*"}`). It lists up to 500 records and gives the count,
   range and mean of all of them.

Report values as read, with the file and column. Don't guess a parameter's meaning from its name when
the SWAT+ input documentation would say; say what you assumed.
