#![no_std]

#[cfg(not(any(feature = "perf", feature = "inspect")))]
compile_error!("Activer exactement une feature de benchmark: perf | inspect");

#[cfg(all(feature = "perf", feature = "inspect"))]
compile_error!("Les features perf et inspect sont mutuellement exclusives");

#[cfg(any(feature = "host", feature = "inspect"))]
extern crate std;

pub mod result;
pub use result::{Error, Result};

pub mod algo;
pub mod format;
pub mod graph;
pub mod inspection {
    pub(crate) mod astar;
    pub mod bench;
    pub(crate) mod bucket;
    pub(crate) mod countdown;
    pub(crate) mod dijkstra;
    pub mod event;
    pub(crate) mod window;
    pub use bench::*;
}

pub use algo::path;
pub use inspection::event;
