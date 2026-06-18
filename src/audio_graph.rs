use crate::{
    effect::Effect,
    frame::Frame,
    instrument::Instrument,
    midi::{EventKind, ScheduledEvent},
};

pub enum AudioNode {
    Source(Box<dyn Instrument + Send>),
    Processor(Box<dyn Effect + Send>),
    Output,
}

pub struct Edge {
    from: usize,
    to: usize,
}

pub struct AudioGraph {
    pub nodes: Vec<AudioNode>,
    pub edges: Vec<Edge>,
    pub order: Vec<usize>,
    pub buffers: Vec<Frame>,
}

impl AudioGraph {
    pub fn new(nodes: Vec<AudioNode>, edges: Vec<(usize, usize)>) -> Result<Self, String> {
        let num_nodes = nodes.len();
        let edges: Vec<Edge> = edges
            .into_iter()
            .map(|(from, to)| Edge { from, to })
            .collect();

        match topological_sort(num_nodes, &edges) {
            Ok(order) => Ok(Self {
                nodes,
                edges,
                order,
                buffers: vec![Frame::SILENCE; num_nodes],
            }),
            Err(msg) => Err(msg),
        }
    }

    pub fn process_event(&mut self, event: &ScheduledEvent) {
        match &event.kind {
            EventKind::Note { targets, event } => {
                for node in &mut self.nodes {
                    if let AudioNode::Source(instrument) = node
                        && targets.contains(&instrument.id())
                    {
                        instrument.process_event(event);
                    }
                }
            }
            EventKind::SetParam {
                node_idx,
                param_idx,
                value,
            } => {
                if let Some(AudioNode::Processor(effect)) = self.nodes.get_mut(*node_idx) {
                    effect.set_param(*param_idx, *value);
                }
            }
        }
    }

    pub fn next_sample(&mut self, sample_rate: f32) -> Frame {
        for &node_idx in &self.order {
            let input: Frame = self
                .edges
                .iter()
                .filter(|e| e.to == node_idx)
                .map(|e| self.buffers[e.from])
                .sum();

            let output = match &mut self.nodes[node_idx] {
                AudioNode::Source(instrument) => instrument.next_sample(sample_rate),
                AudioNode::Processor(effect) => effect.process(input),
                AudioNode::Output => input,
            };

            self.buffers[node_idx] = output;
        }

        self.order
            .iter()
            .filter(|&&idx| matches!(self.nodes[idx], AudioNode::Output))
            .map(|&idx| self.buffers[idx])
            .sum()
    }
}

fn topological_sort(num_nodes: usize, edges: &[Edge]) -> Result<Vec<usize>, String> {
    let mut in_degree = vec![0usize; num_nodes];
    let mut adjacency: Vec<Vec<usize>> = vec![vec![]; num_nodes];

    for edge in edges {
        in_degree[edge.to] += 1;
        adjacency[edge.from].push(edge.to);
    }

    // Start with nodes that have no incoming edges
    let mut queue: Vec<usize> = (0..num_nodes).filter(|&i| in_degree[i] == 0).collect();

    let mut order = Vec::with_capacity(num_nodes);

    while let Some(node) = queue.pop() {
        order.push(node);
        for &neighbor in &adjacency[node] {
            in_degree[neighbor] -= 1;
            if in_degree[neighbor] == 0 {
                queue.push(neighbor);
            }
        }
    }

    if order.len() != num_nodes {
        return Err(format!(
            "Audio graph has a cycle! Processed {} of {} nodes.",
            order.len(),
            num_nodes
        ));
    }
    Ok(order)
}
