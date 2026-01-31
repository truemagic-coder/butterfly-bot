use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::interfaces::scheduler::ScheduledJob;

pub struct Scheduler {
    jobs: Vec<Arc<dyn ScheduledJob>>,
    handles: Vec<JoinHandle<()>>,
    stop: Option<watch::Sender<bool>>,
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            jobs: Vec::new(),
            handles: Vec::new(),
            stop: None,
        }
    }

    pub fn register_job(&mut self, job: Arc<dyn ScheduledJob>) {
        self.jobs.push(job);
    }

    pub fn is_running(&self) -> bool {
        self.stop.is_some()
    }

    pub fn start(&mut self) {
        if self.stop.is_some() {
            return;
        }
        let (tx, rx) = watch::channel(false);
        self.stop = Some(tx);

        for job in &self.jobs {
            let job = Arc::clone(job);
            let mut tick = tokio::time::interval(job.interval());
            let mut rx = rx.clone();
            let handle = tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = tick.tick() => {
                            let _ = job.run().await;
                        }
                        _ = rx.changed() => {
                            if *rx.borrow() {
                                break;
                            }
                        }
                    }
                }
            });
            self.handles.push(handle);
        }
    }

    pub async fn stop(&mut self) {
        if let Some(tx) = self.stop.take() {
            let _ = tx.send(true);
        }
        let handles = std::mem::take(&mut self.handles);
        for handle in handles {
            let _ = handle.await;
        }
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

pub fn seconds(n: u64) -> Duration {
    Duration::from_secs(n)
}
