use std::cell::RefCell;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Default, serde::Serialize)]
struct Counts {
    path: String,
    bytes: usize,
    analyses: usize,
    lexical_scans: usize,
    streamed_dry_scans: usize,
    planning_invocations: usize,
    emissions: usize,
    strict_parses: usize,
    verification_parses: usize,
    rollback: usize,
    fallback: Option<String>,
    seconds: f64,
}

thread_local! {
    static COUNTS: RefCell<Option<Counts>> = const { RefCell::new(None) };
}

#[derive(Clone, Copy)]
pub(crate) enum Counter {
    Analysis,
    Scan,
    DryScan,
    Plan,
    Emit,
    Strict,
    Verify,
    Rollback,
}

pub(crate) fn count(counter: Counter) {
    COUNTS.with_borrow_mut(|counts| {
        if let Some(counts) = counts {
            let slot = match counter {
                Counter::Analysis => &mut counts.analyses,
                Counter::Scan => &mut counts.lexical_scans,
                Counter::DryScan => &mut counts.streamed_dry_scans,
                Counter::Plan => &mut counts.planning_invocations,
                Counter::Emit => &mut counts.emissions,
                Counter::Strict => &mut counts.strict_parses,
                Counter::Verify => &mut counts.verification_parses,
                Counter::Rollback => &mut counts.rollback,
            };
            *slot += 1;
        }
    });
}

pub(crate) struct Recording {
    destination: PathBuf,
    previous: Option<Counts>,
    started: Instant,
}

impl Recording {
    pub(crate) fn start(
        path: &Path,
        bytes: usize,
        initial: bool,
        strict: Option<bool>,
    ) -> Option<Self> {
        let destination = std::env::var_os("RYL_LAYOUT_STATS")?;
        let counts = Counts {
            path: path.to_string_lossy().into_owned(),
            bytes,
            analyses: usize::from(initial) * (1 + usize::from(strict == Some(false))),
            strict_parses: usize::from(initial),
            ..Counts::default()
        };
        Some(Self {
            destination: destination.into(),
            previous: COUNTS.with_borrow_mut(|slot| slot.replace(counts)),
            started: Instant::now(),
        })
    }

    pub(crate) fn fallback(reason: &super::Fallback) {
        COUNTS.with_borrow_mut(|counts| {
            if let Some(counts) = counts {
                counts.fallback = Some(match reason {
                    super::Fallback::UnsettledQuote(byte) => {
                        format!("unsettled-quote:{byte}")
                    }
                    other => format!("{other:?}"),
                });
            }
        });
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        let mut counts = COUNTS
            .with_borrow_mut(|slot| std::mem::replace(slot, self.previous.take()))
            .expect("a recording owns the active counter set");
        counts.seconds = self.started.elapsed().as_secs_f64();
        let write = || -> std::io::Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.destination)?;
            let mut line = serde_json::to_vec(&counts)
                .expect("layout statistics contain only JSON-native values");
            line.push(b'\n');
            file.write_all(&line)
        };
        if let Err(error) = write() {
            eprintln!("layout statistics: {error}");
        }
    }
}
