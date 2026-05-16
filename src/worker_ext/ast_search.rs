use crate::cgroup::wrap_command;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    Rust,
    Python,
    TypeScript,
    JavaScript,
    Go,
}

impl Language {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Rust => "rust",
            Self::Python => "python",
            Self::TypeScript => "typescript",
            Self::JavaScript => "javascript",
            Self::Go => "go",
        }
    }
}

#[derive(Clone, Debug)]
struct SearchBinaries {
    sg: String,
    rg: String,
}

impl Default for SearchBinaries {
    fn default() -> Self {
        Self {
            sg: "sg".to_string(),
            rg: "rg".to_string(),
        }
    }
}

pub struct AstSearchTool {
    pub pattern: String,
    pub language: Language,
    pub paths: Vec<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AstSearchBackend {
    AstGrep,
    Ripgrep,
    Unavailable,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AstSearchResult {
    pub file: String,
    pub line: usize,
    pub matched_node: String,
    pub context: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AstSearchResponse {
    pub backend: AstSearchBackend,
    pub matches: Vec<AstSearchResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
}

impl AstSearchTool {
    pub fn new(pattern: String, language: Language, paths: Vec<PathBuf>) -> Self {
        Self {
            pattern,
            language,
            paths,
        }
    }

    pub fn execute(&self) -> Result<AstSearchResponse, String> {
        self.execute_with_binaries(&SearchBinaries::default())
    }

    fn execute_with_binaries(
        &self,
        binaries: &SearchBinaries,
    ) -> Result<AstSearchResponse, String> {
        if command_succeeds(&binaries.sg, ["--version"]) {
            return self.execute_ast_grep(binaries);
        }

        self.execute_fallback(
            binaries,
            format!("ast-grep binary `{}` is unavailable", binaries.sg),
        )
    }

    fn execute_ast_grep(&self, binaries: &SearchBinaries) -> Result<AstSearchResponse, String> {
        let mut cmd = Command::new(&binaries.sg);
        cmd.arg("run")
            .arg("-p")
            .arg(&self.pattern)
            .arg("-l")
            .arg(self.language.as_str())
            .arg("--json=compact");

        for path in &self.paths {
            cmd.arg(path);
        }

        wrap_command(&mut cmd);
        let output = cmd
            .output()
            .map_err(|err| format!("Failed to run ast-grep: {err}"))?;

        if !output.status.success() && output.stdout.is_empty() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if output.status.code() == Some(1) {
                return Ok(AstSearchResponse {
                    backend: AstSearchBackend::AstGrep,
                    matches: Vec::new(),
                    degraded_reason: None,
                });
            }
            return Err(if stderr.is_empty() {
                "ast-grep returned a non-success exit status".to_string()
            } else {
                format!("ast-grep failed: {stderr}")
            });
        }

        let matches = Self::parse_output(&String::from_utf8_lossy(&output.stdout));
        Ok(AstSearchResponse {
            backend: AstSearchBackend::AstGrep,
            matches,
            degraded_reason: None,
        })
    }

    fn execute_fallback(
        &self,
        binaries: &SearchBinaries,
        degraded_reason: String,
    ) -> Result<AstSearchResponse, String> {
        let mut cmd = Command::new(&binaries.rg);
        cmd.arg("--with-filename")
            .arg("--line-number")
            .arg("--no-heading")
            .arg("--color")
            .arg("never")
            .arg("-F")
            .arg(&self.pattern);

        for path in &self.paths {
            cmd.arg(path);
        }

        wrap_command(&mut cmd);
        let output = match cmd.output() {
            Ok(output) => output,
            Err(err) => {
                return Ok(AstSearchResponse {
                    backend: AstSearchBackend::Unavailable,
                    matches: Vec::new(),
                    degraded_reason: Some(format!(
                        "{degraded_reason}; ripgrep fallback failed: {err}"
                    )),
                })
            }
        };

        if !output.status.success() && output.status.code() != Some(1) && output.stdout.is_empty() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Ok(AstSearchResponse {
                backend: AstSearchBackend::Unavailable,
                matches: Vec::new(),
                degraded_reason: Some(if stderr.is_empty() {
                    degraded_reason
                } else {
                    format!("{degraded_reason}; ripgrep fallback failed: {stderr}")
                }),
            });
        }

        Ok(AstSearchResponse {
            backend: AstSearchBackend::Ripgrep,
            matches: Self::parse_ripgrep_output(&String::from_utf8_lossy(&output.stdout)),
            degraded_reason: Some(degraded_reason),
        })
    }

    pub fn parse_output(stdout_str: &str) -> Vec<AstSearchResult> {
        let trimmed = stdout_str.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }
        if trimmed.starts_with('[') {
            let json_value: serde_json::Value = match serde_json::from_str(trimmed) {
                Ok(value) => value,
                Err(_) => return Vec::new(),
            };
            return json_value
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Self::parse_match)
                .collect();
        }

