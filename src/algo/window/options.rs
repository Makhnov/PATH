#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowPolicy {
    SpatialPriority,
    SpatialTieBreak,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowOptions {
    pub delta: u32,
    pub max_batch: usize,
    pub policy: WindowPolicy,
}

impl WindowOptions {
    pub fn new(delta: u32, max_batch: usize) -> Result<Self, &'static str> {
        if max_batch == 0 {
            return Err("tile batch must contain at least one candidate");
        }
        Ok(Self {
            delta,
            max_batch,
            policy: WindowPolicy::SpatialPriority,
        })
    }

    pub fn with_policy(mut self, policy: WindowPolicy) -> Self {
        self.policy = policy;
        if policy != WindowPolicy::SpatialPriority {
            self.delta = 0;
        }
        self
    }
}

impl Default for WindowOptions {
    fn default() -> Self {
        Self::new(16, 32).expect("valid default")
    }
}
