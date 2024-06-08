//! Basic monte-carlo tree search for e-graph extraction.
use std::cmp;

use fxhash::FxHashMap;

use crate::{extraction_state::ExtractionState, Assignment, Egraph, MctsConfig, Utility};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
struct TreeNodeId(u32);

impl TreeNodeId {
    fn index(self) -> usize {
        self.0 as usize
    }
}

struct TreeNode<N, C> {
    /// The class to which this TreeNode corresponds.
    ///
    /// NB: We do not strictly need this, but it is useful for debugging. Once
    /// we are more confident in the correctness of the code, we can remove this
    /// field.
    class: C,
    n_visits: u32,
    total_utility: Utility,
    // NB: look at replacing this with a SmallVec of kv pairs; the arity for
    // most languages / rulesets will be bounded and small.
    state: FxHashMap<N, TreeNodeId>,
}

const fn cast_util(n: u32) -> Utility {
    // SAFETY: We are always converting from a u32, which will always round to a
    // non-NaN value.
    unsafe { Utility::new_unchecked(n as f32) }
}

/// Compute the score of the current node given the total ruounds run under
/// the parent node, and a constant `c` for weighting exploration.
fn uct_score(
    child_rounds: u32,
    child_avg_utility: Utility,
    total_rounds: u32,
    c: Utility,
) -> Utility {
    let n_visits = cmp::max(child_rounds, 1) as f32;
    let exploration_term =
        c * Utility::new(((total_rounds as f32).ln() / n_visits).sqrt()).unwrap();
    child_avg_utility + exploration_term
}

pub(crate) struct SearchTree<E: Egraph> {
    root_class: E::ClassId,
    root_tree_node: TreeNodeId,
    nodes: Vec<TreeNode<E::NodeId, E::ClassId>>,
}

impl<E: Egraph> SearchTree<E> {
    pub(crate) fn new(root_class: E::ClassId) -> Self {
        let root_tree_node = TreeNodeId(0);
        Self {
            root_class: root_class.clone(),
            root_tree_node,
            nodes: vec![TreeNode {
                class: root_class,
                n_visits: Default::default(),
                total_utility: Default::default(),
                state: Default::default(),
            }],
        }
    }

    pub(crate) fn start_round<F>(
        &mut self,
        estimate_util: F,
        exploration_term: Utility,
    ) -> SearchState<E, F> {
        let root_class = self.root_class.clone();
        let start_node = self.root_tree_node;
        SearchState {
            tree: self,
            assignment: ExtractionState::new(root_class),
            start_node,
            path: Default::default(),
            estimate_util,
            exploration_term,
        }
    }

    fn fresh_node(&mut self, class: E::ClassId) -> TreeNodeId {
        let res = TreeNodeId(u32::try_from(self.nodes.len()).unwrap());
        self.nodes.push(TreeNode {
            class,
            n_visits: Default::default(),
            total_utility: Default::default(),
            state: Default::default(),
        });
        res
    }
}

pub(crate) struct SearchState<'a, E: Egraph, F> {
    tree: &'a mut SearchTree<E>,
    assignment: ExtractionState<E>,
    start_node: TreeNodeId,
    path: Vec<TreeNodeId>,
    estimate_util: F,
    exploration_term: Utility,
}

impl<E: Egraph, F: FnMut(&mut ExtractionState<E>, &E) -> Utility> SearchState<'_, E, F> {
    /// Pick the next node in the assignment based on the data in the current playouts.
    ///
    /// Returns false if the current node is a leaf.
    fn pick_node(&mut self, egraph: &E) -> Option<bool> {
        // Look at the current start node and pick the child with the highest
        // number of visits.
        let Some(handle) = self.assignment.start_next_assign() else {
            return Some(false);
        };
        let cur_node = &self.tree.nodes[self.start_node.index()];
        let (next_enode, next_tree_node) = cur_node.state.iter().max_by_key(|(_, &child)| {
            let child_node = &self.tree.nodes[child.index()];
            child_node.n_visits
        })?;

        assert!(&self.tree.nodes[next_tree_node.index()].class == handle.class());
        handle.assign(next_enode.clone(), egraph);
        self.start_node = *next_tree_node;
        self.assignment.push_snapshot();
        Some(true)
    }

    pub(crate) fn assign(&mut self, options: &MctsConfig, egraph: &E) -> Option<Assignment<E>> {
        loop {
            for _ in 0..options.playouts_per_round {
                self.run_playout(egraph);
            }
            if !self.pick_node(egraph)? {
                break;
            }
        }
        Some(self.assignment.complete_assignment()?.clone())
    }

    /// The core of the MCTS loop: iterate through the tree, simulate a run,
    /// then backpropagate information up the tree.
    fn run_playout(&mut self, egraph: &E) {
        // NB: we use the `path` vector to store nodes we have visited along the
        // way instead of recursion. Terms can have a lot of nodes and we don't
        // want to blow the stack.
        let mut cur_node_id = self.start_node;
        self.path.push(cur_node_id);
        let mut leaf_util = None;
        while let Some(handle) = self.assignment.start_next_assign() {
            let cur_node = &self.tree.nodes[cur_node_id.index()];
            if cur_node.n_visits == 0 {
                let cost = (self.estimate_util)(&mut self.assignment, egraph);
                leaf_util = Some(cost);
                break;
            } else {
                let total_rounds = cur_node.n_visits;
                let next_state = egraph
                    .members(handle.class())
                    .map(|node| {
                        if let Some(child) = cur_node.state.get(node) {
                            let child_node = &self.tree.nodes[child.index()];
                            (
                                uct_score(
                                    child_node.n_visits,
                                    child_node.total_utility
                                        / cast_util(cmp::max(child_node.n_visits, 1)),
                                    total_rounds,
                                    self.exploration_term,
                                ),
                                Some(*child),
                                node,
                            )
                        } else {
                            (
                                uct_score(0, cast_util(0), total_rounds, self.exploration_term),
                                None,
                                node,
                            )
                        }
                    })
                    .max_by_key(|(x, _, _)| *x);
                let Some((_, child_tree_node, enode_id)) = next_state else {
                    // There aren't any nodes in this e-class, so we can't extract.
                    leaf_util = Some(Utility::default());
                    break;
                };
                let child = if let Some(child) = child_tree_node {
                    child
                } else {
                    let new = self.tree.fresh_node(handle.class().clone());
                    self.tree.nodes[cur_node_id.index()]
                        .state
                        .insert(enode_id.clone(), new);
                    new
                };
                self.path.push(child);
                cur_node_id = child;
                handle.assign(enode_id.clone(), egraph);
            }
        }
        let util = if let Some(util) = leaf_util {
            util
        } else {
            // We got a complete assignment.
            (self.estimate_util)(&mut self.assignment, egraph)
        };
        for node_id in self.path.drain(..).rev() {
            let node = &mut self.tree.nodes[node_id.index()];
            node.n_visits = node.n_visits.saturating_add(1);
            node.total_utility += util;
        }
        self.assignment.reset(egraph);
    }
}
