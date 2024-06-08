# Egraph extraction using Monte-Carlo Tree Search (MCTS)

This document contains (rough!) notes on what's going on in this repo. The goal
is to help bring equality saturaiton to bear on domains where it is difficult to
build a traditional cost mode. The contents of this repo are very much in
progress.

## Basic Definitions

An *egraph* is a set of *e-classes* and *e-nodes*. E-classes contain sets of
e-nodes, and e-nodes have an ordered sequence of child e-classes. A *term* that
is *extracted* from an e-class *c* is a minimal mapping (or assignment) of nodes to
classes where:

* The root class has a mapping.
* For every e-node in the mapping, all of its children have an assignment.

It is commont to add constraints here. The code in this repo, for example,
assumes that no extracted term may contain a cycle.

## Existing Extraction Algorithms

Extraction algorithms try to find a term represented by an e-class with the
minimum possible cost. The algorithms in
[extraction-gym](https://github.com/egraphs-good/extraction-gym) require that
each e-node has a cost associated with it; they then work to minimize the *sum*
of all per-enode costs in the extracted term. Egg lets you look at the a node's
children before estimating its cost.

Even the narrow "per-node cost" variant of this problem is 
[difficult to solve](https://effect.systems/blog/egraph-extraction.html), but in
some settings the model can be too restrictive.

## Limitations to Existing Cost Models

Depending on the domain, cost models of the form supported by extraction-gym or
egg implicitly make strong assumptions about the _actual_ cost of a term. For
example, if we were writing a compiler via equality saturation where each e-node
was a single instruction of some kind, we would effectively be assuming that the
cost of a program is some linear function of the instructions that get executed:
[state-of-the-art tools](https://llvm.org/docs/CommandGuide/llvm-mca.html) for
estimating the cost of code sequences do not make this assumption
This problem gets even harder once the e-graph can contain higher-level
constructs like function calls or loops. It is very difficult to estimate the
cost of a loop a priori when declaring an e-graph: furthermore, with a fixed
cost model it is very difficult to justify loop unrolling optimizations at all,
unless we can eliminate the loop entirely.

Compilers are the domain I've thought about the most, but other domains like
query optimization and place and route may run into this issue as well.

## This Repo

This repo aims to extract programs from an e-graph under the assumption that we
can _only_ estimate the cost of entire terms. This opens up settings where the
cost model is just "how long it takes to run the code 100 times" or "the number
of correct bits in a floating point expression." The initial algorithm here
[Monte-Carlo Tree Search](https://en.wikipedia.org/wiki/Monte_Carlo_tree_search)
(MCTS), which is an algorithm that comes out of the game-playing literature.

The core idea is to view egraph extraction as a single-player game, where
assigning an e-node to an e-class is a move. The game ends when we have
extracted a full term fro the e-graph, at which point we can estimate its cost.
MCTS then takes these "leaf costs" and propagates them back up the tree of
choices we've made. Over the course of enough playouts, MCTS _may_ just get a
reasonable model of which nodes are best to pick.

The MCTS algorithm randomly assigns nodes to classes to estimate the utility
from making a certain assignment, and then propagates that information up a tree
of choices, balancing exploration and exploitation when trying out different
assignments.

The algorithm in this repo visits nodes *top-down* so as to minimize the number
of nodes we have to visit before extracting a term. This makes the code a bit
more cumbersome than bottom-up extraction algorithms, which do not have to worry
about explicitly tracking whether dependencies on a node have been resolved. All
of the code for top-down partial terms is handled in its own module, however,
the core MCTS stuff should be fairly straightforward.

## Things to Look at

* Finding a good domain to see if this works at all. It is highly speculative
and the inefficiency of the search may outweigh any of the added accuracy of the
cost model for any reasonable timeframe of running.

* *AlphaGo-style MCTS* A famous variant of MCTS replaces statistics computed
during random search with the output of a neural net. The advantage of neural 
nets here, in addition to producing lower-cost outputs, is that they may
allow us to generalize to examples that we "cannot run." That's an example
that's important for compilers, where running a function being compiled isn't
necessarily possible, but having a suite of benchmarks on which to train an
extractor is achievable.

* There are also plenty of other random search algorithms in the literature (e.g.
simulated annealing, hill climbing, etc.) that could help here as well. It may
be worth implementing a suite of them.

* MCTS can also be interpreted to estimate a probability with which different
nodes should be chosen for a given e-class. We could leverage this
interpretation to sample terms from an e-graph and use them as a basis for beam
search to continue equality saturation further.