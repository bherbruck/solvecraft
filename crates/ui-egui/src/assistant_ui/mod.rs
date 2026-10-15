//! The assistant sidebar's logic: the conversation, the worker turn, and the gate every tool
//! call of the assistant passes before it reaches the design.
//!
//! The assistant is another client of the control channel that lives in this process. Its tool
//! loop (`solidcraft_assistant::tool_loop`) runs on a worker thread and sends control requests
//! into an inlet; [`next_request`] hands them to `SolveApp::drain_control` on the UI thread, after
//! the policy of `solidcraft_assistant::policy` has refused what an assistant may not do and
//! held what needs the person's yes. The design is only ever touched on this thread.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use serde_json::{Value, json};
use solidcraft_assistant::acp::session::{AgentSession, PermissionRequest};
use solidcraft_assistant::context;
use solidcraft_assistant::model::{Message, Usage};
use solidcraft_assistant::net::Cancel;
use solidcraft_assistant::policy::{Verdict, classify};
use solidcraft_assistant::provider::Codec;
use solidcraft_assistant::provider::anthropic::Anthropic;
use solidcraft_assistant::provider::openai::OpenAi;
use solidcraft_assistant::runtime::{self, AgentJob, Bridge, Job, TurnHandle, WorkerMsg, agent_roots, merge_roots};
use solidcraft_assistant::settings::{BackendKind, Settings};
use solidcraft_assistant::slash::{self, Input, Situation};
use solidcraft_assistant::tool_loop::{Event, Finish, MAX_STEPS, ToolHost, call_title};

use web_time::Instant;

use crate::SolveApp;
use crate::control::ControlRequest;

pub mod panel;

#[cfg(test)]
mod tests;

/// Longest a tool call may take before the turn gives up on it (a batch may be much longer).
const CALL_TIMEOUT: Duration = Duration::from_secs(600);
const BATCH_TIMEOUT: Duration = Duration::from_secs(3600);

/// Longest message a person may send.
pub const MAX_MESSAGE: usize = 32 * 1024;

/// Reply tokens asked of the model.
const MAX_TOKENS: u32 = 16_384;

/// Most items kept for display in one conversation.
const MAX_ITEMS: usize = 2000;

/// What the app needs to start the listener external agents' MCP servers connect to.
pub struct EndpointSpec {
    /// Where accepted requests go (the assistant's inlet, behind the gate).
    pub inlet: Sender<ControlRequest>,
    /// The turn requests are tagged with.
    pub turn: Arc<AtomicU64>,
    pub ctx: egui::Context,
}

/// A running agent endpoint: a loopback port with its own random token.
pub struct Endpoint {
    pub port: u16,
    pub token: String,
    stop: Option<Box<dyn FnOnce() + Send>>,
}

impl Endpoint {
    pub fn new(port: u16, token: String, stop: impl FnOnce() + Send + 'static) -> Endpoint {
        Endpoint { port, token, stop: Some(Box::new(stop)) }
    }
}

impl Drop for Endpoint {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop();
        }
    }
}

/// Starts the agent endpoint (the desktop app provides it).
pub type StartEndpoint = Arc<dyn Fn(EndpointSpec) -> Result<Endpoint, String> + Send + Sync>;

/// A question from an external agent, waiting for the person.
pub struct Ask {
    pub request: PermissionRequest,
    reply: Sender<Option<String>>,
}

/// One line of the conversation as the person sees it.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    User {
        text: String,
        picture: bool,
    },
    Assistant {
        text: String,
        thinking: String,
    },
    Tool(ToolCard),
    Notice {
        text: String,
        error: bool,
    },
    /// A turn that changed the design ends here; `checkpoint` goes back to before it.
    TurnEnd {
        checkpoint: Option<u64>,
        revision: u64,
        changed: bool,
    },
}

/// A tool call and what it returned.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolCard {
    pub id: String,
    pub name: String,
    pub title: String,
    pub input: Value,
    pub result: Option<ToolOutcome>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolOutcome {
    pub is_error: bool,
    pub text: String,
    pub images: usize,
}

/// A request held for the person's yes.
pub struct Confirm {
    pub lines: Vec<String>,
    req: ControlRequest,
}

struct Active {
    id: u64,
    handle: TurnHandle,
    checkpoint: u64,
    revision: u64,
    /// Which design tab the turn started in.
    tab: (usize, usize),
    started: Instant,
    /// The turn runs through an external agent.
    agent: bool,
    /// The conversation before this turn, for a turn that has to be given up on.
    before: Vec<Message>,
    /// When the turn was told to stop.
    stopping_since: Option<Instant>,
}

