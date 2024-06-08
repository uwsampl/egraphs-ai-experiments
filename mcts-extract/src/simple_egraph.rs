//! A simple egraph data-structure used for testing.
//!
//! This module does not implement congruence closure, or any other useful
//! egraph algorithms.

use crate::{Assignment, Egraph, EgraphTotalCost, Utility};

pub(crate) struct SimpleEgraph {
    pub nodes: Vec<Vec<usize>>,
    pub classes: Vec<Vec<usize>>,
    // We could let-bind this up top, but we only use it here.
    #[allow(clippy::type_complexity)]
    pub score_fn: Box<dyn Fn(&Assignment<SimpleEgraph>, &SimpleEgraph) -> Utility>,
}

impl Egraph for SimpleEgraph {
    type ClassId = usize;
    type NodeId = usize;

    fn children(&self, id: &Self::NodeId) -> impl Iterator<Item = &Self::ClassId> {
        self.nodes[*id].iter()
    }

    fn members(&self, id: &Self::ClassId) -> impl Iterator<Item = &Self::NodeId> {
        self.classes[*id].iter()
    }
}

impl EgraphTotalCost for SimpleEgraph {
    fn assignment_utility(&self, assignment: &Assignment<Self>) -> Utility {
        (self.score_fn)(assignment, self)
    }
}
