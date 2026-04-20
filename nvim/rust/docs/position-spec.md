# Smear Cursor Position Spec

This note captures the reducer-owned cursor-position contract after the
position-boundary refactor.

## Projected Cursor Truth

`ObservedCell::Exact` and `ObservedCell::Deferred` both carry projected
display-space `ScreenCell` values. `Deferred` means freshness is lower and an
exact refresh is still owed; it does not mean the cell is still in raw host
coordinates.

In practice that means the observation pipeline may use cached or deferred
projection during fast motion, followed by exact refresh when needed.

Probe policy may choose exact projection versus deferred-allowed reads, but it
may not change the coordinate space of the returned observation.

Stated another way: exact and deferred observed cells both retain projected
display-space cursor cells, and probe policy chooses freshness/cost only; it
does not switch between raw and projected coordinate systems.
