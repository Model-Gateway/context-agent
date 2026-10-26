//! The `ask_user` tool: the agent asks the user one to four questions in the
//! desktop app, each with optional choices, and waits for the answer. Like
//! the browser tool it does nothing itself — it sends a `question/request`
//! notification and waits for the app's `question/respond`. There is no
//! timeout (the user may be away); `task/cancel` and `task/finished` drop a
//! session's waiting questions. Only `ctx serve` registers it, and only for
//! lead runs: the TUI and headless runs have no one to ask.

use super::*;
use async_trait::async_trait;
use ctx_security::{Action, ActionKind};
use ctx_tools::{Tool, ToolContext, ToolOutput};
use std::sync::atomic::AtomicU64;

/// Most questions one call may ask.
const MAX_QUESTIONS: usize = 4;
/// Most choices one question may offer (the app always adds free text).
const MAX_OPTIONS: usize = 6;

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Question {
    question: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    header: Option<String>,
    #[serde(default)]
    options: Vec<QuestionOption>,
    #[serde(default)]
    multi_select: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct QuestionOption {
    label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

#[derive(Deserialize)]
struct AskArgs {
    questions: Vec<Question>,
}

/// One question's answer: the chosen option labels and/or the user's own text.
#[derive(Debug, Clone, Default, Deserialize)]
pub(super) struct QuestionAnswer {
    #[serde(default)]
    pub(super) selected: Vec<String>,
    #[serde(default)]
    pub(super) other: Option<String>,
}

/// What the app sends back (`question/respond`): an answer per question in
/// order, and/or a free-form message; neither means the user skipped.
#[derive(Debug, Clone, Default)]
pub(super) struct QuestionReply {
    pub(super) answers: Option<Vec<QuestionAnswer>>,
    pub(super) message: Option<String>,
}

/// Questions waiting for the app, keyed by `approval_key(session, request_id)`
/// so a session's questions can be dropped together when its run ends.
#[derive(Default)]
pub(super) struct QuestionState {
    pending: Mutex<HashMap<String, oneshot::Sender<QuestionReply>>>,
    next: AtomicU64,
}

impl QuestionState {
    /// A fresh request id for `session_id`, filed before the notification
    /// goes out so an answer can never arrive unannounced.
    pub(super) fn register(&self, session_id: &str) -> (String, oneshot::Receiver<QuestionReply>) {
        let id = format!("question_{}", self.next.fetch_add(1, Ordering::SeqCst));
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(approval_key(session_id, &id), tx);
        (id, rx)
    }

    /// The app's answer for `key`; `false` when nothing waits for it.
    pub(super) fn respond(&self, key: &str, reply: QuestionReply) -> bool {
        match self.pending.lock().remove(key) {
            Some(tx) => {
                let _ = tx.send(reply);
                true
            }
            None => false,
        }
    }

    fn forget(&self, key: &str) {
        self.pending.lock().remove(key);
    }

    /// Drop a session's waiting questions (cancel, finish).
    pub(super) fn clear_session(&self, session_id: &str) {
        let prefix = approval_key(session_id, "");
        self.pending.lock().retain(|k, _| !k.starts_with(&prefix));
    }
}

/// One run's way to ask the user.
pub(super) struct AskUserTool {
    pub(super) server: Arc<Server>,
    pub(super) state: Arc<QuestionState>,
    pub(super) project_id: String,
    pub(super) session_id: String,
}

#[async_trait]
impl Tool for AskUserTool {
    fn name(&self) -> &str {
        "ask_user"
    }

    fn description(&self) -> &str {
        "Ask the user questions and wait for the answers. Use it when the request is ambiguous in a way that changes what you build, when there are several reasonable approaches and the choice is the user's (a library, a design, a behaviour), or when you need a fact only the user knows. Do not use it for things you can find out from the repository or decide with a sensible default, and do not ask for permission to run tools (the approval system does that). Ask 1-4 short, specific questions at once; give each 2-4 concrete `options` (label: a few words; description: what it means or implies) when the answer is a choice, and put your recommended option first with \"(Recommended)\" after its label. The user can always type their own answer instead of choosing. Set `multi_select` when several options may apply. `header` is a very short tag shown above the question (max 12 characters). The user may also skip the questions: then decide yourself and say what you assumed."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "questions": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": MAX_QUESTIONS,
                    "items": {
                        "type": "object",
                        "properties": {
                            "question": {"type": "string", "description": "The question, ending with a question mark"},
                            "header": {"type": "string", "maxLength": 12, "description": "Very short label for the question, e.g. \"Database\""},
                            "options": {
                                "type": "array",
                                "maxItems": MAX_OPTIONS,
                                "description": "The choices (omit for an open question); the user can always answer in their own words",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "label": {"type": "string", "description": "The choice, 1-5 words"},
                                        "description": {"type": "string", "description": "What choosing it means"}
                                    },
                                    "required": ["label"],
                                    "additionalProperties": false
                                }
                            },
                            "multi_select": {"type": "boolean", "description": "Allow choosing several options"}
                        },
                        "required": ["question"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["questions"],
            "additionalProperties": false
        })
    }

    fn action(&self, args: &Value, _ctx: &ToolContext) -> Result<Action> {
        let a = parse_args(args)?;
        let summary = match a.questions.as_slice() {
            [one] => format!("ask: {}", one.question.trim()),
            many => format!("ask {} questions", many.len()),
        };
        let details = a.questions.iter().map(|q| q.question.trim()).collect::<Vec<_>>().join("\n");
        Ok(Action { tool: "ask_user".into(), kind: ActionKind::AskUser, summary, details })
    }

    async fn invoke(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let a = parse_args(&args)?;
        let (request_id, reply) = self.state.register(&self.session_id);
        self.server.notify("question/request", json!({"project_id": self.project_id, "session_id": self.session_id, "request_id": request_id, "questions": a.questions}));
        let reply = tokio::select! {
            r = reply => match r {
                Ok(reply) => reply,
                Err(_) => return Err(CtxError::Cancelled),
            },
            _ = ctx.cancel.cancelled() => {
                self.state.forget(&approval_key(&self.session_id, &request_id));
                return Err(CtxError::Cancelled);
            }
        };
        let (content, summary) = describe_reply(&a.questions, &reply);
        Ok(ToolOutput::ok(content, summary))
    }
}

