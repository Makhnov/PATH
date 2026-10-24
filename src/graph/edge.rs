#[derive(Debug, Clone, Copy)]
pub struct Edge<N> {
    pub node: N,
    pub neighbor: N,
    pub cost: u32,
}

impl<N> Edge<N> {
    pub fn new(node: N, neighbor: N, cost: u32) -> Edge<N> {
        Edge {
            node,
            neighbor,
            cost,
        }
    }
}
