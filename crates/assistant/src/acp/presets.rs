//! Starting points for external agents. These are defaults the person can edit: the programs
//! and their ACP switches change between releases, so check one that fails to start against the
//! agent's own documentation.

use crate::settings::{AgentMode, AgentProfile};

fn p(name: &str, command: &str, args: &[&str], mode: AgentMode) -> AgentProfile {
    AgentProfile { name: name.into(), command: command.into(), args: args.iter().map(|s| s.to_string()).collect(), env: Vec::new(), mode }
}

/// The presets, in the order the settings list them.
pub fn presets() -> Vec<AgentProfile> {
    vec![
        p("OpenCode", "opencode", &["acp"], AgentMode::Acp),
        p("Claude Code (ACP adapter)", "npx", &["-y", "@zed-industries/claude-code-acp"], AgentMode::Acp),
        p("Gemini CLI", "gemini", &["--experimental-acp"], AgentMode::Acp),
        p("Codex (ACP adapter)", "codex-acp", &[], AgentMode::Acp),
        p("Antigravity (agy, one prompt per message)", "agy", &["-p", "{prompt}"], AgentMode::Print),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_usable_defaults() {
        let all = presets();
        assert!(all.len() >= 4);
        assert!(all.iter().all(|a| !a.name.is_empty() && !a.command.is_empty()));
        let agy = all.iter().find(|a| a.command == "agy").unwrap();
        assert_eq!(agy.mode, AgentMode::Print);
        assert!(agy.args.iter().any(|a| a == "{prompt}"));
    }
}
