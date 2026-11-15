use super::nfa::{Program, State};
#[derive(Clone, Copy)]
struct Thread {
    state: usize,
    start: usize,
}
pub(crate) struct Scanner<'a> {
    program: &'a Program,
    position: usize,
    current: Vec<Thread>,
    next: Vec<Thread>,
    stack: Vec<Thread>,
    seen: Vec<usize>,
    epoch: usize,
    best: Option<(usize, usize)>,
    ignore_empty: bool,
    injected: Option<usize>,
}
pub(crate) struct Scan {
    pub found: Option<(usize, usize)>,
    pub pending: bool,
}
impl<'a> Scanner<'a> {
    pub(super) fn new(program: &'a Program, at: usize, ignore_empty: bool) -> Self {
        Self {
            program,
            position: at,
            current: Vec::new(),
            next: Vec::new(),
            stack: Vec::new(),
            seen: vec![0; program.states.len()],
            epoch: 0,
            best: None,
            ignore_empty,
            injected: None,
        }
    }
    pub fn feed(&mut self, text: &[u8], eof: bool) -> Scan {
        loop {
            self.epoch = self.epoch.wrapping_add(1);
            if self.epoch == 0 {
                self.seen.fill(0);
                self.epoch = 1;
            }
            self.next.clear();
            let boundary = self.position == text.len();
            let mut future = usize::MAX;
            let mut end_pending = usize::MAX;
            let mut seeds = std::mem::take(&mut self.current);
            if self.best.is_none() && self.injected != Some(self.position) {
                seeds.push(Thread {
                    state: self.program.start,
                    start: self.position,
                });
                self.injected = Some(self.position);
            }
            for &seed in &seeds {
                self.stack.push(seed);
                while let Some(thread) = self.stack.pop() {
                    if self.seen[thread.state] == self.epoch
                        || self.best.is_some_and(|(start, _)| thread.start > start)
                    {
                        continue;
                    }
                    self.seen[thread.state] = self.epoch;
                    match &self.program.states[thread.state] {
                        State::Split(a, b) => {
                            self.stack.push(Thread {
                                state: *b,
                                ..thread
                            });
                            self.stack.push(Thread {
                                state: *a,
                                ..thread
                            });
                        }
                        State::Start(out) => {
                            if self.position == 0 {
                                self.stack.push(Thread {
                                    state: *out,
                                    ..thread
                                });
                            }
                        }
                        State::End(out) => {
                            if boundary && eof {
                                self.stack.push(Thread {
                                    state: *out,
                                    ..thread
                                });
                            } else if boundary && self.program.zero_finish[*out] {
                                end_pending = end_pending.min(thread.start);
                            }
                        }
                        State::Consume(test, out) => {
                            if boundary {
                                if self.program.can_finish[thread.state] {
                                    future = future.min(thread.start);
                                }
                            } else if test.matches(text[self.position]) {
                                self.next.push(Thread {
                                    state: *out,
                                    ..thread
                                });
                            }
                        }
                        State::Accept => {
                            if (!self.ignore_empty || thread.start != self.position)
                                && self.best.is_none_or(|(start, end)| {
                                    thread.start < start
                                        || (thread.start == start && self.position > end)
                                })
                            {
                                self.best = Some((thread.start, self.position));
                            }
                        }
                        State::Reject => {}
                    }
                }
            }
            if boundary {
                self.current = seeds;
                let pending = !eof
                    && self.best.is_none_or(|(start, end)| {
                        future <= start
                            || end_pending < start
                            || (end_pending == start && end < text.len())
                    });
                return Scan {
                    found: self.best,
                    pending,
                };
            }
            seeds.clear();
            self.current = std::mem::take(&mut self.next);
            self.next = seeds;
            self.position += 1;
            if self.best.is_some() && self.current.is_empty() {
                return Scan {
                    found: self.best,
                    pending: false,
                };
            }
        }
    }
}
