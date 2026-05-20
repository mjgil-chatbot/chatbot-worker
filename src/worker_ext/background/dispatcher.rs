use super::types::{
    BackgroundResult, BackgroundTask, BackgroundTaskExecutor, MergedResults, DEFAULT_MAX_CONCURRENT,
};
use std::sync::Arc;
use tokio::sync::Mutex;

use super::executor::provider_task_executor;
use std::sync::atomic::AtomicBool;

pub struct BackgroundDispatcher {
    pub max_concurrent: usize,
    pub tasks: Vec<BackgroundTask>,
    pub results: Arc<Mutex<Vec<BackgroundResult>>>,
}

impl BackgroundDispatcher {
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            max_concurrent: if max_concurrent == 0 {
                DEFAULT_MAX_CONCURRENT
            } else {
                max_concurrent
            },
            tasks: Vec::new(),
            results: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn add_task(&mut self, task: BackgroundTask) {
        self.tasks.push(task);
    }

    /// Execute queued tasks with a bounded level of concurrency.
    pub async fn run_all(&mut self) {
        self.run_all_with_executor(provider_task_executor(Arc::new(AtomicBool::new(false))))
            .await;
    }

    /// Execute queued tasks with bounded concurrency via a tokio semaphore.
    /// Results are collected into `self.results`.
    pub async fn run_all_with_executor(&mut self, executor: Arc<BackgroundTaskExecutor>) {
        let tasks = std::mem::take(&mut self.tasks);
        let results = self.results.clone();

        let semaphore = Arc::new(tokio::sync::Semaphore::new(self.max_concurrent));
        let mut handles = Vec::new();

        for task in tasks {
            let sem = semaphore.clone();
            let results_clone = results.clone();
            let executor = executor.clone();

            let handle = tokio::spawn(async move {
                let _permit = match sem.acquire_owned().await {
                    Ok(p) => p,
                    Err(_) => return,
                };

                let result = executor(task).await;
                let mut lock = results_clone.lock().await;
                lock.push(result);
            });
            handles.push(handle);
        }

        for handle in handles {
            let _ = handle.await;
        }
    }

    /// Drain collected results and return a MergedResults summary.
    pub async fn take_merged(&self) -> MergedResults {
        let results = {
            let mut lock = self.results.lock().await;
            std::mem::take(&mut *lock)
        };
        MergedResults::from_results(results)
    }
}
