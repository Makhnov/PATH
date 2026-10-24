#[derive(PartialOrd, PartialEq, Debug, Clone, Copy)]
pub struct Node {
    pub x: f64,
    pub y: f64,
}

impl Eq for Node {}

impl Node {
    pub fn new(x: f64, y: f64) -> Self {
        Node { x, y }
    }
}
