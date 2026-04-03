#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WindowPoolPlacementSpec {
    pub(crate) row: i64,
    pub(crate) col: i64,
    pub(crate) width: u16,
    pub(crate) zindex: u32,
}

impl WindowPoolPlacementSpec {
    pub(crate) const fn builder() -> WindowPoolPlacementSpecBuilder {
        WindowPoolPlacementSpecBuilder::new()
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct WindowPoolPlacementSpecBuilder {
    row: i64,
    col: i64,
    width: u16,
    zindex: u32,
}

impl WindowPoolPlacementSpecBuilder {
    pub(crate) const fn new() -> Self {
        Self {
            row: 0,
            col: 0,
            width: 1,
            zindex: 50,
        }
    }

    pub(crate) const fn origin(mut self, row: i64, col: i64) -> Self {
        self.row = row;
        self.col = col;
        self
    }

    pub(crate) const fn width(mut self, width: u16) -> Self {
        self.width = width;
        self
    }

    pub(crate) const fn zindex(mut self, zindex: u32) -> Self {
        self.zindex = zindex;
        self
    }

    pub(crate) const fn build(self) -> WindowPoolPlacementSpec {
        WindowPoolPlacementSpec {
            row: self.row,
            col: self.col,
            width: self.width,
            zindex: self.zindex,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WindowPoolWindowSpec {
    pub(crate) window_id: i32,
    pub(crate) buffer_id: i32,
    pub(crate) last_used_epoch: u64,
    pub(crate) visible: bool,
    pub(crate) placement: WindowPoolPlacementSpec,
}

impl WindowPoolWindowSpec {
    pub(crate) const fn builder() -> WindowPoolWindowSpecBuilder {
        WindowPoolWindowSpecBuilder::new()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WindowPoolWindowSpecBuilder {
    window_id: i32,
    buffer_id: i32,
    last_used_epoch: u64,
    visible: bool,
    placement: WindowPoolPlacementSpec,
}

impl Default for WindowPoolWindowSpecBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowPoolWindowSpecBuilder {
    pub(crate) const fn new() -> Self {
        Self {
            window_id: 0,
            buffer_id: 0,
            last_used_epoch: 0,
            visible: false,
            placement: WindowPoolPlacementSpec::builder().build(),
        }
    }

    pub(crate) const fn ids(mut self, window_id: i32, buffer_id: i32) -> Self {
        self.window_id = window_id;
        self.buffer_id = buffer_id;
        self
    }

    pub(crate) const fn last_used_epoch(mut self, last_used_epoch: u64) -> Self {
        self.last_used_epoch = last_used_epoch;
        self
    }

    pub(crate) const fn visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    pub(crate) const fn placement(mut self, placement: WindowPoolPlacementSpec) -> Self {
        self.placement = placement;
        self
    }

    pub(crate) const fn build(self) -> WindowPoolWindowSpec {
        WindowPoolWindowSpec {
            window_id: self.window_id,
            buffer_id: self.buffer_id,
            last_used_epoch: self.last_used_epoch,
            visible: self.visible,
            placement: self.placement,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WindowPoolFixture {
    pub(crate) expected_demand: usize,
    pub(crate) windows: Vec<WindowPoolWindowSpec>,
}

impl WindowPoolFixture {
    pub(crate) fn new(expected_demand: usize, windows: Vec<WindowPoolWindowSpec>) -> Self {
        Self {
            expected_demand,
            windows,
        }
    }
}
