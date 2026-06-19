use crate::{
    effect::Effect,
    frame::Frame,
    instrument::Instrument,
    midi::{EventKind, ScheduledEvent},
};

/// Largest block the graph will process in a single `process_block` call.
/// Per-node scratch buffers are pre-allocated to this size so the audio
/// callback never allocates. It's a ceiling, not a processing contract: the
/// callback splits any longer request into `MAX_BLOCK`-sized chunks.
pub const MAX_BLOCK: usize = 4096;

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
    /// Per-node output scratch, each `MAX_BLOCK` frames long. A node writes its
    /// block here; downstream nodes read it via their incoming edges.
    buffers: Vec<Vec<Frame>>,
    /// Reusable mix scratch for summing a node's inputs, `MAX_BLOCK` long.
    mix_scratch: Vec<Frame>,
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
                buffers: vec![vec![Frame::SILENCE; MAX_BLOCK]; num_nodes],
                mix_scratch: vec![Frame::SILENCE; MAX_BLOCK],
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
            // A plugin node may be either a Source or a Processor, so route to
            // whichever it is.
            #[cfg(feature = "clap-host")]
            EventKind::SetPluginParam {
                node_idx,
                param_id,
                value,
            } => match self.nodes.get_mut(*node_idx) {
                Some(AudioNode::Processor(effect)) => effect.set_param_by_id(*param_id, *value),
                Some(AudioNode::Source(instrument)) => {
                    instrument.set_param_by_id(*param_id, *value)
                }
                _ => {}
            },
        }
    }

    /// Process `out.len()` frames through the whole graph, writing the summed
    /// output of every `Output` node into `out`. `out.len()` must be
    /// `<= MAX_BLOCK` (callers chunk longer requests). No allocation occurs.
    pub fn process_block(&mut self, out: &mut [Frame], sample_rate: f32) {
        let n = out.len();
        debug_assert!(n <= MAX_BLOCK, "block of {n} exceeds MAX_BLOCK {MAX_BLOCK}");

        for &node_idx in &self.order {
            // Sum this node's inputs into mix_scratch (immutable reads of the
            // upstream buffers, which are already filled — topo order guarantees
            // it). Sources have no inputs, so mix stays silent for them.
            let mix = &mut self.mix_scratch[..n];
            mix.fill(Frame::SILENCE);
            for edge in &self.edges {
                if edge.to == node_idx {
                    let src = &self.buffers[edge.from][..n];
                    for (m, s) in mix.iter_mut().zip(src) {
                        *m += *s;
                    }
                }
            }

            // Process into the node's own output buffer.
            let dst = &mut self.buffers[node_idx][..n];
            match &mut self.nodes[node_idx] {
                AudioNode::Source(instrument) => instrument.render_block(dst, sample_rate),
                AudioNode::Processor(effect) => {
                    dst.copy_from_slice(&self.mix_scratch[..n]);
                    effect.process_block(dst);
                }
                AudioNode::Output => dst.copy_from_slice(&self.mix_scratch[..n]),
            }
        }

        // Sum every Output node's buffer into `out`.
        out.fill(Frame::SILENCE);
        for &idx in &self.order {
            if matches!(self.nodes[idx], AudioNode::Output) {
                for (o, s) in out.iter_mut().zip(&self.buffers[idx][..n]) {
                    *o += *s;
                }
            }
        }
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
