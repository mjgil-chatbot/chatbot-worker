pub(crate) fn terminal_cli_error_message(error: Option<&str>) -> String {
    error
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "CLI command failed".to_string())
}
