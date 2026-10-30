//! `tsort`: topological sort of the whitespace-separated pairs in the input.
//!
//! Each pair `X Y` says X must come before Y. Nodes are first ordered by name (byte order);
//! the queue of ready nodes starts in that order and is served first-in first-out. A node's
//! successors are released last-added first, which matches GNU's output order.
//!
//! When no node is ready but some remain, the input has a loop. The smallest remaining node
//! starts a depth-first search; the cycle found is reported on stderr, and that node is then
//! released so the output still contains every node.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{self, Read, Write};

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{io_error_text, print_version, report, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, is_broken_pipe, open_input, stdout_writer};

const CMD: &str = "tsort";

const SPECS: &[OptSpec] = &[
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

const HELP: &str = "Usage: tsort [OPTION] [FILE]\n\
Write totally ordered list consistent with the partial ordering in FILE.\n\
With no FILE, or when FILE is -, read standard input.\n\n\
      --help     display this help and exit\n\
      --version  output version information and exit\n\n\
Exit status: 0 on success, 1 on error (odd token count, loop, unreadable input).\n";

/// The partial order read from the input.
#[derive(Default)]
struct Graph {
    names: Vec<Vec<u8>>,
    ids: HashMap<Vec<u8>, usize>,
    /// Successors per node, in insertion order.
    successors: Vec<Vec<usize>>,
    indegree: Vec<usize>,
    edges: HashSet<(usize, usize)>,
}

impl Graph {
    fn id(&mut self, name: &[u8]) -> usize {
        if let Some(&id) = self.ids.get(name) {
            return id;
        }
        let id = self.names.len();
        self.names.push(name.to_vec());
        self.ids.insert(name.to_vec(), id);
        self.successors.push(Vec::new());
        self.indegree.push(0);
        id
    }

    /// Adds `before -> after`. Self-edges and repeated edges add nothing.
    fn add_edge(&mut self, before: &[u8], after: &[u8]) {
        let from = self.id(before);
        let to = self.id(after);
        if from != to && self.edges.insert((from, to)) {
            self.successors[from].push(to);
            self.indegree[to] += 1;
        }
    }
}

pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new(CMD, SPECS).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage(CMD, &error, EXIT_FAIL),
    };
    let mut operands = Vec::new();
    for item in &items {
        match item {
            Item::Opt { id: "help", .. } => {
                print!("{HELP}");
                return EXIT_OK;
            }
            Item::Opt { id: "version", .. } => return print_version(CMD),
            Item::Opt { .. } => {}
            Item::Operand(name) => operands.push(name.clone()),
        }
    }
    if operands.len() > 1 {
        eprintln!(
            "{CMD}: extra operand '{}'\nTry '{CMD} --help' for more information.",
            operands[1]
        );
        return EXIT_FAIL;
    }
    let name = operands.pop().unwrap_or_else(|| "-".to_owned());
    let data = match read_input(&name) {
        Ok(data) => data,
        Err(error) => {
            report(CMD, &name, &error);
            return EXIT_FAIL;
        }
    };
    let tokens: Vec<&[u8]> = data
        .split(|byte| byte.is_ascii_whitespace())
        .filter(|token| !token.is_empty())
        .collect();
    if !tokens.len().is_multiple_of(2) {
        eprintln!("{CMD}: {name}: input contains an odd number of tokens");
        return EXIT_FAIL;
    }
    let mut graph = Graph::default();
    for pair in tokens.chunks(2) {
        graph.add_edge(pair[0], pair[1]);
    }
    let mut out = stdout_writer();
    let mut status = EXIT_OK;
    if let Err(error) = sort_into(&mut graph, &name, &mut out, &mut status) {
        if is_broken_pipe(&error) {
            return EXIT_OK;
        }
        eprintln!("{CMD}: write error: {}", io_error_text(&error));
        return EXIT_FAIL;
    }
    let flushed = finish_output(CMD, &mut out);
    if flushed != EXIT_OK {
        return flushed;
    }
    status
}

fn read_input(name: &str) -> io::Result<Vec<u8>> {
    let mut data = Vec::new();
    open_input(name)?.read_to_end(&mut data)?;
    Ok(data)
}

