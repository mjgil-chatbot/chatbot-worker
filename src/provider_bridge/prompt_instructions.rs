fn query_requests_mcp_server_inventory(query: &str) -> bool {
    let lower = query.to_ascii_lowercase();
    lower.contains("mcp") && lower.contains("server")
}
fn format_mcp_server_name_list(names: &[String]) -> String {
    names
        .iter()
        .map(|name| format!("`{name}`"))
        .collect::<Vec<_>>()
        .join(", ")
}
fn query_requests_build_or_test_execution(query: &str) -> bool {
    let lower = query.to_ascii_lowercase();
    let patterns = [
        "cargo build",
        "cargo test",
        "npm test",
        "pnpm test",
        "yarn test",
        "pytest",
        "trunk build",
        "run tests",
        "run the tests",
        "run test",
        "build this",
        "build the app",
        "build the project",
        "build the repo",
        "compile this",
        "compile the project",
    ];

    patterns.iter().any(|pattern| lower.contains(pattern))
}
fn build_test_tooling_system_prompt(query: &str) -> Option<String> {
    if !query_requests_build_or_test_execution(query) {
        return None;
    }

    Some(
        "For build or test work in this environment, use the `lightweight-builds` and `lightweight-tests` MCP servers rather than shell MCP tools such as `bash` or `mcp-server-commands`. Prefer `run_build` through `lightweight-builds` and `run_test` through `lightweight-tests`. If those MCP servers are unavailable, say so instead of falling back to shell-based build/test execution."
            .to_string(),
    )
}
