pub type NodeId = usize;

#[derive(Clone, Debug)]
pub struct Node<Instruction: Clone> {
    pub id: NodeId,
    pub front_edges: Vec<NodeId>,
    pub bytecode: Instruction,
}

#[derive(Clone, Debug)]
pub struct NodeGraph<Instruction: Clone> {
    pub nodes: Vec<Node<Instruction>>,
}
