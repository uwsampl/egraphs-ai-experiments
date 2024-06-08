//! Data-structures for incrementally maintaining a partial assignment of
//! e-nodes to e-classes.
//!
//! Extraction in this crate proceeds top-down, starting at the root e-class and
//! then iteratively assigning nodes to classes that have been discovered, and
//! then discovering more classes to assign based on the nodes chosen in the
//! past. The advantage of this approach is that each attempt at extracting a
//! term only has to consider the classes and nodes needed for that term,
//! whereas bottom-up approaches need to process the entire e-graph reachable
//! from the root class. Top-down extraction has a number of downsides, however:
//!
//!  * Bottom-up extraction can prune cycles more easily: one only starts
//!  processing an e-node once there is a valid extraction for all of its
//!  children. Top-down extraction can't do this, and as a result it can
//!  traverse unfruitful paths (though ideally MCTS will prune these for us).
//!
//!  * Top-down extraction is probably worse for greedy extraction where each
//!  e-node has known cost. For bottom-up, we can choose between the cost of
//!  entire subtrees of a term, whereas top-down can only look at the node cost
//!  on its own. This crate is focused on cases where we are only interested in
//!  computing whole-term costs anyways, so this downside won't concern us here.
//!
//!  * Handling cycles "lazily" ends up being much more complicated.
//!
//! # How it works
//!
//! The main `ExtractionState` data-structure maintains several layers of
//! metadata about the e-graph:
//!
//!   * The current partial assignment of nodes to classes. This only includes
//!   nodes whose children are also assigned.
//!   * The pending state, which include:
//!     - A provisional assignment of nodes to classes. This (logically)
//!     contains nodes whose dependencies may not be satisfied yet. We use a
//!     scheme similar to two-watch literals in SAT solvers to track
//!     dependencies efficiently here. Once all dependencies are satisfied, the
//!     assignment here is added to the main assignment.
//!     - A queue of classes to visit.
//!   * A stack of snapshots of the state, which we use to backtrack when we
//!     finish an MCTS playout or when we finish extracting random terms to
//!     estimate cost. Many of the data-structures here include quirks that make
//!     backtracking easier.
//!
//! The resulting scheme naturally handles cycles, because a cyclic assignment
//! will not be able to resolve all of its dependencies. We check for this case
//! when generating a complete assignment. A potential optimization would be to
//! add an "occurrs check" that filters out any potential assignments that would
//! introduce a cycle.
use fxhash::FxHashSet;
use indexmap::IndexMap;
use rand::Rng;
use smallvec::SmallVec;

use crate::{
    backtrack_queue::{BacktrackQueue, QueueSnapshot},
    Assignment, Egraph, EgraphTotalCost, Utility,
};

/// Given an egraph that can estimate the utility of an assignment, simulate
/// a random extraction given the partial extraion in `state` and return its
/// cost. Returns `None` is random extraction fails.
pub(crate) fn random_cost_estimate<E: EgraphTotalCost>(
    egraph: &E,
    state: &mut ExtractionState<E>,
    g: &mut impl Rng,
) -> Option<Utility> {
    // Push a snapshot so we can hand the state back like we got it.
    state.push_snapshot();
    let res = || -> Option<Utility> {
        // Scratch space to use for repeated allocations of enodes.
        let mut scratch = Vec::new();
        while let Some(handle) = state.start_next_assign() {
            scratch.extend(egraph.members(handle.class()));
            if scratch.is_empty() {
                return None;
            }
            let choice = g.gen_range(0..scratch.len());
            handle.assign(scratch[choice].clone(), egraph);
            scratch.clear();
        }
        Some(egraph.assignment_utility(state.complete_assignment()?))
    }();
    state.reset(egraph);
    state.pop_snapshot();
    res
}

pub(crate) struct ExtractionState<E: Egraph> {
    assign: Assignment<E>,
    pending: PendingState<E>,
    snapshots: Vec<StateSnapshot>,
}

#[derive(Debug)]
struct StateSnapshot {
    assign_len: usize,
    pending: PendingStateSnapshot,
}

impl<E: Egraph> ExtractionState<E> {
    pub(crate) fn new(root: E::ClassId) -> Self {
        let mut res = Self {
            assign: Default::default(),
            pending: Default::default(),
            snapshots: Default::default(),
        };
        res.pending.push_to_visit(root);
        res.push_snapshot();
        res
    }
    pub(crate) fn push_snapshot(&mut self) {
        self.snapshots.push(StateSnapshot {
            assign_len: self.assign.len(),
            pending: self.pending.save_snapshot(),
        });
    }

    pub(crate) fn reset(&mut self, egraph: &E) {
        let Some(snapshot) = self.snapshots.last() else {
            return;
        };
        self.assign.truncate(snapshot.assign_len);
        self.pending
            .restore(&snapshot.pending, &mut self.assign, egraph);
    }
    pub(crate) fn pop_snapshot(&mut self) {
        self.snapshots.pop();
    }
    pub(crate) fn complete_assignment(&self) -> Option<&Assignment<E>> {
        if self.pending.n_remaining == 0 && self.pending.to_visit_set.is_empty() {
            Some(&self.assign)
        } else {
            None
        }
    }
    fn provisional_assign(&mut self, class: E::ClassId, node: E::NodeId, egraph: &E) {
        self.pending.to_visit_set.remove(&class);
        self.pending
            .provisional_assign
            .insert(class.clone(), node.clone());
        self.pending.n_remaining += 1;
        self.pending.n_remaining -= self.pending.deps.track_pending_assignment(
            node.clone(),
            class,
            &mut self.assign,
            egraph.children(&node).cloned(),
        );
        for child in egraph
            .children(&node)
            .filter(|x| !self.assign.contains_key(*x))
        {
            self.pending.push_to_visit(child.clone());
        }
    }

