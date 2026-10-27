use super::{Config, input::Line};
use crate::rules::quoted_strings::protection::Preserved;

pub(crate) struct Runs<'a> {
    config: Config,
    protected: &'a Preserved,
    run: Vec<usize>,
    seen: bool,
    flushed: bool,
}

impl<'a> Runs<'a> {
    pub(crate) const fn new(config: Config, protected: &'a Preserved) -> Self {
        Self {
            config,
            protected,
            run: Vec::new(),
            seen: false,
            flushed: false,
        }
    }
    fn prepare(&mut self) {
        if self.flushed {
            self.run.clear();
            self.flushed = false;
        }
    }
    fn middle(&self) -> i64 {
        if self.seen {
            self.config.max()
        } else {
            self.config.max_start()
        }
    }
    fn select(&mut self, max: i64) -> &[usize] {
        self.flushed = true;
        let keep = usize::try_from(max).unwrap_or(0).min(self.run.len());
        &self.run[keep..]
    }
    pub(crate) fn marker(&mut self) -> &[usize] {
        self.prepare();
        let max = self.middle();
        self.seen = true;
        self.select(max)
    }
    pub(crate) fn line(&mut self, index: usize, line: Line) -> &[usize] {
        self.prepare();
        if line.blank && !self.protected.line(line.origin) {
            if line.terminated {
                self.run.push(index);
            }
            return &[];
        }
        let max = self.middle();
        self.seen |= !line.empty;
        self.select(max)
    }
    pub(crate) fn finish(&mut self, marker_follows: bool) -> &[usize] {
        self.prepare();
        let max = if marker_follows {
            self.middle()
        } else {
            self.config.max_end()
        };
        self.select(max)
    }
}

pub(super) fn clears_stream(
    initial: &super::input::Stream<'_>,
    config: &Config,
) -> bool {
    initial.starts_lf()
        && config.max_start() == 0
        && config.max_end() == 0
        && initial.only_lf()
}
