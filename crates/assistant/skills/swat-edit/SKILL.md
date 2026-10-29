---
name: swat-edit
description: Change a SWAT+ model's parameters safely — one value or many by a rule (replace, add, multiply as SWAT-CUP's v__, a__, r__), checked against cal_parms.cal, reviewed, reverted and saved. Use for "set ESCO to", "increase CN2 by 10%", "change the soil AWC", calibration edits.
---

# Changing a SWAT+ model

The swat commands commands change the open project in memory; nothing is written until `swat.save`.

1. **Find it first** (`swat.find`, `swat.get`): know the file, the column, the records and the current
   values before changing anything. Tell the user what will change.
2. **Change:** `swat.set` with `file`, `column`, `value` and a `mode`:
   - `replace` (default, SWAT-CUP v__): the value itself;
   - `add` (a__): the value added;
   - `multiply` (r__): times (1 + value), so 0.1 is 10 % more and -0.2 is 20 % less.
   Narrow it with `match` or `where`; without them every record changes. A number keeps the decimals it
   had and the file keeps its columns.
3. **Ranges:** a value outside *cal_parms.cal*'s range is refused. Don't pass `"force": true` unless the
   user asked for that value knowing the range.
4. **Review:** `swat.diff` lists what changed since the project was opened (counts only for a
   confidential project). `swat.revert` undoes all of it, one file's, or one column's.
5. **Save:** `swat.save` writes the changed files over the folder it came from (the user is asked), or the
   whole project to another folder with `path`. Suggest saving to another folder when the user wants
   to keep the original, as for a calibration run.

After a change, say what changed in how many records, with the new range (values_now in the reply).