    pub(crate) fn start_next_assign(&mut self) -> Option<AssignHandle<E>> {
        let next = self.pending.to_visit.front()?;
        assert!(
            self.pending.to_visit_set.contains(next),
            "missing class {next:?} in pending set"
        );
        Some(AssignHandle { state: self })
    }
}

struct PendingState<E: Egraph> {
    /// A provisional assignment from classes to nodes, includes nodes whose
    /// children have not been resolved.
    provisional_assign: Assignment<E>,
    /// The number of provisional assignments that are not final. (We don't
    /// directly remove from provisional_assign to make backtracking easier).
    n_remaining: usize,
    /// A data-structure tracking dependencies for provisional assignments. Once
    /// all dependencies are satisfied, the assignment is final.
    deps: Deps<E>,
    /// A queue of classes to visit, along with a set to prevent duplicates.
    to_visit: BacktrackQueue<E::ClassId>,
    to_visit_set: FxHashSet<E::ClassId>,
}

#[derive(Debug)]
struct PendingStateSnapshot {
    assign_len: usize,
    n_remaining: usize,
    to_visit: QueueSnapshot,
}

impl<E: Egraph> PendingState<E> {
    fn push_to_visit(&mut self, class: E::ClassId) {
        if !self.provisional_assign.contains_key(&class) && self.to_visit_set.insert(class.clone())
        {
            self.to_visit.push_back(class);
        }
    }

    fn save_snapshot(&self) -> PendingStateSnapshot {
        PendingStateSnapshot {
            assign_len: self.provisional_assign.len(),
            n_remaining: self.n_remaining,
            to_visit: self.to_visit.snapshot(),
        }
    }

    fn restore(
        &mut self,
        snapshot: &PendingStateSnapshot,
        full_assign: &mut Assignment<E>,
        egraph: &E,
    ) {
        self.provisional_assign.truncate(snapshot.assign_len);
        self.n_remaining = snapshot.n_remaining;
        self.to_visit.restore(&snapshot.to_visit);
        self.to_visit_set.clear();
        for entry in self.to_visit.iter() {
            self.to_visit_set.insert(entry.clone());
        }
        self.deps.clear();
        for (class, node) in self.provisional_assign.iter() {
            if full_assign.contains_key(class) {
                continue;
            }
            assert_eq!(
                self.deps.track_pending_assignment(
                    node.clone(),
                    class.clone(),
                    full_assign,
                    egraph.children(node).cloned(),
                ),
                0
            );
        }
    }
}

pub(crate) struct AssignHandle<'a, E: Egraph> {
    state: &'a mut ExtractionState<E>,
}

impl<E: Egraph> AssignHandle<'_, E> {
    pub(crate) fn class(&self) -> &E::ClassId {
        self.state.pending.to_visit.front().unwrap()
    }
    pub(crate) fn assign(self, node: E::NodeId, egraph: &E) {
        let class = self.state.pending.to_visit.pop_front().unwrap();
        self.state.provisional_assign(class, node, egraph);
    }
}

struct PendingNode<E: Egraph> {
    node: E::NodeId,
    class: E::ClassId,
    deps: SmallVec<[E::ClassId; 2]>,
}

struct Deps<E: Egraph> {
    data: IndexMap<E::ClassId, SmallVec<[PendingNode<E>; 1]>>,
}

impl<E: Egraph> Deps<E> {
    fn clear(&mut self) {
        self.data.clear();
    }
    fn resolve_dep(&mut self, class: E::ClassId, assign: &mut Assignment<E>) -> usize {
        // look at all pending nodes listening on the newly-resolved class.
        let mut assigned = 0;
        let Some(pending) = self.data.swap_remove(&class) else {
            return assigned;
        };
        for mut pending in pending {
            pending.deps.retain(|dep| !assign.contains_key(dep));
            if let Some(first) = pending.deps.first() {
                // Start watching another unresolved dependency.
                self.data.entry(first.clone()).or_default().push(pending);
            } else {
                // This was the last pending dependency for this node, so we can
                // safely assign it.
                assign.insert(pending.class.clone(), pending.node);
                assigned += self.resolve_dep(pending.class, assign) + 1;
            }
        }
        assigned
    }
    #[must_use]
    fn track_pending_assignment(
        &mut self,
        node: E::NodeId,
        class: E::ClassId,
        assign: &mut Assignment<E>,
        deps: impl Iterator<Item = E::ClassId>,
    ) -> usize {
        let deps = deps
            .filter(|x| !assign.contains_key(x))
            .collect::<SmallVec<[_; 2]>>();
        let Some(dep) = deps.first() else {
            // No pending dependencies! Make the final assignment to the node
            // and update any other provisional assignments that depend on it.
            assign.insert(class.clone(), node);
            return self.resolve_dep(class, assign) + 1;
        };
        self.data.entry(dep.clone()).or_default().push(PendingNode {
            node: node.clone(),
            class: class.clone(),
            deps,
        });
        0
    }
}

impl<E: Egraph> Default for PendingState<E> {
    fn default() -> Self {
        Self {
            provisional_assign: Default::default(),
            deps: Default::default(),
            to_visit: Default::default(),
            to_visit_set: Default::default(),
            n_remaining: 0,
        }
    }
}

impl<E: Egraph> Default for Deps<E> {
    fn default() -> Self {
        Self {
            data: Default::default(),
        }
    }
}
