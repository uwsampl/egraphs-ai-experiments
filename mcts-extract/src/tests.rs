use crate::{mcts_extract, simple_egraph::SimpleEgraph, Assignment, MctsConfig, Utility};

#[test]
fn finds_high_util() {
    // Set up a simple egraph with a single term with positive utility.
    let nodes = vec![
        vec![2, 1],
        vec![2, 2],
        vec![2, 3],
        vec![3],
        vec![3, 3],
        vec![],
    ];

    let classes = vec![vec![0, 1], vec![2, 3], vec![4], vec![5]];

    let egraph = SimpleEgraph {
        nodes,
        classes,
        score_fn: Box::new(score_fn),
    };

    let assign = mcts_extract(
        &egraph,
        0,
        MctsConfig {
            playouts_per_round: 4,
            terms_to_sample: 4,
        },
    )
    .expect("extraction should succeed");
    assert_eq!(assign.len(), 3);
    assert_eq!(assign[&0], 1);
    assert_eq!(assign[&2], 4);
    assert_eq!(assign[&3], 5);
}

#[test]
fn fails_unextractable() {
    // Set up a small egraph with no valid extractions
    let nodes = vec![
        vec![0, 1],
        vec![2, 2],
        vec![3, 2],
        vec![0, 3],
        vec![1, 0],
        vec![2, 3, 1],
    ];

    let classes = vec![vec![0, 1], vec![2, 3], vec![4], vec![5]];

    let egraph = SimpleEgraph {
        nodes,
        classes,
        score_fn: Box::new(score_fn),
    };

    assert!(mcts_extract(
        &egraph,
        0,
        MctsConfig {
            playouts_per_round: 4,
            terms_to_sample: 4,
        },
    )
    .is_none());
}

/// Simple score function in used in some tests.
fn score_fn(assignment: &Assignment<SimpleEgraph>, _: &SimpleEgraph) -> Utility {
    if assignment.get(&0) == Some(&1)
        && assignment.get(&2) == Some(&4)
        && assignment.get(&3) == Some(&5)
    {
        Utility::new(1.0).unwrap()
    } else {
        Utility::new(0.0).unwrap()
    }
}
