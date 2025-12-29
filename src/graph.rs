use petgraph::visit::Bfs;
use petgraph::visit::EdgeRef;
use petgraph::{Direction, Graph};
use std::collections::HashMap;
use std::rc::Rc;

use crate::api::PullRequest;

pub type FlatDep = Vec<(Rc<PullRequest>, Option<Rc<PullRequest>>)>;

pub fn build(prs: &[Rc<PullRequest>]) -> Graph<Rc<PullRequest>, usize> {
    let mut tree = Graph::<Rc<PullRequest>, usize>::new();
    let heads = prs.iter().map(|pr| pr.head());
    let handles: Vec<_> = prs.iter().map(|pr| tree.add_node(pr.clone())).collect();
    let handles_by_head: HashMap<_, _> = heads.zip(handles.iter()).collect();

    for (i, pr) in prs.iter().enumerate() {
        let head_handle = handles[i];
        if let Some(&base_handle) = handles_by_head.get(pr.base()) {
            tree.add_edge(*base_handle, head_handle, 1);
        }
    }

    tree
}

/// Return a flattened list of graph nodes as tuples; each tuple is `(node, node's parent [if exists])`.
/// TODO: Panic if this isn't a single flat list of dependencies
pub fn log(graph: &Graph<Rc<PullRequest>, usize>) -> FlatDep {
    let roots: Vec<_> = graph.externals(Direction::Incoming).collect();
    let mut out = Vec::new();

    for root in roots {
        let mut bfs = Bfs::new(&graph, root);
        while let Some(node) = bfs.next(&graph) {
            let parent = graph.edges_directed(node, Direction::Incoming).next();
            let node: Rc<PullRequest> = graph[node].clone();

            match parent {
                Some(parent) => out.push((node, Some(graph[parent.source()].clone()))),
                None => out.push((node, None)),
            }
        }
    }

    out.sort_by_key(|(dep, _)| dep.state().clone());

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{PullRequest, PullRequestStatus};

    fn make_pr(number: usize, head: &str, base: &str) -> Rc<PullRequest> {
        Rc::new(PullRequest::new_for_test(
            number,
            head,
            base,
            &format!("PR #{}", number),
            PullRequestStatus::Open,
            false,
            None,
            vec![],
        ))
    }

    #[test]
    fn test_build_empty_graph() {
        let prs: Vec<Rc<PullRequest>> = vec![];
        let graph = build(&prs);
        assert_eq!(graph.node_count(), 0);
        assert_eq!(graph.edge_count(), 0);
    }

    #[test]
    fn test_build_single_pr() {
        let prs = vec![make_pr(1, "feature-1", "main")];
        let graph = build(&prs);
        assert_eq!(graph.node_count(), 1);
        assert_eq!(graph.edge_count(), 0); // No edge since base "main" is not a PR
    }

    #[test]
    fn test_build_linear_stack() {
        // PR 1: feature-1 -> main
        // PR 2: feature-2 -> feature-1
        // PR 3: feature-3 -> feature-2
        let prs = vec![
            make_pr(1, "feature-1", "main"),
            make_pr(2, "feature-2", "feature-1"),
            make_pr(3, "feature-3", "feature-2"),
        ];
        let graph = build(&prs);
        assert_eq!(graph.node_count(), 3);
        assert_eq!(graph.edge_count(), 2); // feature-1 -> feature-2, feature-2 -> feature-3
    }

    #[test]
    fn test_build_branching_stack() {
        // PR 1: feature-1 -> main
        // PR 2: feature-2a -> feature-1
        // PR 3: feature-2b -> feature-1 (branching)
        let prs = vec![
            make_pr(1, "feature-1", "main"),
            make_pr(2, "feature-2a", "feature-1"),
            make_pr(3, "feature-2b", "feature-1"),
        ];
        let graph = build(&prs);
        assert_eq!(graph.node_count(), 3);
        assert_eq!(graph.edge_count(), 2); // Both branch from feature-1
    }

    #[test]
    fn test_log_linear_stack() {
        let prs = vec![
            make_pr(1, "feature-1", "main"),
            make_pr(2, "feature-2", "feature-1"),
            make_pr(3, "feature-3", "feature-2"),
        ];
        let graph = build(&prs);
        let flat = log(&graph);

        assert_eq!(flat.len(), 3);

        // First PR should have no parent (base is main, not in stack)
        assert!(flat.iter().any(|(pr, parent)| pr.number() == 1 && parent.is_none()));

        // Second PR should have first as parent
        assert!(flat
            .iter()
            .any(|(pr, parent)| pr.number() == 2 && parent.as_ref().map(|p| p.number()) == Some(1)));

        // Third PR should have second as parent
        assert!(flat
            .iter()
            .any(|(pr, parent)| pr.number() == 3 && parent.as_ref().map(|p| p.number()) == Some(2)));
    }

    #[test]
    fn test_log_sorts_by_state() {
        let open_pr = Rc::new(PullRequest::new_for_test(
            1,
            "feature-1",
            "main",
            "Open PR",
            PullRequestStatus::Open,
            false,
            None,
            vec![],
        ));
        let closed_pr = Rc::new(PullRequest::new_for_test(
            2,
            "feature-2",
            "other",
            "Closed PR",
            PullRequestStatus::Closed,
            false,
            None,
            vec![],
        ));

        let prs = vec![closed_pr, open_pr];
        let graph = build(&prs);
        let flat = log(&graph);

        // Open PRs should come before Closed PRs after sorting
        assert_eq!(flat[0].0.number(), 1); // Open PR first
        assert_eq!(flat[1].0.number(), 2); // Closed PR second
    }
}
