# Position Spec

## Projected Cursor Contract

`ObservedCell::Exact` and `ObservedCell::Deferred` both carry projected display-space `ScreenCell` values. `Deferred` means freshness is lower and an exact refresh is still owed; it does not mean the cell is still in raw host coordinates.

The observation path permits cached or deferred projection during fast motion, followed by exact refresh when needed.

Probe policy may choose exact projection versus deferred-allowed reads, but it may not change the coordinate space of the returned observation.

The reducer contract is that exact and deferred observed cells both retain projected display-space cursor cells.

Put differently: probe policy chooses freshness/cost only; it does not switch between raw and projected coordinate systems.
