use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rate {
    window: bool,
    count: i64,
    millis: i64,
}

impl Rate {
    fn parse(text: &str) -> Option<Self> {
        match text.split_once('/') {
            Some((count, millis)) => Some(Self {
                window: true,
                count: count.trim().parse().ok()?,
                millis: millis.trim().parse().ok()?,
            }),
            None => Some(Self {
                window: false,
                count: 1,
                millis: text.trim().parse().ok()?,
            }),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Record {
    window: bool,
    started: Instant,
    frequency: i64,
}

#[derive(Default)]
pub struct RateLimiter {
    records: Mutex<HashMap<String, Record>>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn acquire<'a>(&'a self, key: &'a str, rate: Option<&str>) -> Option<RateGuard<'a>> {
        let rate = rate
            .map(str::trim)
            .filter(|text| !text.is_empty() && *text != "0")?;
        let rate = Rate::parse(rate)?;
        loop {
            let wait = self.reserve(key, rate);
            if wait.is_zero() {
                return Some(RateGuard {
                    limiter: self,
                    key,
                    release: !rate.window,
                });
            }
            tokio::time::sleep(wait).await;
        }
    }

    fn reserve(&self, key: &str, rate: Rate) -> Duration {
        let mut records = self
            .records
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let now = Instant::now();
        let millis = Duration::from_millis(rate.millis.max(0) as u64);
        let Some(record) = records.get_mut(key) else {
            records.insert(
                key.to_string(),
                Record {
                    window: rate.window,
                    started: now,
                    frequency: 1,
                },
            );
            return Duration::ZERO;
        };
        if record.window != rate.window {
            *record = Record {
                window: rate.window,
                started: now,
                frequency: 1,
            };
            return Duration::ZERO;
        }

        let elapsed = now.saturating_duration_since(record.started);
        if rate.window {
            if elapsed >= millis {
                record.started = now;
                record.frequency = 1;
                return Duration::ZERO;
            }
            if record.frequency >= rate.count {
                return millis.saturating_sub(elapsed);
            }
            record.frequency += 1;
            return Duration::ZERO;
        }

        if record.frequency > 0 {
            return millis;
        }
        if elapsed >= millis {
            record.started = now;
            record.frequency = 1;
            return Duration::ZERO;
        }
        millis.saturating_sub(elapsed)
    }

    fn release(&self, key: &str) {
        let mut records = self
            .records
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(record) = records.get_mut(key)
            && record.frequency > 0
        {
            record.frequency -= 1;
        }
    }
}

pub struct RateGuard<'a> {
    limiter: &'a RateLimiter,
    key: &'a str,
    release: bool,
}

impl Drop for RateGuard<'_> {
    fn drop(&mut self) {
        if self.release {
            self.limiter.release(self.key);
        }
    }
}
