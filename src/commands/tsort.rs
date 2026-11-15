use crate::common::{args, error, io};
use std::collections::{HashMap, HashSet, VecDeque};

const HELP: &str = "Usage: tsort [OPTION] [FILE]\nWrite a topological sort of the input pairs.\n      --help     display this help\n      --version  output version information\n";

#[derive(Default)]
struct Graph {
    nodes: Vec<Vec<u8>>,
    edges: Vec<Vec<usize>>,
    reverse: Vec<Vec<usize>>,
    seen_edges: HashSet<(usize, usize)>,
}

impl Graph {
    fn node(&mut self, text: &[u8], indices: &mut HashMap<Vec<u8>, usize>) -> usize {
        if let Some(&index) = indices.get(text) {
            return index;
        }
        let index = self.nodes.len();
        let key = text.to_vec();
        indices.insert(key.clone(), index);
        self.nodes.push(key);
        self.edges.push(Vec::new());
        self.reverse.push(Vec::new());
        index
    }

    fn add_edge(&mut self, from: usize, to: usize) {
        if from == to {
            return;
        }
        if self.seen_edges.insert((from, to)) {
            self.edges[from].push(to);
            self.reverse[to].push(from);
        }
    }
}

fn build_graph(data: &[u8]) -> Result<Graph, ()> {
    let tokens = data
        .split(|byte| byte.is_ascii_whitespace())
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    if tokens.len() % 2 != 0 {
        return Err(());
    }
    let mut graph = Graph::default();
    let mut indices = HashMap::new();
    for pair in tokens.as_chunks::<2>().0 {
        let from = graph.node(pair[0], &mut indices);
        let to = graph.node(pair[1], &mut indices);
        graph.add_edge(from, to);
    }
    Ok(graph)
}

fn cyclic_nodes(graph: &Graph) -> Vec<bool> {
    let count = graph.nodes.len();
    let mut visited = vec![false; count];
    let mut finish = Vec::with_capacity(count);
    for start in 0..count {
        if visited[start] {
            continue;
        }
        visited[start] = true;
        let mut stack = vec![(start, 0usize)];
        while let Some((node, next)) = stack.last_mut() {
            if *next < graph.edges[*node].len() {
                let child = graph.edges[*node][*next];
                *next += 1;
                if !visited[child] {
                    visited[child] = true;
                    stack.push((child, 0));
                }
            } else {
                finish.push(*node);
                stack.pop();
            }
        }
    }
    let mut assigned = vec![false; count];
    let mut cyclic = vec![false; count];
    for start in finish.into_iter().rev() {
        if assigned[start] {
            continue;
        }
        assigned[start] = true;
        let mut component = Vec::new();
        let mut stack = vec![start];
        while let Some(node) = stack.pop() {
            component.push(node);
            for &parent in &graph.reverse[node] {
                if !assigned[parent] {
                    assigned[parent] = true;
                    stack.push(parent);
                }
            }
        }
        let self_loop = component.len() == 1 && graph.edges[component[0]].contains(&component[0]);
        if component.len() > 1 || self_loop {
            for node in component {
                cyclic[node] = true;
            }
        }
    }
    cyclic
}

fn order(graph: &Graph) -> (Vec<usize>, Vec<bool>) {
    let mut indegree = graph.reverse.iter().map(Vec::len).collect::<Vec<_>>();
    let mut ready = indegree
        .iter()
        .enumerate()
        .filter_map(|(index, degree)| (*degree == 0).then_some(index))
        .collect::<VecDeque<_>>();
    let mut result = Vec::with_capacity(graph.nodes.len());
    let mut emitted = vec![false; graph.nodes.len()];
    while let Some(node) = ready.pop_front() {
        if emitted[node] {
            continue;
        }
        emitted[node] = true;
        result.push(node);
        for &next in &graph.edges[node] {
            indegree[next] -= 1;
            if indegree[next] == 0 {
                ready.push_back(next);
            }
        }
    }
    let cycle_nodes = cyclic_nodes(graph);
    for (index, seen) in emitted.iter().enumerate() {
        if !seen {
            result.push(index);
        }
    }
    (result, cycle_nodes)
}

fn read(path: &str) -> Result<Vec<u8>, String> {
    let mut reader = io::input(path).map_err(|e| format!("{path}: {}", error::io_text(&e)))?;
    let mut data = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut data)
        .map_err(|e| format!("{path}: {}", error::io_text(&e)))?;
    Ok(data)
}

pub fn run(args: &[String]) -> i32 {
    let parsed = match args::parse_command("tsort", args, HELP, &[], true) {
        Ok(items) => items,
        Err(code) => return code,
    };
    let files = parsed
        .into_iter()
        .filter_map(|item| match item {
            args::Item::Operand(path) => Some(path),
            _ => None,
        })
        .collect::<Vec<_>>();
    if files.len() > 1 {
        return error::usage("tsort", format!("extra operand '{}'", files[1]), 1);
    }
    let file = files.first().map(String::as_str).unwrap_or("-");
    let data = match read(file) {
        Ok(data) => data,
        Err(message) => {
            eprintln!("tsort: {message}");
            return 1;
        }
    };
    let graph = match build_graph(&data) {
        Ok(graph) => graph,
        Err(()) => {
            eprintln!("tsort: {file}: input contains an odd number of tokens");
            return 1;
        }
    };
    let (sorted, cyclic) = order(&graph);
    let code = io::output("tsort", |out| {
        for index in sorted {
            out.write_all(&graph.nodes[index])?;
            out.write_all(b"\n")?;
        }
        Ok(())
    });
    if code != 0 {
        return code;
    }
    if cyclic.iter().any(|member| *member) {
        eprintln!("tsort: {file}: input contains a loop:");
        for (index, member) in cyclic.iter().enumerate() {
            if *member {
                eprintln!("{}", String::from_utf8_lossy(&graph.nodes[index]));
            }
        }
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_first_seen_kahn_queue_order() {
        let graph = build_graph(b"a b c d e f a c").unwrap();
        let (order, cycles) = order(&graph);
        let labels = order
            .iter()
            .map(|i| String::from_utf8_lossy(&graph.nodes[*i]).to_string())
            .collect::<Vec<_>>();
        assert_eq!(labels, ["a", "e", "b", "c", "f", "d"]);
        assert!(!cycles.iter().any(|node| *node));
    }

    #[test]
    fn finds_cycle_members_without_marking_dependents() {
        let graph = build_graph(b"a b b a b c d e").unwrap();
        let (result, cycles) = order(&graph);
        assert_eq!(result.len(), 5);
        assert_eq!(cycles, [true, true, false, false, false]);
    }

    #[test]
    fn rejects_odd_token_count() {
        assert!(build_graph(b"a b c").is_err());
    }

    #[test]
    fn deduplicates_repeated_edges() {
        let graph = build_graph(b"a b a b b c").unwrap();
        let (sorted, cycles) = order(&graph);
        assert_eq!(sorted.len(), 3);
        assert!(!cycles.iter().any(|v| *v));
    }

    #[test]
    fn self_pair_declares_node_without_creating_cycle() {
        let graph = build_graph(b"a a").unwrap();
        let (sorted, cycles) = order(&graph);
        assert_eq!(sorted, [0]);
        assert_eq!(graph.nodes, [b"a".to_vec()]);
        assert!(!cycles[0]);
    }
}
