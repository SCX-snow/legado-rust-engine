use std::sync::Mutex;
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub struct TraceEvent<'a> {
    pub index: usize,
    pub rule: &'a str,
    pub input: &'a str,
    pub output: &'a str,
    pub elapsed: Duration,
}

pub trait TraceSink: Send + Sync {
    fn event(&self, event: TraceEvent<'_>);
}

#[derive(Debug, Default)]
pub struct TraceBuffer {
    events: Mutex<Vec<TraceRecord>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceRecord {
    pub index: usize,
    pub rule: String,
    pub input: String,
    pub output: String,
    pub elapsed: Duration,
}

impl TraceBuffer {
    pub fn records(&self) -> Vec<TraceRecord> {
        self.events.lock().expect("trace 锁被毒化").clone()
    }
}

impl TraceSink for TraceBuffer {
    fn event(&self, event: TraceEvent<'_>) {
        self.events
            .lock()
            .expect("trace 锁被毒化")
            .push(TraceRecord {
                index: event.index,
                rule: event.rule.to_string(),
                input: event.input.to_string(),
                output: event.output.to_string(),
                elapsed: event.elapsed,
            });
    }
}
