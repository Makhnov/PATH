pub mod astar;
pub mod bucket;
pub mod countdown;
pub mod dijkstra;

pub(crate) mod utils {
    pub(crate) mod node_queue;
    pub mod path;
    pub(crate) mod states;
}

pub use self::astar::astar;
pub use self::bucket::{
    BucketPolicy, BucketStateMode, astar_bucket, astar_bucket_coupled, astar_bucket_decoupled,
};
pub use self::countdown::{
    CountdownPolicy, CountdownStateMode, astar_countdown, astar_countdown_coupled,
    astar_countdown_decoupled,
};
pub use self::dijkstra::dijkstra;

pub use self::utils::path;
pub use crate::event::GpsEvent;
pub use crate::{event, inspection as bench};

#[path = "algo/window/registry.rs"]
pub mod registry;
#[cfg(algorithm_astar_window)]
pub mod window;