fn parse_args(args: &Value) -> Result<AskArgs> {
    let invalid = |message: String| CtxError::InvalidArgument(format!("ask_user: {message}"));
    let a: AskArgs = serde_json::from_value(args.clone()).map_err(|e| invalid(e.to_string()))?;
    if a.questions.is_empty() || a.questions.len() > MAX_QUESTIONS {
        return Err(invalid(format!("ask between 1 and {MAX_QUESTIONS} questions (got {})", a.questions.len())));
    }
    for (i, q) in a.questions.iter().enumerate() {
        if q.question.trim().is_empty() {
            return Err(invalid(format!("question {} is empty", i + 1)));
        }
        if q.options.len() > MAX_OPTIONS {
            return Err(invalid(format!("question {} has {} options (at most {MAX_OPTIONS})", i + 1, q.options.len())));
        }
        let mut seen = HashSet::new();
        for o in &q.options {
            let label = o.label.trim();
            if label.is_empty() {
                return Err(invalid(format!("question {} has an option without a label", i + 1)));
            }
            if !seen.insert(label.to_lowercase()) {
                return Err(invalid(format!("question {} lists `{label}` twice", i + 1)));
            }
        }
    }
    Ok(a)
}

/// The text the model reads, and the tool row's one-line summary.
fn describe_reply(questions: &[Question], reply: &QuestionReply) -> (String, String) {
    let message = reply.message.as_deref().map(str::trim).filter(|m| !m.is_empty());
    let answers = reply.answers.as_deref().unwrap_or_default();
    let answered = answers.iter().filter(|a| !a.selected.is_empty() || a.other.as_deref().is_some_and(|o| !o.trim().is_empty())).count();
    if answered == 0 {
        return match message {
            Some(message) => (format!("The user did not pick answers and wrote instead:\n{message}"), "the user replied".to_string()),
            None => (
                "The user skipped the questions without answering. Continue with your best judgment and state the assumptions you made.".to_string(),
                "the user skipped the questions".to_string(),
            ),
        };
    }
    let mut out = String::from("The user answered:\n");
    for (i, q) in questions.iter().enumerate() {
        let answer = answers.get(i).cloned().unwrap_or_default();
        let mut parts: Vec<String> = answer.selected.iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        if let Some(other) = answer.other.as_deref().map(str::trim).filter(|o| !o.is_empty()) {
            parts.push(if parts.is_empty() { other.to_string() } else { format!("(their own words) {other}") });
        }
        let text = if parts.is_empty() { "(no answer)".to_string() } else { parts.join("; ") };
        out.push_str(&format!("\n{}. {}\n   → {}\n", i + 1, q.question.trim(), text));
    }
    if let Some(message) = message {
        out.push_str(&format!("\nThey added:\n{message}\n"));
    }
    let summary = if questions.len() == 1 { "the user answered".to_string() } else { format!("the user answered {answered} of {} questions", questions.len()) };
    (out, summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn questions() -> Vec<Question> {
        parse_args(&json!({"questions": [
            {"question": "Which database?", "header": "Database", "options": [{"label": "SQLite (Recommended)"}, {"label": "Postgres"}]},
            {"question": "Which platforms?", "options": [{"label": "macOS"}, {"label": "Linux"}], "multi_select": true}
        ]}))
        .unwrap()
        .questions
    }

    #[test]
    fn arguments_are_checked() {
        assert!(parse_args(&json!({"questions": []})).is_err());
        assert!(parse_args(&json!({"questions": [{"question": "  "}]})).is_err());
        assert!(parse_args(&json!({"questions": [{"question": "A?", "options": [{"label": "x"}, {"label": "X"}]}]})).is_err());
        assert!(parse_args(&json!({"questions": [{"question": "Open question?"}]})).is_ok());
        assert_eq!(questions().len(), 2);
    }

    #[test]
    fn replies_read_as_answers_or_a_skip() {
        let qs = questions();
        let reply = QuestionReply {
            answers: Some(vec![QuestionAnswer { selected: vec!["Postgres".into()], other: None }, QuestionAnswer { selected: vec!["macOS".into()], other: Some("and Windows later".into()) }]),
            message: None,
        };
        let (text, summary) = describe_reply(&qs, &reply);
        assert!(text.contains("1. Which database?\n   → Postgres"), "{text}");
        assert!(text.contains("→ macOS; (their own words) and Windows later"), "{text}");
        assert_eq!(summary, "the user answered 2 of 2 questions");

        let (text, summary) = describe_reply(&qs, &QuestionReply::default());
        assert!(text.starts_with("The user skipped"), "{text}");
        assert_eq!(summary, "the user skipped the questions");

        let (text, _) = describe_reply(&qs, &QuestionReply { answers: None, message: Some("Use whatever is simplest".into()) });
        assert!(text.contains("wrote instead:\nUse whatever is simplest"), "{text}");
    }

    #[tokio::test]
    async fn answers_reach_their_question_and_cleared_sessions_cancel() {
        let state = QuestionState::default();
        let (id, rx) = state.register("s1");
        assert!(state.respond(&approval_key("s1", &id), QuestionReply { answers: None, message: Some("hi".into()) }));
        assert_eq!(rx.await.unwrap().message.as_deref(), Some("hi"));
        assert!(!state.respond(&approval_key("s1", &id), QuestionReply::default()), "answered once");
        let (_id2, rx2) = state.register("s1");
        let (_id3, mut rx3) = state.register("s2");
        state.clear_session("s1");
        assert!(rx2.await.is_err(), "a cleared session's question is dropped");
        assert!(rx3.try_recv().is_err(), "the other session still waits");
    }
}
