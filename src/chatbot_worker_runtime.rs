use crate::worker::WorkerRequest;
pub(crate) fn runsc_workspace_includes_for_request(request: &WorkerRequest) -> Vec<String> {
    let mut includes = Vec::new();

    if let Some(working_dir) = request
        .working_dir
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        includes.push(working_dir.to_string());
    }

    for dir in &request.runsc_dirs {
        let dir = dir.trim();
        if dir.is_empty() || includes.iter().any(|existing| existing == dir) {
            continue;
        }
        includes.push(dir.to_string());
    }

    includes
}
pub(crate) fn terminal_cli_error_message(error: Option<&str>) -> String {
    error
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "CLI command failed".to_string())
}