        trimmed
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter_map(|value| Self::parse_match(&value))
            .collect()
    }

    fn parse_match(value: &serde_json::Value) -> Option<AstSearchResult> {
        let file = value
            .get("file")
            .and_then(|item| item.as_str())?
            .to_string();
        let line = value
            .get("range")
            .and_then(|item| item.get("start"))
            .and_then(|item| item.get("line"))
            .and_then(|item| item.as_u64())
            .unwrap_or(0) as usize
            + 1;
        let matched_node = value
            .get("text")
            .and_then(|item| item.as_str())
            .unwrap_or_default()
            .to_string();
        let context = value
            .get("lines")
            .and_then(|item| item.as_str())
            .unwrap_or(&matched_node)
            .to_string();

        Some(AstSearchResult {
            file,
            line,
            matched_node,
            context,
        })
    }

    fn parse_ripgrep_output(stdout_str: &str) -> Vec<AstSearchResult> {
        stdout_str
            .lines()
            .filter_map(|line| {
                let mut parts = line.splitn(3, ':');
                let file = parts.next()?.to_string();
                let line_number = parts.next()?.parse::<usize>().ok()?;
                let matched_text = parts.next().unwrap_or_default().to_string();
                Some(AstSearchResult {
                    file,
                    line: line_number,
                    matched_node: matched_text.clone(),
                    context: matched_text,
                })
            })
            .collect()
    }
}

fn command_succeeds<const N: usize>(binary: &str, args: [&str; N]) -> bool {
    let mut cmd = Command::new(binary);
    cmd.args(args);
    wrap_command(&mut cmd);
    cmd.output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_parse_empty_array() {
        let results = AstSearchTool::parse_output("[]");
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_malformed_json() {
        let results = AstSearchTool::parse_output("not json at all");
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_multiple_results() {
        let json_str = r#"[
            {"file":"a.rs","range":{"start":{"line":0,"column":0},"end":{"line":1,"column":1}},"text":"fn a(){}","lines":"fn a(){}\n"},
            {"file":"b.rs","range":{"start":{"line":5,"column":0},"end":{"line":6,"column":1}},"text":"fn b(){}","lines":"fn b(){}\n"}
        ]"#;
        let results = AstSearchTool::parse_output(json_str);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].file, "a.rs");
        assert_eq!(results[0].line, 1);
        assert_eq!(results[1].file, "b.rs");
        assert_eq!(results[1].line, 6);
    }

    #[test]
    fn test_parse_stream_output() {
        let json_str = concat!(
            "{\"file\":\"src/a.rs\",\"range\":{\"start\":{\"line\":2}},\"text\":\"let a = 1;\",\"lines\":\"let a = 1;\\n\"}\n",
            "{\"file\":\"src/b.rs\",\"range\":{\"start\":{\"line\":7}},\"text\":\"let b = 2;\",\"lines\":\"let b = 2;\\n\"}\n"
        );
        let results = AstSearchTool::parse_output(json_str);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].file, "src/a.rs");
        assert_eq!(results[0].line, 3);
        assert_eq!(results[1].file, "src/b.rs");
        assert_eq!(results[1].line, 8);
    }

    #[test]
    fn test_parse_missing_fields() {
        let json_str = r#"[{"file":"x.rs"}]"#;
        let results = AstSearchTool::parse_output(json_str);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file, "x.rs");
        assert_eq!(results[0].line, 1);
        assert_eq!(results[0].matched_node, "");
        assert_eq!(results[0].context, "");
    }

    #[test]
    fn test_parse_ripgrep_output() {
        let results = AstSearchTool::parse_ripgrep_output(
            "src/main.rs:11:fn main() {}\nsrc/lib.rs:4:fn helper() {}\n",
        );
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].file, "src/main.rs");
        assert_eq!(results[0].line, 11);
        assert_eq!(results[0].matched_node, "fn main() {}");
    }

    #[test]
    fn test_execute_degrades_when_ast_grep_and_ripgrep_are_missing() {
        let tool = AstSearchTool::new(
            "fn main() {}".to_string(),
            Language::Rust,
            vec![PathBuf::from("src")],
        );

        let response = tool
            .execute_with_binaries(&SearchBinaries {
                sg: "__missing_sg__".to_string(),
                rg: "__missing_rg__".to_string(),
            })
            .unwrap();

        assert_eq!(response.backend, AstSearchBackend::Unavailable);
        assert!(response.matches.is_empty());
        assert!(response
            .degraded_reason
            .as_deref()
            .unwrap_or_default()
            .contains("ast-grep binary `__missing_sg__` is unavailable"));
    }

    #[test]
    fn test_execute_uses_ripgrep_fallback_when_ast_grep_missing() {
        let temp = tempdir().unwrap();
        let file = temp.path().join("sample.rs");
        std::fs::write(&file, "fn main() {}\nfn helper() {}\n").unwrap();
        let tool = AstSearchTool::new("fn main() {}".to_string(), Language::Rust, vec![file]);

        let response = tool
            .execute_with_binaries(&SearchBinaries {
                sg: "__missing_sg__".to_string(),
                rg: "rg".to_string(),
            })
            .unwrap();

        assert_eq!(response.backend, AstSearchBackend::Ripgrep);
        assert_eq!(response.matches.len(), 1);
        assert!(response
            .degraded_reason
            .as_deref()
            .unwrap_or_default()
            .contains("ast-grep binary `__missing_sg__` is unavailable"));
    }
}
