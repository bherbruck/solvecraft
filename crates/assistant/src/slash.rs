//! Quick actions: `/explain`, `/fix`, `/params`, `/check` are ready-made prompts that pull in
//! what is selected and what is failing.

/// What the composer's text means.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    /// A normal message.
    Message(String),
    /// A quick action expanded to the prompt to send; `shown` is what the person typed.
    Prompt { shown: String, prompt: String },
    /// `/new`: start a fresh conversation.
    NewChat,
    /// `/help`: list the quick actions.
    Help,
    /// An unknown `/word`.
    Unknown(String),
}

/// A quick action, for the autocomplete list and the empty state.
pub struct Quick {
    pub name: &'static str,
    pub summary: &'static str,
}

pub const QUICK: &[Quick] = &[
    Quick { name: "explain", summary: "Explain the design or the selection" },
    Quick { name: "fix", summary: "Find out why features fail and fix them" },
    Quick { name: "params", summary: "Review the parameters" },
    Quick { name: "check", summary: "Check the design without changing it" },
    Quick { name: "new", summary: "Start a new conversation" },
    Quick { name: "help", summary: "List the quick actions" },
];

/// What the prompt templates may refer to.
#[derive(Clone, Debug, Default)]
pub struct Situation {
    /// Features that have an error or warning: `name: message`.
    pub failing: Vec<String>,
    /// What is selected, in words (empty: nothing).
    pub selection: String,
}

/// Quick actions whose name starts with `prefix` (without the slash).
pub fn matching(prefix: &str) -> Vec<&'static Quick> {
    let p = prefix.to_ascii_lowercase();
    QUICK.iter().filter(|q| q.name.starts_with(&p)).collect()
}

/// Interpret the composer text.
pub fn parse(input: &str, s: &Situation) -> Input {
    let text = input.trim();
    let Some(rest) = text.strip_prefix('/') else { return Input::Message(text.to_string()) };
    let (word, extra) = match rest.split_once(char::is_whitespace) {
        Some((w, e)) => (w, e.trim()),
        None => (rest, ""),
    };
    let extra_line = if extra.is_empty() { String::new() } else { format!("\n\nMy additional request: {extra}") };
    let selection = if s.selection.is_empty() { String::new() } else { format!("\n\nThe current selection: {}.", s.selection) };
    let prompt = match word.to_ascii_lowercase().as_str() {
        "new" => return Input::NewChat,
        "help" | "?" => return Input::Help,
        "explain" => format!(
            "Explain this design to me: what each feature in the timeline does, how the parameters drive the shape, and \
             what the part is for if that is clear. Keep it short. If something is selected, concentrate on that.{selection}{extra_line}"
        ),
        "fix" if s.failing.is_empty() => format!(
            "Look for problems in this design: features with errors or warnings, over- or under-constrained sketches, \
             rolled back or suppressed features that matter. If you find none, say so. Fix what you can and tell me what you changed.{selection}{extra_line}"
        ),
        "fix" => format!(
            "These features have errors or warnings:\n{}\n\nFind the cause of each (inspect the feature with \
             `execute` `timeline.inspect`, and the sketches it uses), fix it if you can, and verify the result. Tell me what you changed.{selection}{extra_line}",
            s.failing.iter().map(|f| format!("- {f}")).collect::<Vec<_>>().join("\n")
        ),
        "params" => format!(
            "Review the design's user parameters: which are unused, which numbers are repeated in the features and should \
             be parameters, and whether the names are clear. Suggest improvements first; change the design only if I agree.{extra_line}"
        ),
        "check" => format!(
            "Check this design without changing it: measure the bodies and compare with what the features should produce, \
             look at thin walls and tiny features, and report errors, warnings and sketches that are not fully constrained. \
             Give me a short list of findings, most important first.{selection}{extra_line}"
        ),
        other => return Input::Unknown(other.to_string()),
    };
    Input::Prompt { shown: text.to_string(), prompt }
}

/// Suggested first messages for an empty conversation, based on the design's state.
pub fn suggestions(has_bodies: bool, failing: bool) -> Vec<&'static str> {
    match (has_bodies, failing) {
        (_, true) => vec!["/fix", "/explain", "/check"],
        (true, false) => vec!["/explain", "/check", "/params", "Add a 3 mm fillet to the outer edges"],
        (false, false) => {
            vec!["Make a 40 x 30 x 20 mm box with a 10 mm hole through the centre", "Make an L-bracket with two mounting holes", "What can you do?"]
        }
    }
}

/// The text of `/help`.
pub fn help_text() -> String {
    let mut s = String::from("Quick actions:\n");
    for q in QUICK {
        s.push_str(&format!("- `/{}` — {}\n", q.name, q.summary));
    }
    s.push_str("\nAnything else is sent as a message. Add text after a quick action to give it more to go on.");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_a_message() {
        assert_eq!(parse("  make a box  ", &Situation::default()), Input::Message("make a box".into()));
        assert_eq!(parse("a / b", &Situation::default()), Input::Message("a / b".into()));
    }

    #[test]
    fn quick_actions_expand_with_context() {
        let s = Situation { failing: vec!["Fillet 1: radius too large".into()], selection: "edge of Body1".into() };
        let Input::Prompt { shown, prompt } = parse("/fix keep the wall thick", &s) else { panic!("not a prompt") };
        assert_eq!(shown, "/fix keep the wall thick");
        assert!(prompt.contains("- Fillet 1: radius too large") && prompt.contains("edge of Body1") && prompt.contains("keep the wall thick"));
        let Input::Prompt { prompt, .. } = parse("/FIX", &Situation::default()) else { panic!("not a prompt") };
        assert!(prompt.contains("Look for problems"));
    }

    #[test]
    fn control_words() {
        assert_eq!(parse("/new", &Situation::default()), Input::NewChat);
        assert_eq!(parse("/help", &Situation::default()), Input::Help);
        assert_eq!(parse("/frobnicate x", &Situation::default()), Input::Unknown("frobnicate".into()));
        assert_eq!(parse("/", &Situation::default()), Input::Unknown(String::new()));
    }

    #[test]
    fn autocomplete() {
        assert_eq!(matching("e").iter().map(|q| q.name).collect::<Vec<_>>(), ["explain"]);
        assert_eq!(matching("").len(), QUICK.len());
        assert!(matching("zzz").is_empty());
        assert!(help_text().contains("/explain"));
    }

    #[test]
    fn suggestions_depend_on_the_design() {
        assert_eq!(suggestions(true, true)[0], "/fix");
        assert!(suggestions(false, false).iter().any(|s| s.contains("box")));
    }
}