/// How long a stopped turn may take to wind down before the sidebar lets go of it. A server that
/// stops sending would otherwise keep the window in its "working" state until a timeout.
const STOP_GRACE: Duration = Duration::from_secs(4);

/// The sidebar's state.
pub struct State {
    pub settings: Settings,
    settings_loaded: bool,
    pub items: Vec<Item>,
    history: Vec<Message>,
    active: Option<Active>,
    /// The turn the gate accepts requests for (0: none). Shared with whatever forwards requests.
    current_turn: Arc<AtomicU64>,
    next_turn: u64,
    inlet_tx: Sender<ControlRequest>,
    inlet_rx: Receiver<ControlRequest>,
    approved: VecDeque<ControlRequest>,
    pub confirms: Vec<Confirm>,
    /// What the person is typing.
    pub input: String,
    /// Tokens used in this conversation.
    pub usage: Usage,
    /// Tokens of the turn in progress.
    turn_usage: Usage,
    /// A note for the model before the next message (the person undid its changes).
    note: Option<String>,
    /// The last design tab, so a turn does not carry over to another design.
    pub settings_open: bool,
    pub scroll_to_end: bool,
    /// A picture of the viewport waiting to go with the next message (mime, base64).
    pub picture: Option<(String, String)>,
    /// The API key being typed in the settings.
    pub key_edit: String,
    /// The saved conversation this chat is kept in (`None`: not saved yet).
    conv_id: Option<String>,
    /// The design tab the conversation belongs to.
    design_tab: Option<(usize, usize)>,
    /// Where settings and conversations are kept instead of the data folder (tests).
    pub data_root: Option<std::path::PathBuf>,
    /// The external agent from the last message, kept running for the next one.
    agent: Option<AgentSession>,
    /// Which agent profile `agent` was started from.
    agent_key: String,
    /// The listener agents' MCP servers connect to, started with the first agent message.
    endpoint: Option<Endpoint>,
    /// Questions from the agent waiting for the person.
    pub asks: Vec<Ask>,
}

impl Default for State {
    fn default() -> Self {
        let (inlet_tx, inlet_rx) = channel();
        State {
            settings: Settings::default(),
            settings_loaded: false,
            items: Vec::new(),
            history: Vec::new(),
            active: None,
            current_turn: Arc::new(AtomicU64::new(0)),
            next_turn: 1,
            inlet_tx,
            inlet_rx,
            approved: VecDeque::new(),
            confirms: Vec::new(),
            input: String::new(),
            usage: Usage::default(),
            turn_usage: Usage::default(),
            note: None,
            settings_open: false,
            scroll_to_end: false,
            picture: None,
            key_edit: String::new(),
            conv_id: None,
            design_tab: None,
            data_root: None,
            agent: None,
            agent_key: String::new(),
            endpoint: None,
            asks: Vec::new(),
        }
    }
}

impl State {
    /// A turn is running.
    pub fn busy(&self) -> bool {
        self.active.is_some()
    }

    /// How long the running turn has been going.
    pub fn elapsed(&self) -> Option<Duration> {
        self.active.as_ref().map(|a| a.started.elapsed())
    }

    /// The folder holding the settings file and the conversations.
    fn root(&self) -> Option<std::path::PathBuf> {
        self.data_root.clone().or_else(solidcraft_assistant::settings::data_folder)
    }

    fn store(&self) -> Option<solidcraft_assistant::store::Store> {
        Some(solidcraft_assistant::store::Store::at(self.root()?.join("conversations")))
    }

    /// Read the settings file the first time the sidebar is used.
    pub fn ensure_loaded(&mut self) {
        if !self.settings_loaded {
            if let Some(root) = self.root() {
                self.settings = Settings::load_from(&root.join("settings.json"));
            }
            self.settings_loaded = true;
        }
    }

    /// Mark the settings as loaded (tests set them directly).
    pub fn set_settings(&mut self, s: Settings) {
        self.settings = s;
        self.settings_loaded = true;
    }

    /// Save the settings, telling the person when it fails.
    pub fn save_settings(&mut self) {
        let Some(root) = self.root() else { return };
        if let Err(e) = self.settings.save_to(&root.join("settings.json")) {
            self.notice(e, true);
        }
    }

    fn notice(&mut self, text: impl Into<String>, error: bool) {
        self.items.push(Item::Notice { text: text.into(), error });
        self.scroll_to_end = true;
    }

    /// Start over: the conversation goes, the design stays.
    pub fn new_chat(&mut self) {
        self.stop();
        self.items.clear();
        self.history.clear();
        self.usage = Usage::default();
        self.note = None;
        self.picture = None;
        self.conv_id = None;
        // A new conversation is a new agent: its own context goes with the old one.
        self.agent = None;
    }

