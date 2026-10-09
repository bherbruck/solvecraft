# Parameters and expressions

Every dimension and feature input is a **model parameter** (`d1`, `d2`, …). **User parameters**
are ones you add yourself, with a unit, an expression and an optional comment. Change them in
**Change Parameters** (`parameters.change`). The timeline rebuilds from the first feature that
uses them.

Expressions are unit-aware: `2 * width + 5 mm`, `angle / 2`, `sqrt(area)`, `1 in + 3 mm`, using
`+ - * / ^`, parentheses, `sin cos tan asin acos atan atan2 sqrt abs min max floor ceil round mod
trunc` and unit conversion. A length where an angle is expected (or the other way round) is an
error, not a silent conversion. Cycles are reported with their path (`a → b → a`), renaming a
parameter updates every expression, dimension and feature input that uses it, and a parameter
in use can't be deleted (the error lists its users).

The design's units (Document Settings in the browser: mm, cm, m, in or ft) set how a bare number
typed into a feature is read. In an inch design `2` means 2 in. Changing the units never moves
existing geometry.

Parameters export to and import from CSV or JSON (`parameters.export`, `parameters.import`).
**Configurations** (`config.*`) keep a table of rows, each with its own parameter values and
suppressed features, switched with `config.activate`.
