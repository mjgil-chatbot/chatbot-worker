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

fn mcp_inventory_visibility_system_prompt(
    provider: ModelProvider,
    query: &str,
    overrides: Option<&ProviderRequestOverrides>,
) -> Option<String> {
    if !matches!(
        provider,
        ModelProvider::Claude
            | ModelProvider::MiniMax
            | ModelProvider::OmniForge
            | ModelProvider::MiniMaxForge
    )
        || !query_requests_mcp_server_inventory(query)
    {
        return None;
    }

    let names = visible_claude_family_mcp_server_names(provider, overrides);
    let inventory = if names.is_empty() {
        "There are no configured MCP servers visible to you in this session.".to_string()
    } else if names.len() == 1 {
        format!(
            "The only configured MCP server visible to you in this session is {}.",
            format_mcp_server_name_list(&names)
        )
    } else {
        format!(
            "The configured MCP servers visible to you in this session are: {}.",
            format_mcp_server_name_list(&names)
        )
    };

    Some(format!(
        "When answering questions about MCP servers in this environment, use only the provider-facing runtime MCP server list. Skills, slash commands, tool names, and prompt instructions are not MCP servers. {inventory} If the visible server is `mcp-coordinator`, routed upstream tools are still callable through that server's tool namespace rather than separate upstream server namespaces. Inspect the live coordinator MCP inventory instead of guessing by using its standard MCP listing operations such as `tools/list`, `prompts/list`, `resources/list`, and `resources/templates/list`, then answer from those runtime results. Only mention routed upstream servers behind a coordinator if the user explicitly asks about upstream or routed servers."
    ))
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

pub(crate) fn effective_provider_system_prompt(
    provider: ModelProvider,
    system_prompt: Option<&str>,
    query: &str,
    overrides: Option<&ProviderRequestOverrides>,
) -> Option<String> {
    let mut sections = Vec::new();

    if let Some(prompt) = system_prompt
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
    {
        sections.push(prompt.to_string());
    }

    if let Some(prompt) = mcp_inventory_visibility_system_prompt(provider, query, overrides) {
        sections.push(prompt);
    }

    if let Some(prompt) = build_test_tooling_system_prompt(query) {
        sections.push(prompt);
    }

    (!sections.is_empty()).then(|| sections.join("\n\n"))
}