/// Writes the nodes in topological order. `status` becomes `EXIT_FAIL` when a loop is found.
fn sort_into(
    graph: &mut Graph,
    name: &str,
    out: &mut impl Write,
    status: &mut i32,
) -> io::Result<()> {
    let mut sorted: Vec<usize> = (0..graph.names.len()).collect();
    sorted.sort_by(|a, b| graph.names[*a].cmp(&graph.names[*b]));
    let mut done = vec![false; graph.names.len()];
    let mut queued = vec![false; graph.names.len()];
    let mut ready: VecDeque<usize> = VecDeque::new();
    for &node in &sorted {
        if graph.indegree[node] == 0 {
            queued[node] = true;
            ready.push_back(node);
        }
    }
    let mut next_unsorted = 0;
    loop {
        while let Some(node) = ready.pop_front() {
            done[node] = true;
            out.write_all(&graph.names[node])?;
            out.write_all(b"\n")?;
            for &successor in graph.successors[node].iter().rev() {
                if done[successor] {
                    continue;
                }
                graph.indegree[successor] = graph.indegree[successor].saturating_sub(1);
                if graph.indegree[successor] == 0 && !queued[successor] {
                    queued[successor] = true;
                    ready.push_back(successor);
                }
            }
        }
        while next_unsorted < sorted.len() && done[sorted[next_unsorted]] {
            next_unsorted += 1;
        }
        let Some(&start) = sorted.get(next_unsorted) else {
            return Ok(());
        };
        *status = EXIT_FAIL;
        report_loop(graph, start, name);
        queued[start] = true;
        ready.push_back(start);
    }
}

/// Reports the cycle reachable from `start` among the unfinished nodes.
fn report_loop(graph: &Graph, start: usize, name: &str) {
    let cycle = find_cycle(graph, start);
    eprintln!("{CMD}: {name}: input contains a loop:");
    for node in cycle {
        eprintln!("{CMD}: {}", String::from_utf8_lossy(&graph.names[node]));
    }
}

/// Depth-first search from `start` over unfinished successors. Returns the nodes of the first
/// cycle met, in path order. Nodes that cannot reach a cycle are skipped.
fn find_cycle(graph: &Graph, start: usize) -> Vec<usize> {
    let mut on_path = vec![false; graph.names.len()];
    let mut visited = vec![false; graph.names.len()];
    let mut path: Vec<(usize, usize)> = vec![(start, 0)];
    on_path[start] = true;
    visited[start] = true;
    while let Some(&(node, next)) = path.last() {
        let candidates = &graph.successors[node];
        let Some(&successor) = candidates.iter().rev().nth(next) else {
            on_path[node] = false;
            path.pop();
            continue;
        };
        path.last_mut().expect("path is not empty").1 += 1;
        if on_path[successor] {
            let at = path.iter().position(|&(n, _)| n == successor).unwrap_or(0);
            return path[at..].iter().map(|&(n, _)| n).collect();
        }
        if !visited[successor] {
            visited[successor] = true;
            on_path[successor] = true;
            path.push((successor, 0));
        }
    }
    vec![start]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order(input: &str) -> (Vec<String>, Vec<String>) {
        let tokens: Vec<&[u8]> = input
            .as_bytes()
            .split(|byte| byte.is_ascii_whitespace())
            .filter(|token| !token.is_empty())
            .collect();
        let mut graph = Graph::default();
        for pair in tokens.chunks(2) {
            graph.add_edge(pair[0], pair[1]);
        }
        let mut out = Vec::new();
        let mut status = EXIT_OK;
        sort_into(&mut graph, "-", &mut out, &mut status).unwrap();
        let lines = String::from_utf8(out)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect();
        let loops = if status == EXIT_FAIL {
            vec!["loop".to_owned()]
        } else {
            Vec::new()
        };
        (lines, loops)
    }

    #[test]
    fn orders_chains_and_independent_pairs_like_gnu() {
        assert_eq!(order("a b\nc d\n").0, ["a", "c", "b", "d"]);
        assert_eq!(order("a b b c a d\n").0, ["a", "d", "b", "c"]);
        assert_eq!(order("a d\na b\na c\n").0, ["a", "c", "b", "d"]);
    }

    #[test]
    fn self_edges_and_repeats_are_ignored() {
        assert_eq!(order("a a\n").0, ["a"]);
        assert_eq!(order("a b a b\n").0, ["a", "b"]);
    }

    #[test]
    fn loops_are_reported_and_every_node_is_still_printed() {
        assert_eq!(
            order("a b\nb a\n"),
            (vec!["a".into(), "b".into()], vec!["loop".into()])
        );
        assert_eq!(order("a b\nb c\nc a\nd e\n").0, ["d", "e", "a", "b", "c"]);
        assert_eq!(order("a b\nb c\nc b\nc d\n").0, ["a", "b", "c", "d"]);
    }

    #[test]
    fn cycle_search_returns_the_loop_members() {
        let mut graph = Graph::default();
        graph.add_edge(b"x", b"y");
        graph.add_edge(b"y", b"z");
        graph.add_edge(b"z", b"y");
        let x = graph.ids[b"x".as_slice()];
        let members: Vec<&[u8]> = find_cycle(&graph, x)
            .into_iter()
            .map(|node| graph.names[node].as_slice())
            .collect();
        assert_eq!(members, [b"y".as_slice(), b"z".as_slice()]);
    }

    #[test]
    fn empty_input_has_no_output() {
        assert_eq!(order("").0, Vec::<String>::new());
    }
}