    /// Stop the running turn: it ends after the step in progress, and everything waiting for a
    /// yes is declined.
    pub fn stop(&mut self) {
        if let Some(a) = &mut self.active {
            a.handle.cancel.cancel();
            a.stopping_since.get_or_insert_with(Instant::now);
        }
        self.decline_all("cancelled by the user");
    }

    /// Answer the agent's pending questions (`None`: no).
    pub fn answer_ask(&mut self, index: usize, option: Option<String>) {
        if index < self.asks.len() {
            let a = self.asks.remove(index);
            let _ = a.reply.send(option);
        }
    }

    fn decline_all(&mut self, why: &str) {
        for a in self.asks.drain(..) {
            let _ = a.reply.send(None);
        }
        for c in self.confirms.drain(..) {
            let _ = c.req.reply.send(json!({"ok": false, "error": why}));
        }
        for r in self.approved.drain(..) {
            let _ = r.reply.send(json!({"ok": false, "error": why}));
        }
    }

    /// The person's answer to a held request.
    pub fn answer(&mut self, index: usize, allow: bool) {
        if index >= self.confirms.len() {
            return;
        }
        let c = self.confirms.remove(index);
        let live = self.active.as_ref().is_some_and(|a| !a.handle.cancel.is_cancelled());
        if allow && live {
            self.approved.push_back(c.req);
        } else {
            let why = if allow { "cancelled by the user" } else { "the user declined" };
            let _ = c.req.reply.send(json!({"ok": false, "error": why}));
        }
    }

    /// The most recent turn's checkpoint that is still good, with whether the design was
    /// changed since.
    pub fn undoable(&self, session: &solidcraft_engine::Session) -> Option<(usize, bool)> {
        let (i, id, rev) = self.items.iter().enumerate().rev().find_map(|(i, it)| match it {
            Item::TurnEnd { checkpoint: Some(id), revision, changed: true } => Some((i, *id, *revision)),
            _ => None,
        })?;
        session.has_checkpoint(id).then_some((i, session.revision != rev))
    }
}

/// What the person's message turned into.
pub enum Sent {
    Started,
    /// A quick action that needed no model.
    Handled,
    Refused(String),
}

