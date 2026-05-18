pub(super) mod dispatcher;
pub(super) mod executor;
pub(super) mod failure;
pub(super) mod openrouter_prompt;
pub(super) mod session_ids;
pub(super) mod types;
pub use dispatcher::BackgroundDispatcher;
pub use types::{
    AgentProfile, BackgroundResult, BackgroundTask, BackgroundTaskExecutor, BoxedBackgroundFuture,
    MergedResults,
};
