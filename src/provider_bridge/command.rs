fn uses_agent_adapter(provider: ModelProvider) -> bool {
    matches!(
        provider,
        ModelProvider::Claude
            | ModelProvider::MiniMax
            | ModelProvider::Gemini
            | ModelProvider::Codex
            | ModelProvider::Kimi
            | ModelProvider::OpenCode
            | ModelProvider::Hermes
            | ModelProvider::Forge
            | ModelProvider::Claw
    )
}
fn agent_adapter_cli_program() -> String {
    crate::companion_binary::resolve_agent_adapter_cli()
        .to_string_lossy()
        .into_owned()
}
fn shell_join(args: &[String]) -> String {
    args.iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ")
}
fn shell_quote(arg: &str) -> String {
    if arg.is_empty() {
        return "\"\"".to_string();
    }

    if arg
        .bytes()
        .all(|byte| matches!(byte, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'/' | b'.' | b'_' | b':' | b',' | b'-' | b'=' ))
    {
        return arg.to_string();
    }

    format!(
        "\"{}\"",
        arg.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('$', "\\$")
    )
}