/// Send `text` (a message or a quick action). `ctx` is woken as the answer arrives.
pub fn send(app: &mut SolveApp, ctx: &egui::Context, text: &str) -> Sent {
    app.assistant.ensure_loaded();
    if app.assistant.busy() {
        return Sent::Refused("the assistant is still answering; stop it first".into());
    }
    let text = text.trim();
    if text.is_empty() {
        return Sent::Refused("nothing to send".into());
    }
    if text.len() > MAX_MESSAGE {
        return Sent::Refused(format!("the message is too long ({} of {MAX_MESSAGE} bytes)", text.len()));
    }
    let situation = situation(app);
    let (shown, prompt) = match slash::parse(text, &situation) {
        Input::Message(m) => (m.clone(), m),
        Input::Prompt { shown, prompt } => (shown, prompt),
        Input::NewChat => {
            app.assistant.new_chat();
            return Sent::Handled;
        }
        Input::Help => {
            app.assistant.items.push(Item::User { text: text.to_string(), picture: false });
            app.assistant.notice(slash::help_text(), false);
            return Sent::Handled;
        }
        Input::Unknown(w) => return Sent::Refused(format!("unknown quick action `/{w}`; try /help")),
    };
    let s = app.assistant.settings.clone();
    let agent = s.backend == BackendKind::Agent;
    let api = s.api().clone();
    let key = api.resolved_key(s.backend);
    let http = app.services.assistant_http.clone();
    if agent {
        if s.selected_agent().is_none() {
            return Sent::Refused("add an agent in the assistant settings".into());
        }
        if app.services.start_agent_endpoint.is_none() {
            return Sent::Refused("the assistant needs the desktop app".into());
        }
    } else {
        if api.model.trim().is_empty() {
            return Sent::Refused("choose a model in the assistant settings".into());
        }
        if s.backend == BackendKind::Anthropic && key.is_empty() {
            return Sent::Refused("add your API key in the assistant settings (or set ANTHROPIC_API_KEY)".into());
        }
        if http.is_none() {
            return Sent::Refused("the assistant needs the desktop app".into());
        }
    }
    let design = app.session.execute("document.inspect", &json!({"measure": false})).ok();
    let mut prompt = prompt;
    if let Some(n) = app.assistant.note.take() {
        prompt = format!("{n}\n\n{prompt}");
    }
    let picture = app.assistant.picture.take();
    let had_picture = picture.is_some();
    // An agent started in this conversation needs the listener; start it before the checkpoint.
    let bridge = if agent {
        match agent_bridge(app, ctx, &s) {
            Ok(b) => Some(b),
            Err(e) => {
                app.assistant.picture = picture;
                return Sent::Refused(e);
            }
        }
    } else {
        None
    };
    let checkpoint = app.session.checkpoint();
    let revision = app.session.revision;
    let id = app.assistant.next_turn;
    app.assistant.next_turn += 1;
    app.assistant.current_turn.store(id, Ordering::SeqCst);
    let history_before = app.assistant.history.clone();
    let repaint_ctx = ctx.clone();
    let repaint: Arc<dyn Fn() + Send + Sync> = Arc::new(move || repaint_ctx.request_repaint());
    let cancel = Cancel::new();
    let spawned = if let (true, Some(profile)) = (agent, s.selected_agent()) {
        let text = match &design {
            Some(d) => format!("{}\n\n{prompt}", context::context_block(d)),
            None => prompt.clone(),
        };
        let key =
            format!("{}|{}|{}", profile.command, profile.args.join(" "), profile.env.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(","));
        if app.assistant.agent_key != key {
            app.assistant.agent = None;
            app.assistant.agent_key = key;
        }
        let job = AgentJob {
            profile: profile.clone(),
            text,
            picture: picture.clone(),
            bridge,
            cwd: agent_cwd(app),
            auto_allow: s.auto_allow_tools,
            session: app.assistant.agent.take(),
            repaint,
        };
        runtime::spawn_agent_turn(job, cancel)
    } else {
        let user = context::user_message(&prompt, design.as_ref(), picture);
        let codec: Arc<dyn Codec> = if s.backend == BackendKind::OpenAi { Arc::new(OpenAi) } else { Arc::new(Anthropic) };
        let host: Arc<dyn ToolHost> = Arc::new(UiHost { inlet: app.assistant.inlet_tx.clone(), ctx: ctx.clone(), turn: id });
        let vision = api.vision;
        let Some(http) = http else { return Sent::Refused("the assistant needs the desktop app".into()) };
        let job = Job {
            codec,
            http,
            base_url: api.base_url.clone(),
            allow_http: api.allow_http,
            key,
            model: api.model.trim().to_string(),
            system: context::system_prompt(),
            tools: solidcraft_assistant::tools::definitions(vision),
            vision,
            max_tokens: MAX_TOKENS,
            max_steps: MAX_STEPS,
            context_window: api.context_window,
            folders: s.folders.iter().map(std::path::PathBuf::from).collect(),
            history: std::mem::take(&mut app.assistant.history),
            user,
            host,
            repaint,
        };
        runtime::spawn_turn(job, cancel)
    };
    match spawned {
        Ok(handle) => {
            app.assistant.items.push(Item::User { text: shown, picture: had_picture });
            app.assistant.turn_usage = Usage::default();
            app.assistant.active = Some(Active {
                id,
                handle,
                checkpoint,
                revision,
                tab: (app.docs.active, app.docs.count()),
                started: Instant::now(),
                agent,
                before: history_before,
                stopping_since: None,
            });
            app.assistant.scroll_to_end = true;
            ctx.request_repaint();
            Sent::Started
        }
        Err(e) => {
            app.session.drop_checkpoint(checkpoint);
            app.assistant.current_turn.store(0, Ordering::SeqCst);
            Sent::Refused(e)
        }
    }
}

/// The program an external agent starts as its MCP server, and the endpoint it connects to.
fn agent_bridge(app: &mut SolveApp, ctx: &egui::Context, s: &Settings) -> Result<Bridge, String> {
    let cli = if s.cli.trim().is_empty() {
        solidcraft_assistant::acp::process::find_program("solidcraft-cli")
    } else {
        Some(std::path::PathBuf::from(s.cli.trim()))
    };
    let cli = cli.ok_or("solidcraft-cli was not found next to the app or on PATH; set its path in the assistant settings")?;
    if app.assistant.endpoint.is_none() {
        let start = app.services.start_agent_endpoint.clone().ok_or("the assistant needs the desktop app")?;
        let spec = EndpointSpec { inlet: app.assistant.inlet_tx.clone(), turn: app.assistant.current_turn.clone(), ctx: ctx.clone() };
        app.assistant.endpoint = Some(start(spec).map_err(|e| format!("cannot start the assistant's connection: {e}"))?);
    }
    let ep = app.assistant.endpoint.as_ref().ok_or("the assistant's connection is not running")?;
    Ok(Bridge { cli, port: ep.port, token: ep.token.clone() })
}

