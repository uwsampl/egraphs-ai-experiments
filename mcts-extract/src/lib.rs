//! A library for performing egraph extraction using Monte-Carlo Tree Search.

use std::{fmt::Debug, hash::Hash};

use extraction_state::{random_cost_estimate, ExtractionState};
use fxhash::FxBuildHasher;
use indexmap::IndexMap;
use ordered_float::NotNan;
use rand::thread_rng;
use search_tree::SearchTree;

pub(crate) mod backtrack_queue;
pub(crate) mod extraction_state;
pub(crate) mod search_tree;
pub(crate) mod simple_egraph;
#[cfg(test)]
mod tests;

/// Tuning params for the search.
#[derive(Clone)]
pub struct MctsConfig {
    /// The number of playouts to run per node in the egraph.
    pub playouts_per_round: usize,

    /// The number of terms to sample when estimating the utility of partial
    /// assignments.
    pub terms_to_sample: usize,
}

/// The type used for cost estimates for an egraph. In keeping with the MCTS
/// literature, we use "utility" where lower-cost extractions will have higher
/// utility.
pub type Utility = NotNan<f32>;

/// An assignment is a mapping from class ids to node ids.
///
/// Assignments can be partial or complete.
pub type Assignment<E> = IndexMap<<E as Egraph>::ClassId, <E as Egraph>::NodeId, FxBuildHasher>;

/// The core egraph specifications: Egraphs represent equivalence classes of
/// terms. Classes contain possible nodes, whose children are themselves
/// equivalence classes.
pub trait Egraph {
    type NodeId: Clone + Hash + Eq + Debug;
    type ClassId: Clone + Hash + Eq + Debug;
    fn children(&self, id: &Self::NodeId) -> impl Iterator<Item = &Self::ClassId>;
    fn members(&self, id: &Self::ClassId) -> impl Iterator<Item = &Self::NodeId>;
}

/// An Egraph that also has a means of estimating the total cost associated with
/// an assignment.
pub trait EgraphTotalCost: Egraph {
    /// The cost of the total assignment for the egraph.
    ///
    /// If the assignment is not compelete, this method may panic.
    fn assignment_utility(&self, assignment: &Assignment<Self>) -> Utility;
}

/// Extract an assignment from an egraph using Monte-Carlo Tree Search.
///
/// Returns `None` if extraction fails.
pub fn mcts_extract<E: EgraphTotalCost>(
    egraph: &E,
    root: E::ClassId,
    config: MctsConfig,
) -> Option<Assignment<E>> {
    let mut tree = SearchTree::<E>::new(root);
    let n_samples = config.terms_to_sample;
    let mut rng = thread_rng();
    let mut searcher = tree.start_round(
        |partial_assign: &mut ExtractionState<E>, eg: &E| -> Utility {
            if let Some(assign) = partial_assign.complete_assignment() {
                eg.assignment_utility(assign)
            } else {
                let mut util = Utility::default();
                for _ in 0..n_samples {
                    // If we fail to extract, count that run as 0 utility.
                    // XXX: This probably isn't the best way to handle this! We
                    // should revisit later. It'd be better to resample here but
                    // just bail if we fail to extract after 10*n samples or
                    // some such.
                    util += random_cost_estimate(eg, partial_assign, &mut rng).unwrap_or_default();
                }
                util / Utility::new(n_samples as f32).unwrap()
            }
        },
        Utility::new(2.0f32.sqrt()).unwrap(),
    );

    searcher.assign(&config, egraph)
}
