use super::executor::rewrite_runsc_workspace_paths;
use super::types::BackgroundTask;

pub(super) fn build_openrouter_prompt(
    task: &BackgroundTask,
    runsc_enabled: bool,
    working_dir: Option<&str>,
) -> String {
    let mut sections = Vec::new();

    if let Some(prompt) = task.system_prompt.as_deref() {
        if !prompt.trim().is_empty() {
            sections.push(format!(
                "System instructions:\n{}",
                rewrite_runsc_workspace_paths(prompt, runsc_enabled, working_dir)
            ));
        }
    }

    if task.prompt.contains('\n') {
        sections.push(format!(
            "User:\n{}",
            rewrite_runsc_workspace_paths(&task.prompt, runsc_enabled, working_dir).trim()
        ));
    } else {
        sections.push(format!(
            "User: {}",
            rewrite_runsc_workspace_paths(&task.prompt, runsc_enabled, working_dir).trim()
        ));
    }

    sections.join("\n\n")
}