/// Where an agent program runs: the design's folder when that is a safe one, else a private
/// folder of its own.
fn agent_cwd(app: &SolveApp) -> std::path::PathBuf {
    if let Some(dir) = app.session.path.as_deref().and_then(|p| std::path::Path::new(p).parent())
        && solidcraft_engine::io::vfs::design_folder_refusal(dir).is_none()
        && dir.is_dir()
    {
        return dir.to_path_buf();
    }
    let own = app.assistant.root().map(|r| r.join("workspace"));
    if let Some(d) = own
        && std::fs::create_dir_all(&d).is_ok()
    {
        return d;
    }
    std::env::temp_dir()
}

/// Features with an error or a warning, as `name: message`.
pub(crate) fn situation_failing(app: &SolveApp) -> Vec<String> {
    let mut failing = Vec::new();
    for f in &app.session.doc.features {
        if let Some(r) = app.session.model.result(f.id) {
            if let Some(e) = &r.error {
                failing.push(format!("{}: {e}", f.name));
            } else if let Some(w) = &r.warning {
                failing.push(format!("{} (warning): {w}", f.name));
            }
        }
    }
    failing
}

/// What quick actions may refer to: failing features and the selection, in words.
fn situation(app: &SolveApp) -> Situation {
    let failing = situation_failing(app);
    let selection = if app.session.selection.is_empty() {
        String::new()
    } else {
        format!("{} item(s); see `selection` in the design state", app.session.selection.len())
    };
    Situation { failing, selection }
}

/// Render the current view and keep it for the next message (a vision model sees it).
pub(crate) fn attach_view(app: &mut SolveApp) {
    match crate::control::render_model_png(app, 960, 600, None, None) {
        Ok(png) => app.assistant.picture = Some(("image/png".into(), base64(&png))),
        Err(e) => app.assistant.notice(format!("Cannot attach the view: {e}"), true),
    }
}

fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(c.get(1).copied().unwrap_or(0)) << 8) | u32::from(c.get(2).copied().unwrap_or(0));
        out.push(A[(n >> 18) as usize & 63] as char);
        out.push(A[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { A[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { A[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Run once per frame: take what the worker reported, and end a turn that belongs to a design
/// that is no longer shown.
pub fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    follow_design(app);
    let Some(a) = &app.assistant.active else { return };
    if a.tab != (app.docs.active, app.docs.count()) && !a.handle.cancel.is_cancelled() {
        app.assistant.stop();
        app.assistant.notice("Stopped: another design was selected.", true);
    }
    // A turn that was told to stop and still has not ended: let go of it.
    if app.assistant.active.as_ref().and_then(|a| a.stopping_since).is_some_and(|t| t.elapsed() > STOP_GRACE) {
        if let Some(a) = app.assistant.active.take() {
            app.assistant.history = a.before;
            app.assistant.current_turn.store(0, Ordering::SeqCst);
            app.assistant.decline_all("cancelled by the user");
            app.assistant.agent = None;
            app.assistant.notice("Stopped.", false);
            let changed = app.session.revision != a.revision;
            if changed {
                app.assistant.items.push(Item::TurnEnd { checkpoint: Some(a.checkpoint), revision: app.session.revision, changed: true });
            } else {
                app.session.drop_checkpoint(a.checkpoint);
            }
        }
        return;
    }
    let mut done = None;
    let mut n = 0;
    while n < 256 {
        n += 1;
        let Some(a) = &app.assistant.active else { break };
        match a.handle.rx.try_recv() {
            Ok(WorkerMsg::Event(e)) => apply(&mut app.assistant, e),
            Ok(WorkerMsg::Done { outcome, history }) => {
                done = Some((outcome, Some(history)));
                break;
            }
            Ok(WorkerMsg::AgentDone { outcome, session }) => {
                app.assistant.agent = session;
                done = Some((outcome, None));
                break;
            }
            Ok(WorkerMsg::Permission { request, reply }) => {
                if app.assistant.asks.len() < 16 {
                    app.assistant.asks.push(Ask { request, reply });
                } else {
                    let _ = reply.send(None);
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => break,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                done = Some((
                    solidcraft_assistant::tool_loop::TurnOutcome {
                        finish: Finish::Error("The assistant stopped unexpectedly.".into()),
                        usage: Usage::default(),
                        steps: 0,
                        tool_calls: 0,
                    },
                    None,
                ));
                break;
            }
        }
    }
    if let Some((outcome, history)) = done {
        finish(app, outcome, history);
    }
    if app.assistant.active.is_some() {
        ctx.request_repaint_after(Duration::from_millis(100));
    }
}

/// A chat belongs to one design: when another design tab is picked, this one is saved and the
/// newest chat of the design now shown comes up (or an empty one).
fn follow_design(app: &mut SolveApp) {
    let tab = (app.docs.active, app.docs.count());
    match app.assistant.design_tab {
        None => app.assistant.design_tab = Some(tab),
        Some(t) if t == tab => {}
        Some(_) if app.assistant.busy() => {}
        Some(_) => {
            app.assistant.design_tab = Some(tab);
            app.assistant.new_chat();
            if let Some(first) = saved_conversations(app).first().map(|s| s.id.clone()) {
                load_conversation(app, &first);
            }
        }
    }
}

fn apply(st: &mut State, e: Event) {
    match e {
        Event::Text(t) => {
            if let Some(Item::Assistant { text, .. }) = st.items.last_mut() {
                if text.len() < solidcraft_assistant::model::MAX_BLOCK {
                    text.push_str(&t);
                }
            } else {
                st.items.push(Item::Assistant { text: t, thinking: String::new() });
            }
        }
        Event::Thinking(t) => {
            if let Some(Item::Assistant { thinking, .. }) = st.items.last_mut() {
                if thinking.len() < 64 * 1024 {
                    thinking.push_str(&t);
                }
            } else {
                st.items.push(Item::Assistant { text: String::new(), thinking: t });
            }
        }
        Event::ToolStart { .. } => {}
        Event::ToolCall { id, name, input } => {
            let title = call_title(&name, &input);
            st.items.push(Item::Tool(ToolCard { id, name, title, input, result: None }));
        }
        Event::ToolResult { id, is_error, text, images, .. } => {
            for it in st.items.iter_mut().rev() {
                if let Item::Tool(c) = it
                    && c.id == id
                {
                    c.result = Some(ToolOutcome { is_error, text, images });
                    break;
                }
            }
        }
        Event::Usage(u) => st.turn_usage = u,
        Event::Notice(t) => st.items.push(Item::Notice { text: t, error: false }),
    }
    if st.items.len() > MAX_ITEMS {
        let drop = st.items.len() - MAX_ITEMS;
        st.items.drain(..drop);
    }
    st.scroll_to_end = true;
}

fn finish(app: &mut SolveApp, outcome: solidcraft_assistant::tool_loop::TurnOutcome, history: Option<Vec<Message>>) {
    let Some(a) = app.assistant.active.take() else { return };
    app.assistant.current_turn.store(0, Ordering::SeqCst);
    app.assistant.decline_all("the turn ended");
    if let Some(h) = history {
        app.assistant.history = h;
    }
    app.assistant.usage.add(&outcome.usage);
    match &outcome.finish {
        Finish::Done => {}
        Finish::Cancelled => app.assistant.notice("Stopped.", false),
        Finish::StepLimit => {
            app.assistant.notice(format!("Stopped after {} steps without a final answer. Ask it to continue if it should.", outcome.steps), true)
        }
        Finish::MaxTokens => app.assistant.notice("The reply was cut off at the length limit.", true),
        Finish::Error(m) => app.assistant.notice(m.clone(), true),
    }
    let changed = app.session.revision != a.revision;
    if changed {
        app.assistant.items.push(Item::TurnEnd { checkpoint: Some(a.checkpoint), revision: app.session.revision, changed: true });
    } else {
        // Nothing changed: the checkpoint is not needed.
        app.session.drop_checkpoint(a.checkpoint);
    }
    app.assistant.scroll_to_end = true;
    save_conversation(app);
}

/// Put the design back to before the last turn that changed it. `force` goes ahead although
/// the design was edited since (those edits are lost).
pub fn undo_turn(app: &mut SolveApp, force: bool) -> Result<(), String> {
    let Some((index, edited)) = app.assistant.undoable(&app.session) else {
        return Err("there is nothing to undo".into());
    };
    if edited && !force {
        return Err("the design was edited after this turn; undoing it would lose those edits".into());
    }
    let Item::TurnEnd { checkpoint: Some(id), .. } = app.assistant.items[index].clone() else { return Err("there is nothing to undo".into()) };
    app.session.restore_checkpoint(id).map_err(|e| e.to_string())?;
    // Later checkpoints of this conversation went with it.
    for it in app.assistant.items.iter_mut() {
        if let Item::TurnEnd { checkpoint, .. } = it
            && checkpoint.is_some_and(|c| c >= id)
        {
            *checkpoint = None;
        }
    }
    app.assistant.note = Some(
        "[The person undid your last changes: the design is back to how it was before their previous request. Do not assume those changes exist.]"
            .into(),
    );
    app.assistant.notice("Undid the last assistant changes.", false);
    app.follow_view_request(false);
    Ok(())
}

/// The next request for the UI thread to run: held ones the person allowed, then new ones from
/// the worker after the gate. Refused ones are answered here.
pub(crate) fn next_request(app: &mut SolveApp) -> Option<ControlRequest> {
    while let Some(req) = app.assistant.approved.pop_front() {
        if app.assistant.active.as_ref().is_some_and(|a| !a.handle.cancel.is_cancelled()) {
            return Some(req);
        }
        let _ = req.reply.send(json!({"ok": false, "error": "cancelled by the user"}));
    }
    loop {
        let req = app.assistant.inlet_rx.try_recv().ok()?;
        // Looked up when there is something to decide: it touches the file system.
        let roots = agent_roots(&app.assistant.settings.folders);
        if let Some(r) = gate(&mut app.assistant, req, &roots) {
            return Some(r);
        }
    }
}

/// Decide one request from the assistant: `Some` runs now; `None` was answered or is held.
fn gate(st: &mut State, mut req: ControlRequest, roots: &[String]) -> Option<ControlRequest> {
    let refuse = |req: &ControlRequest, why: &str| {
        let _ = req.reply.send(json!({"ok": false, "error": why}));
    };
    let turn = req.params.get("_turn").and_then(Value::as_u64);
    let live = st.active.as_ref().is_some_and(|a| Some(a.id) == turn && !a.handle.cancel.is_cancelled());
    if !live {
        refuse(&req, "cancelled: that turn is no longer running");
        return None;
    }
    if let Some(o) = req.params.as_object_mut() {
        o.remove("_turn");
        // The assistant's folders are the app's to choose, whatever the request says. A request
        // through the agent endpoint also names the MCP server's own scratch folder (where it
        // renders pictures); nothing else it claims is taken.
        let requested = o.remove("allow_paths");
        o.insert("allow_paths".into(), json!(merge_roots(roots, requested.as_ref(), &solidcraft_engine::io::vfs::temp_dir())));
    } else {
        refuse(&req, "the request parameters must be an object");
        return None;
    }
    match classify(&req.method, &req.params) {
        Verdict::Allow => Some(req),
        Verdict::Refuse(why) => {
            refuse(&req, &why);
            None
        }
        Verdict::Confirm(lines) => {
            if st.confirms.len() >= 16 {
                refuse(&req, "too many requests are waiting for an answer");
                return None;
            }
            st.confirms.push(Confirm { lines, req });
            None
        }
    }
}

/// The worker's route to the design: control requests into the inlet, answers back.
struct UiHost {
    inlet: Sender<ControlRequest>,
    ctx: egui::Context,
    turn: u64,
}

impl ToolHost for UiHost {
    fn call(&self, method: &str, params: Value, cancel: &Cancel) -> Result<Value, String> {
        let mut p = match params {
            Value::Object(o) => o,
            Value::Null => serde_json::Map::new(),
            _ => return Err("the request parameters must be an object".into()),
        };
        p.insert("_turn".into(), json!(self.turn));
        let (req, rx) = ControlRequest::new(method.to_string(), Value::Object(p));
        self.inlet.send(req).map_err(|_| "the app is closing".to_string())?;
        self.ctx.request_repaint();
        let limit = if method == "engine.batch" { BATCH_TIMEOUT } else { CALL_TIMEOUT };
        let start = Instant::now();
        loop {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(v) => {
                    return if v.get("ok").and_then(Value::as_bool) == Some(true) {
                        Ok(v.get("result").cloned().unwrap_or(Value::Null))
                    } else {
                        Err(v.get("error").and_then(Value::as_str).unwrap_or("the app refused the request").to_string())
                    };
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if cancel.is_cancelled() {
                        return Err("cancelled by the user".into());
                    }
                    if start.elapsed() > limit {
                        // Whatever is still queued for this turn must not run later.
                        cancel.cancel();
                        return Err("the app did not answer in time".into());
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Err("the app dropped the request".into()),
            }
        }
    }
}

/// The folder name of the open design's conversations.
fn conversation_key(app: &SolveApp) -> String {
    solidcraft_assistant::store::key_for(app.session.path.as_deref())
}

/// Save the conversation (after a turn). Failures are shown, never fatal.
fn save_conversation(app: &mut SolveApp) {
    use solidcraft_assistant::store::{self, Saved};
    if app.assistant.history.is_empty() {
        return;
    }
    let Some(store) = app.assistant.store() else { return };
    let id = app.assistant.conv_id.get_or_insert_with(store::new_id).clone();
    let saved = Saved {
        id,
        title: store::title_of(&app.assistant.history),
        updated: store::now(),
        messages: app.assistant.history.clone(),
        usage: app.assistant.usage,
    };
    if let Err(e) = store.save(&conversation_key(app), &saved) {
        log::warn!("assistant: {e}");
    }
}

/// The conversations saved for the open design, newest first.
pub fn saved_conversations(app: &SolveApp) -> Vec<solidcraft_assistant::store::Summary> {
    app.assistant.store().map(|s| s.list(&conversation_key(app))).unwrap_or_default()
}

/// Continue a saved conversation of the open design.
pub fn load_conversation(app: &mut SolveApp, id: &str) -> bool {
    if app.assistant.busy() {
        return false;
    }
    let Some(store) = app.assistant.store() else { return false };
    let Some(saved) = store.load(&conversation_key(app), id) else { return false };
    app.assistant.items = items_from_history(&saved.messages);
    app.assistant.history = saved.messages;
    app.assistant.usage = saved.usage;
    app.assistant.conv_id = Some(saved.id);
    app.assistant.note = None;
    app.assistant.scroll_to_end = true;
    true
}

/// The transcript of a conversation that was loaded from disk.
fn items_from_history(history: &[Message]) -> Vec<Item> {
    use solidcraft_assistant::model::{Block, Part, Role};
    let mut items: Vec<Item> = Vec::new();
    for m in history {
        for b in &m.blocks {
            match (m.role, b) {
                (Role::User, Block::Text(t)) if !t.starts_with(context::CONTEXT_OPEN) => items.push(Item::User { text: t.clone(), picture: false }),
                (Role::Assistant, Block::Text(t)) => items.push(Item::Assistant { text: t.clone(), thinking: String::new() }),
                (Role::Assistant, Block::ToolUse { id, name, input, .. }) => {
                    items.push(Item::Tool(ToolCard {
                        id: id.clone(),
                        name: name.clone(),
                        title: call_title(name, input),
                        input: input.clone(),
                        result: None,
                    }));
                }
                (_, Block::ToolResult { id, parts, is_error }) => {
                    let text =
                        parts.iter().filter_map(|p| if let Part::Text(t) = p { Some(t.as_str()) } else { None }).collect::<Vec<_>>().join("\n");
                    for it in items.iter_mut().rev() {
                        if let Item::Tool(c) = it
                            && &c.id == id
                        {
                            c.result = Some(ToolOutcome { is_error: *is_error, text, images: 0 });
                            break;
                        }
                    }
                }
                _ => {}
            }
        }
    }
    items
}

/// The conversation as Markdown, for the Export command.
pub fn export_text(app: &SolveApp) -> String {
    solidcraft_assistant::store::export_markdown(&app.assistant.history)
}

/// A summary of the sidebar for `ui.assistant {action: "state"}`: no secrets.
pub fn summary(app: &SolveApp) -> Value {
    let st = &app.assistant;
    let s = &st.settings;
    json!({
        "open": app.ui.show_assistant,
        "busy": st.busy(),
        "elapsed_s": st.elapsed().map(|d| d.as_secs_f64()),
        "agent_turn": st.active.as_ref().map(|a| a.agent),
        "asks": st.asks.iter().map(|a| json!({"title": a.request.title, "options": a.request.options.iter().map(|o| o.id.clone()).collect::<Vec<_>>()})).collect::<Vec<_>>(),
        "backend": s.backend.id(),
        "model": s.api().model,
        "has_key": !s.api().api_key.is_empty(),
        "items": st.items.iter().map(item_json).collect::<Vec<_>>(),
        "confirms": st.confirms.iter().map(|c| json!(c.lines)).collect::<Vec<_>>(),
        "usage": {"input": st.usage.input, "output": st.usage.output, "cache_read": st.usage.cache_read},
        "undoable": st.undoable(&app.session).map(|(_, edited)| json!({"edited_since": edited})),
    })
}

fn item_json(it: &Item) -> Value {
    match it {
        Item::User { text, picture } => json!({"kind": "user", "text": text, "picture": picture}),
        Item::Assistant { text, .. } => json!({"kind": "assistant", "text": text}),
        Item::Tool(c) => {
            json!({"kind": "tool", "name": c.name, "title": c.title, "result": c.result.as_ref().map(|r| json!({"is_error": r.is_error, "text": r.text, "images": r.images}))})
        }
        Item::Notice { text, error } => json!({"kind": "notice", "text": text, "error": error}),
        Item::TurnEnd { changed, .. } => json!({"kind": "turn_end", "changed": changed}),
    }
}
