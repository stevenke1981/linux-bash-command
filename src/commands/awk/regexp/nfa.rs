use super::syntax::{Node, Test};
use std::collections::VecDeque;
#[derive(Clone)]
pub(super) enum State {
    Consume(Test, usize),
    Split(usize, usize),
    Start(usize),
    End(usize),
    Accept,
    Reject,
}
#[derive(Clone)]
pub(super) struct Program {
    pub states: Vec<State>,
    pub start: usize,
    pub can_finish: Vec<bool>,
    pub zero_finish: Vec<bool>,
}
impl Program {
    pub fn compile(node: &Node) -> Result<Self, String> {
        let mut p = Self {
            states: vec![State::Accept],
            start: 0,
            can_finish: Vec::new(),
            zero_finish: Vec::new(),
        };
        p.start = p.node(node, 0)?;
        p.zero_finish = p.reachable(true, &[0]);
        let mut seeds = vec![0];
        for (index, state) in p.states.iter().enumerate() {
            if let State::End(out) = state {
                if p.zero_finish[*out] {
                    seeds.push(index);
                }
            }
        }
        p.can_finish = p.reachable(false, &seeds);
        Ok(p)
    }
    fn push(&mut self, state: State) -> Result<usize, String> {
        const STATE_BYTES: usize = 10 * 1024 * 1024;
        if self.states.len() >= STATE_BYTES / std::mem::size_of::<State>() {
            return Err("regular expression exceeds 10 MiB state compilation budget".into());
        }
        let index = self.states.len();
        self.states.push(state);
        Ok(index)
    }
    fn node(&mut self, node: &Node, out: usize) -> Result<usize, String> {
        match node {
            Node::Empty => Ok(out),
            Node::Consume(test) => self.push(State::Consume(test.clone(), out)),
            Node::Start => self.push(State::Start(out)),
            Node::End => self.push(State::End(out)),
            Node::Sequence(nodes) => {
                let mut tail = out;
                for node in nodes.iter().rev() {
                    tail = self.node(node, tail)?;
                }
                Ok(tail)
            }
            Node::Alternative(nodes) => {
                let mut starts = nodes
                    .iter()
                    .map(|node| self.node(node, out))
                    .collect::<Result<Vec<_>, _>>()?;
                let mut tail = starts.pop().unwrap_or(out);
                for start in starts.into_iter().rev() {
                    tail = self.push(State::Split(start, tail))?;
                }
                Ok(tail)
            }
            Node::Repeat(node, min, max) => {
                if !node.possible() {
                    return if *min == 0 {
                        Ok(out)
                    } else {
                        self.push(State::Reject)
                    };
                }
                if !node.consumes() {
                    return if *min == 0 {
                        Ok(out)
                    } else {
                        self.node(node, out)
                    };
                }
                if let Node::Repeat(_, 0, None) = node.as_ref() {
                    return if *max == Some(0) {
                        Ok(out)
                    } else {
                        self.node(node, out)
                    };
                }
                let mut tail = if let Some(max) = max {
                    let mut tail = out;
                    for _ in *min..*max {
                        let start = self.node(node, tail)?;
                        tail = self.push(State::Split(start, tail))?;
                    }
                    tail
                } else {
                    let repeat = self.push(State::Split(0, out))?;
                    let start = self.node(node, repeat)?;
                    self.states[repeat] = State::Split(start, out);
                    repeat
                };
                for _ in 0..*min {
                    tail = self.node(node, tail)?;
                }
                Ok(tail)
            }
        }
    }
    fn reachable(&self, zero: bool, seeds: &[usize]) -> Vec<bool> {
        let mut predecessors = vec![Vec::new(); self.states.len()];
        for (index, state) in self.states.iter().enumerate() {
            match state {
                State::Split(a, b) => {
                    predecessors[*a].push(index);
                    predecessors[*b].push(index);
                }
                State::End(out) if zero => predecessors[*out].push(index),
                State::Consume(test, out) if !zero && test.possible() => {
                    predecessors[*out].push(index)
                }
                _ => {}
            }
        }
        let mut reachable = vec![false; self.states.len()];
        let mut queue = VecDeque::new();
        for &seed in seeds {
            if !reachable[seed] {
                reachable[seed] = true;
                queue.push_back(seed);
            }
        }
        while let Some(state) = queue.pop_front() {
            for &before in &predecessors[state] {
                if !reachable[before] {
                    reachable[before] = true;
                    queue.push_back(before);
                }
            }
        }
        reachable
    }
}
