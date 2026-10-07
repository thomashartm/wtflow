//! Persistent terminal workspace. All operations go through the shared command contract.
mod catalog;
mod explorer;
mod selection;
use crate::{app, clipboard, runtime, Command};
use anyhow::Result;
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use std::{
    collections::BTreeSet,
    io::{self, IsTerminal},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
        Arc,
    },
    time::{Duration, Instant},
};
use wtflow_core::Flow;
use wtflow_extract::{config::RepositoryConfig, EntryPoint};

const TABS: [&str; 6] = [
    "Project",
    "Entrypoints",
    "Flows",
    "Settings",
    "Activity",
    "Actions",
];
const PROJECT_ACTIONS: [(&str, &str, &str); 7] = [
    ("Initialize project", "init", "Choose the project's languages and owner, then create .wtflow/config.yaml. Existing settings are preserved. Setup does not run indexers."),
    ("Build / refresh index", "index", "Build code indexes for the enabled languages so wtflow can resolve calls between files. Unchanged indexes are reused unless force is enabled. Indexer output appears in Activity and .wtflow/logs."),
    ("Discover entrypoints", "entrypoints", "Scan source for routes, handlers, and other starting points. Refresh the entrypoint list and save .wtflow/entrypoints.json. Files that cannot be parsed are reported in the analysis warnings."),
    ("Analyze FILE#SYMBOL", "analyze", "Follow calls and branches from a function or method. Save the flow, analysis notes, and configured exports. Refreshing an existing flow retains labels on unchanged steps."),
    ("Inspect configuration", "config", "Show project settings in Activity. To change a setting, enter its dotted key and a YAML value in the options form, or use the Settings section. Changes are validated before saving."),
    ("Refresh workspace status", "refresh", "Reload saved flows, cached entrypoints, and configuration, and check index freshness. This does not rebuild indexes or rescan entrypoints."),
    ("Clear generated data", "clear", "Delete saved flows and their labels, diagrams, indexes, logs, and cached analysis inside .wtflow. Keep config.yaml unless remove-config is enabled. Files outside .wtflow are retained. Enable yes in the options form to confirm deletion."),
];
const SETTINGS: [(&str, &str, &str); 15] = [
    ("analysis.resolver", "auto", "auto / scip / heuristic"),
    ("analysis.depth", "32", "Maximum call traversal depth"),
    (
        "output.flows_dir",
        ".wtflow/flows",
        "Saved YAML and analysis context",
    ),
    (
        "output.export_dir",
        ".wtflow/flows",
        "Rendered exports; relative to project root",
    ),
    ("output.formats", "[html, md, lint]", "html, md, mmd, lint"),
    ("output.lang", "en", "Mermaid connector language: en / de"),
    (
        "output.detail",
        "false",
        "Show source alongside labels / expand HTML source details",
    ),
    ("output.direction", "TD", "Mermaid direction: TD / LR"),
    ("output.theme", "default", "default / light / dark"),
    (
        "output.expanded",
        "false",
        "Initially expand all HTML calls",
    ),
    ("output.open", "false", "Open HTML after analysis"),
    (
        "index.typescript.enabled",
        "false",
        "Enable TypeScript indexing",
    ),
    ("index.java.enabled", "false", "Enable Java indexing"),
    (
        "index.python.enabled",
        "false",
        "Enable Python indexing; set project_name in advanced settings",
    ),
    (
        "collapse",
        "true",
        "Collapse adjacent unresolved operations",
    ),
];
#[derive(Default)]
struct Snapshot {
    entries: Vec<EntryPoint>,
    flows: Vec<(PathBuf, Flow)>,
    config: serde_json::Value,
    status: String,
    notices: Vec<String>,
}
impl Snapshot {
    fn load(root: &Path, directory: &Path) -> Result<Self> {
        let config = RepositoryConfig::load(root)?;
        let mut snap = Self {
            status: "Not built".into(),
            ..Self::default()
        };
        if config.path.is_file() {
            snap.config = serde_yaml_ng::from_str(&crate::read(&config.path)?)?;
        }
        let entries = root.join(".wtflow/entrypoints.json");
        if entries.exists() {
            match crate::read(&entries).and_then(|s| Ok(serde_json::from_str(&s)?)) {
                Ok(entries) => snap.entries = entries,
                Err(e) => snap.notices.push(format!("Entrypoint cache: {e:#}")),
            }
        }
        let paths = crate::flows::saved_paths(directory, &config)?;
        for path in paths {
            let loaded = crate::load(&path).and_then(|f| {
                f.verify_fingerprint()?;
                Ok(f)
            });
            match loaded {
                Ok(flow) => snap.flows.push((path, flow)),
                Err(e) => snap.notices.push(format!("{e:#}")),
            }
        }
        if let Some(meta) = wtflow_resolve::metadata::Metadata::load(root)? {
            let stale = meta.files.iter().any(|(file, _)| {
                std::fs::read(root.join(file)).map_or(true, |bytes| !meta.fresh(file, &bytes))
            });
            let missing = meta
                .indexers
                .keys()
                .any(|lang| !root.join(format!(".wtflow/index/{lang}.scip")).is_file());
            snap.status = if missing {
                "Missing index"
            } else if stale {
                "Stale"
            } else {
                "Tracked files current"
            }
            .into();
        }
        Ok(snap)
    }
    fn setting(&self, key: &str, fallback: &str) -> String {
        let mut current = &self.config;
        for part in key.split('.') {
            current = &current[part];
        }
        if current.is_null() {
            if key == "output.export_dir" {
                return self.setting("output.flows_dir", ".wtflow/flows");
            }
            fallback.into()
        } else if let Some(text) = current.as_str() {
            text.into()
        } else {
            serde_yaml_ng::to_string(current)
                .unwrap_or_default()
                .trim()
                .to_owned()
        }
    }
}
struct Job {
    events: Receiver<runtime::Event>,
    done: Receiver<Result<(i32, Snapshot), String>>,
    cancelled: Arc<AtomicBool>,
    start: Instant,
    phase: String,
    saved: Option<PathBuf>,
}
struct Explorer {
    path: PathBuf,
    flow: Flow,
    context: wtflow_core::context::FlowContext,
    expanded: BTreeSet<String>,
    selected: usize,
    offset: usize,
}
struct Workspace {
    root: PathBuf,
    directory: PathBuf,
    no_open: bool,
    snapshot: Snapshot,
    tab: usize,
    selected: [usize; 6],
    offset: usize,
    query: String,
    searching: bool,
    tests: bool,
    form: Option<catalog::Form>,
    explorer: Option<Explorer>,
    job: Option<Job>,
    activity: Vec<String>,
    status: String,
    details: bool,
    details_offset: u16,
    list_area: Rect,
    help: bool,
    selection: selection::Selection,
}
impl Workspace {
    fn new(root: PathBuf) -> Self {
        Self {
            directory: root.clone(),
            no_open: false,
            root,
            snapshot: Snapshot::default(),
            tab: 0,
            selected: [0; 6],
            offset: 0,
            query: String::new(),
            searching: false,
            tests: false,
            form: None,
            explorer: None,
            job: None,
            activity: Vec::new(),
            status: "Loading workspace...".into(),
            details: false,
            details_offset: 0,
            list_area: Rect::default(),
            help: false,
            selection: selection::Selection::default(),
        }
    }
    fn launch(&mut self, command: Option<Command>) {
        if self.job.is_some() {
            self.status = "A job is running. Ctrl+C requests cancellation.".into();
            return;
        }
        let (events, receiver) = mpsc::channel();
        let (done, completion) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let observer = runtime::Observer {
            events,
            cancelled: cancelled.clone(),
        };
        let root = self.root.clone();
        let directory = self.directory.clone();
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                runtime::observe(observer, || -> Result<(i32, Snapshot)> {
                    let code = if let Some(command) = command {
                        app::execute(command, false)?
                    } else {
                        0
                    };
                    runtime::phase("Refreshing workspace status...");
                    Ok((code, Snapshot::load(&root, &directory)?))
                })
            }))
            .map_err(|_| "Operation panicked; terminal remains available".to_owned())
            .and_then(|r| r.map_err(|e| format!("{e:#}")));
            let _ = done.send(result);
        });
        self.job = Some(Job {
            events: receiver,
            done: completion,
            cancelled,
            start: Instant::now(),
            phase: "Starting...".into(),
            saved: None,
        });
        self.status = "Working...".into();
    }
    fn poll(&mut self) {
        let Some(job) = &mut self.job else {
            return;
        };
        for event in job.events.try_iter() {
            match event {
                runtime::Event::Phase(text) => job.phase = text,
                runtime::Event::FlowSaved(path) => job.saved = Some(path),
                runtime::Event::Output(text)
                | runtime::Event::Diagnostic(text)
                | runtime::Event::Log(text) => self.activity.extend(text.lines().map(clean)),
            }
        }
        if self.activity.len() > 4000 {
            self.activity.drain(..self.activity.len() - 4000);
        }
        match job.done.try_recv() {
            Ok(result) => {
                // Completion and events use separate channels; drain final events
                // before applying the result so a fast job cannot lose its artifact.
                for event in job.events.try_iter() {
                    match event {
                        runtime::Event::FlowSaved(path) => job.saved = Some(path),
                        runtime::Event::Phase(_) => {}
                        runtime::Event::Output(text)
                        | runtime::Event::Diagnostic(text)
                        | runtime::Event::Log(text) => {
                            self.activity.extend(text.lines().map(clean))
                        }
                    }
                }
                let elapsed = job.start.elapsed().as_secs_f32();
                let saved = job.saved.take();
                self.job = None;
                match result {
                    Ok((code, snap)) => {
                        self.activity.extend(snap.notices.iter().map(|s| clean(s)));
                        self.snapshot = snap;
                        if let Some(saved) = saved {
                            if !self.snapshot.flows.iter().any(|(path, _)| path == &saved) {
                                if let Ok(flow) = crate::load(&saved) {
                                    self.snapshot.flows.push((saved.clone(), flow));
                                }
                            }
                            if let Some((path, flow)) = self
                                .snapshot
                                .flows
                                .iter()
                                .find(|(path, _)| path == &saved)
                                .cloned()
                            {
                                self.open_flow(path, flow);
                            }
                        }
                        self.status = format!(
                            "{} in {elapsed:.1}s",
                            if code == 0 {
                                "Completed"
                            } else {
                                "Checks failed — see Activity"
                            }
                        );
                        if let Some(explorer) = &mut self.explorer {
                            if let Ok(flow) = crate::load(&explorer.path) {
                                explorer.flow = flow;
                                if explorer.context.fingerprint != explorer.flow.fingerprint {
                                    explorer.context = wtflow_core::context::FlowContext::default();
                                }
                            }
                        }
                    }
                    Err(error) => {
                        self.status = format!(
                            "{}: {}",
                            if error.contains("cancelled") {
                                "Cancelled"
                            } else {
                                "Failed"
                            },
                            clean(&error)
                        );
                        self.activity.push(self.status.clone());
                    }
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.job = None;
                self.status = "Worker disconnected".into();
            }
            _ => {}
        }
    }
    fn form(&mut self, name: &str) {
        if name == "refresh" {
            self.launch(None);
            return;
        }
        let mut form = catalog::Form::new(name);
        if name == "analyze" && self.no_open {
            form.set("no_open", "true");
        }
        if name == "init" {
            let mut langs = Vec::new();
            if self.root.join("package.json").exists() {
                langs.push("ts");
            }
            if self.root.join("pom.xml").exists() || self.root.join("build.gradle").exists() {
                langs.push("java");
            }
            if self.root.join("pyproject.toml").exists() {
                langs.push("py");
            }
            form.set("lang", langs.join(","));
        }
        if let Some(explorer) = &self.explorer {
            let path = explorer.path.to_string_lossy().into_owned();
            form.set("flow", &path);
            form.set("flows", shell_words::quote(&path));
            form.set(
                "entry",
                format!(
                    "{}#{}",
                    explorer.flow.entry.file, explorer.flow.entry.symbol
                ),
            );
        } else if self.tab == 1 {
            if let Some(entry) = self.entries().get(self.selected[1]) {
                form.set("entry", format!("{}#{}", entry.file, entry.symbol));
            }
        } else if self.tab == 2 {
            if let Some((path, flow)) = self.flows().get(self.selected[2]) {
                let path = path.to_string_lossy().into_owned();
                form.set("flow", &path);
                form.set("flows", shell_words::quote(&path));
                form.set(
                    "entry",
                    format!("{}#{}", flow.entry.file, flow.entry.symbol),
                );
            }
        }
        if matches!(name, "analyze" | "export") {
            form.about = format!(
                "{}\nSave: {} · Export: {}",
                form.about,
                self.snapshot.setting("output.flows_dir", ".wtflow/flows"),
                self.snapshot.setting("output.export_dir", ".wtflow/flows")
            );
        }
        self.form = Some(form);
    }
    fn entries(&self) -> Vec<&EntryPoint> {
        let filter = crate::filter::Filter::new(&self.query);
        self.snapshot
            .entries
            .iter()
            .filter(|e| {
                (self.tests || !crate::picker::is_test_file(&e.file))
                    && filter.matches(&[&e.trigger, &e.file, &e.symbol])
            })
            .collect()
    }
    fn flows(&self) -> Vec<&(PathBuf, Flow)> {
        let filter = crate::filter::Filter::new(&self.query);
        self.snapshot
            .flows
            .iter()
            .filter(|(_, f)| filter.matches(&[&f.flow, &f.entry.file, &f.entry.symbol]))
            .collect()
    }
    fn open_flow(&mut self, path: PathBuf, flow: Flow) {
        let context = crate::read(&path.with_extension("context.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<wtflow_core::context::FlowContext>(&s).ok())
            .filter(|c| c.fingerprint == flow.fingerprint)
            .unwrap_or_default();
        self.explorer = Some(Explorer {
            path,
            flow,
            context,
            expanded: BTreeSet::new(),
            selected: 0,
            offset: 0,
        });
        self.tab = 2;
    }
    fn count(&self) -> usize {
        if let Some(ex) = &self.explorer {
            return explorer::rows(&ex.flow, &ex.expanded).len();
        }
        match self.tab {
            0 => PROJECT_ACTIONS.len(),
            1 => self.entries().len(),
            2 => self.flows().len(),
            3 => SETTINGS.len() + 1,
            4 => self.activity.len(),
            _ => catalog::commands().len(),
        }
    }
    fn navigate(&mut self, delta: isize) {
        self.details_offset = 0;
        let count = self.count();
        let selected = if let Some(ex) = &mut self.explorer {
            &mut ex.selected
        } else {
            &mut self.selected[self.tab]
        };
        *selected = selected
            .saturating_add_signed(delta)
            .min(count.saturating_sub(1));
    }
    fn mouse(&mut self, mouse: MouseEvent) {
        if self.selection.mouse(mouse) {
            if matches!(mouse.kind, MouseEventKind::Up(event::MouseButton::Left)) {
                if let Some(text) = self.selection.text.clone() {
                    self.copy(&text);
                }
            }
            return;
        }
        if self.form.is_some() || self.help {
            return;
        }
        match mouse.kind {
            MouseEventKind::ScrollDown => self.navigate(3),
            MouseEventKind::ScrollUp => self.navigate(-3),
            MouseEventKind::Moved | MouseEventKind::Up(event::MouseButton::Left)
                if self
                    .list_area
                    .contains(Position::new(mouse.column, mouse.row)) =>
            {
                let offset = self
                    .explorer
                    .as_ref()
                    .map(|e| e.offset)
                    .unwrap_or(self.offset);
                let index = offset + (mouse.row - self.list_area.y) as usize;
                if index >= self.count() {
                    return;
                }
                let selected = if let Some(explorer) = &mut self.explorer {
                    &mut explorer.selected
                } else {
                    &mut self.selected[self.tab]
                };
                if *selected != index {
                    *selected = index;
                    self.details_offset = 0;
                }
            }
            _ => {}
        }
    }
    fn activate(&mut self) {
        if let Some(ex) = &mut self.explorer {
            if let Some(row) = explorer::rows(&ex.flow, &ex.expanded).get(ex.selected) {
                if let Some(node) = &row.node {
                    if !ex.expanded.remove(&node.id) {
                        ex.expanded.insert(node.id.clone());
                    }
                }
            }
            return;
        }
        match self.tab {
            0 => self.form(PROJECT_ACTIONS[self.selected[0].min(PROJECT_ACTIONS.len() - 1)].1),
            1 => self.form("analyze"),
            2 => {
                if let Some((path, flow)) = self.flows().get(self.selected[2]) {
                    self.open_flow(path.clone(), flow.clone());
                }
            }
            3 => {
                let i = self.selected[3];
                self.form("config");
                if let Some((key, fallback, _)) = SETTINGS.get(i) {
                    let value = self.snapshot.setting(key, fallback);
                    let form = self.form.as_mut().unwrap();
                    form.set("key", *key);
                    form.set("value", value);
                    form.selected = form
                        .fields
                        .iter()
                        .position(|f| f.id == "value")
                        .unwrap_or(0);
                }
            }
            5 => {
                if let Some((name, _)) = catalog::commands().get(self.selected[5]) {
                    self.form(name);
                }
            }
            _ => {}
        }
    }
    fn key(&mut self, key: KeyEvent) -> Result<bool> {
        if key.kind == KeyEventKind::Release {
            return Ok(false);
        }
        if clipboard::copy_key(key)
            || (self.form.is_none()
                && key.code == KeyCode::Char('y')
                && key.modifiers.contains(KeyModifiers::CONTROL))
        {
            self.copy(&self.copy_text());
            return Ok(false);
        }
        self.selection.clear();
        if clipboard::paste_key(key) {
            if self.can_paste() {
                match clipboard::read() {
                    Ok(text) => self.paste(&text),
                    Err(error) => self.status = error.to_string(),
                }
            } else {
                self.status = "Select a text field or an entrypoint/flow search to paste".into();
            }
            return Ok(false);
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            if let Some(job) = &self.job {
                job.cancelled.store(true, Ordering::Relaxed);
                self.status = "Cancellation requested; waiting for operation to stop...".into();
            } else {
                self.form = None;
                self.searching = false;
            }
            return Ok(false);
        }
        if self.help {
            self.help = false;
            return Ok(false);
        }
        if self.form.is_some() {
            self.form_key(key)?;
            return Ok(false);
        }
        if self.searching {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => self.searching = false,
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.query.clear();
                }
                KeyCode::Backspace => {
                    self.query.pop();
                }
                KeyCode::Char(c)
                    if !key.modifiers.intersects(
                        KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                    ) =>
                {
                    self.query.push(c)
                }
                _ => {}
            }
            self.selected[self.tab] = 0;
            return Ok(false);
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('p') {
            self.tab = 5;
            self.explorer = None;
            return Ok(false);
        }
        if key.modifiers.contains(KeyModifiers::ALT)
            && matches!(key.code, KeyCode::Up | KeyCode::Down)
        {
            self.details_offset = self
                .details_offset
                .saturating_add_signed(if key.code == KeyCode::Up { -3 } else { 3 });
            return Ok(false);
        }
        if self.details && matches!(key.code, KeyCode::PageUp | KeyCode::PageDown) {
            self.details_offset = self
                .details_offset
                .saturating_add_signed(if key.code == KeyCode::PageUp { -5 } else { 5 });
            return Ok(false);
        }
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
        {
            return Ok(false);
        }
        match key.code {
            KeyCode::Char('q') => {
                if self.job.is_some() {
                    self.status = "Job running. Cancel with Ctrl+C before quitting.".into();
                } else {
                    return Ok(true);
                }
            }
            KeyCode::Char('?') => self.help = true,
            KeyCode::Tab => {
                self.tab = (self.tab + 1) % 6;
                self.explorer = None;
                self.offset = 0;
            }
            KeyCode::BackTab => {
                self.tab = (self.tab + 5) % 6;
                self.explorer = None;
                self.offset = 0;
            }
            KeyCode::Char(c @ '1'..='6') => {
                self.tab = c as usize - '1' as usize;
                self.explorer = None;
                self.offset = 0;
            }
            KeyCode::Down | KeyCode::Char('j') => self.navigate(1),
            KeyCode::Up | KeyCode::Char('k') => self.navigate(-1),
            KeyCode::PageDown => self.navigate(10),
            KeyCode::PageUp => self.navigate(-10),
            KeyCode::Home => self.navigate(-(self.count() as isize)),
            KeyCode::End => self.navigate(self.count() as isize),
            KeyCode::Enter | KeyCode::Right => self.activate(),
            KeyCode::Left => {
                if let Some(ex) = &mut self.explorer {
                    let rows = explorer::rows(&ex.flow, &ex.expanded);
                    if let Some(row) = rows.get(ex.selected) {
                        let closed = row.node.as_ref().is_some_and(|n| ex.expanded.remove(&n.id));
                        if !closed {
                            if let Some(parent) = row.parents.last() {
                                ex.selected = rows
                                    .iter()
                                    .position(|r| r.node.as_ref().is_some_and(|n| &n.id == parent))
                                    .unwrap_or(0);
                            }
                        }
                    }
                }
            }
            KeyCode::Esc => {
                if self.explorer.take().is_none() {
                    self.query.clear();
                }
                self.details = false;
            }
            KeyCode::Char('/') => {
                self.searching = true;
                self.explorer = None;
            }
            KeyCode::F(2) => self.tests = !self.tests,
            KeyCode::Char('d') => self.details = !self.details,
            KeyCode::Char('i') => self.form("index"),
            KeyCode::Char('r') => self.form(if self.tab == 1 {
                "entrypoints"
            } else if self.tab == 2 {
                "update"
            } else {
                "refresh"
            }),
            KeyCode::Char('a') => self.form("analyze"),
            KeyCode::Char('e') => self.form("export"),
            KeyCode::Char('c') => self.form("check"),
            KeyCode::Char('l') => {
                let node = self.explorer.as_ref().and_then(|ex| {
                    explorer::rows(&ex.flow, &ex.expanded)
                        .get(ex.selected)
                        .and_then(|r| r.node.clone())
                });
                if let Some(node) = node {
                    self.form("label-step");
                    let form = self.form.as_mut().unwrap();
                    form.set("id", node.id);
                    form.set("text", node.label.unwrap_or_default());
                    form.selected = form.fields.len().saturating_sub(1);
                } else {
                    self.form("label");
                }
            }
            KeyCode::Char('*') => {
                if let Some(ex) = &mut self.explorer {
                    let mut nodes = Vec::new();
                    wtflow_core::visit(&ex.flow.steps, &mut nodes);
                    ex.expanded = nodes.iter().map(|n| n.id.clone()).collect();
                }
            }
            _ => {}
        }
        Ok(false)
    }
    fn form_key(&mut self, key: KeyEvent) -> Result<()> {
        if key.code == KeyCode::Esc {
            self.form = None;
            return Ok(());
        }
        if key.code == KeyCode::F(5)
            || (key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::CONTROL))
        {
            let form = self.form.as_ref().unwrap();
            match form.request() {
                Ok(command) => {
                    if self.job.is_some() {
                        self.form.as_mut().unwrap().error =
                            "Wait for the current job or cancel it with Ctrl+C.".into();
                        return Ok(());
                    }
                    let text = form.equivalent(&self.root);
                    self.activity.push(format!("$ {text}"));
                    if let Command::Flows {
                        dir,
                        filter,
                        no_open,
                    } = command
                    {
                        let dir = dir.canonicalize()?;
                        let root = RepositoryConfig::discover_root(&dir)?;
                        std::env::set_current_dir(&root)?;
                        self.root = root;
                        self.directory = dir.clone();
                        self.no_open = no_open;
                        self.query = filter.unwrap_or_default();
                        self.tab = 2;
                        self.explorer = None;
                        self.form = None;
                        self.status = if no_open {
                            "Browser opening disabled"
                        } else {
                            "Select a flow to explore; export with --open for the browser"
                        }
                        .into();
                        self.launch(Some(Command::Entrypoints {
                            dir,
                            filter: None,
                            json: false,
                        }));
                    } else {
                        self.form = None;
                        self.launch(Some(command));
                    }
                }
                Err(e) => self.form.as_mut().unwrap().error = format!("{e}"),
            }
            return Ok(());
        }
        if key.code == KeyCode::Char('y') && key.modifiers.contains(KeyModifiers::CONTROL) {
            let text = self.form.as_ref().unwrap().equivalent(&self.root);
            self.activity.push(text.clone());
            self.status = match clipboard::copy(&text) {
                Ok(()) => "Equivalent command copied".into(),
                Err(_) => "Clipboard unavailable; equivalent command saved in Activity".into(),
            };
            return Ok(());
        }
        let form = self.form.as_mut().unwrap();
        let count = form.fields.len();
        if count == 0 {
            return Ok(());
        }
        match key.code {
            KeyCode::Down | KeyCode::Tab | KeyCode::Enter => {
                form.selected = (form.selected + 1) % count
            }
            KeyCode::Up | KeyCode::BackTab => form.selected = (form.selected + count - 1) % count,
            _ => {
                let field = &mut form.fields[form.selected];
                if field.toggle {
                    if key.code == KeyCode::Char(' ') {
                        field.value = if field.value == "true" {
                            "false"
                        } else {
                            "true"
                        }
                        .into();
                    }
                } else {
                    match key.code {
                        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            field.value.clear()
                        }
                        KeyCode::Backspace => {
                            field.value.pop();
                        }
                        KeyCode::Char(c)
                            if !key.modifiers.intersects(
                                KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                            ) =>
                        {
                            field.value.push(c)
                        }
                        _ => {}
                    }
                }
            }
        }
        form.error.clear();
        Ok(())
    }
    fn copy_text(&self) -> String {
        if let Some(text) = &self.selection.text {
            return text.clone();
        }
        if let Some(form) = &self.form {
            return form
                .fields
                .get(form.selected)
                .map(|f| f.value.clone())
                .unwrap_or_default();
        }
        if self.searching {
            return self.query.clone();
        }
        if self.tab == 4 {
            return self
                .activity
                .get(self.selected[4])
                .cloned()
                .unwrap_or_default();
        }
        self.details()
    }
    fn copy(&mut self, text: &str) {
        if text.is_empty() {
            self.status = "No text to copy".into();
            return;
        }
        self.status = match clipboard::copy(text) {
            Ok(()) => "Copied to clipboard".into(),
            Err(error) => error.to_string(),
        };
    }
    fn can_paste(&self) -> bool {
        !self.help
            && if let Some(form) = &self.form {
                form.fields.get(form.selected).is_some_and(|f| !f.toggle)
            } else {
                self.searching || (matches!(self.tab, 1 | 2) && self.explorer.is_none())
            }
    }
    fn paste(&mut self, text: &str) {
        if !self.can_paste() {
            return;
        }
        self.selection.clear();
        if let Some(form) = &mut self.form {
            if let Some(field) = form.fields.get_mut(form.selected) {
                field.value.push_str(&clipboard::paste_text(text, true));
                form.error.clear();
            }
        } else {
            self.searching = true;
            self.query.push_str(&clipboard::paste_text(text, false));
            self.selected[self.tab] = 0;
            self.offset = 0;
        }
    }
    fn details(&self) -> String {
        if let Some(ex) = &self.explorer {
            if let Some(row) = explorer::rows(&ex.flow, &ex.expanded).get(ex.selected) {
                if let Some(n) = &row.node {
                    let mut text = format!("{}\n{}\n\n{}\n\nSource: {}\nStep: {}\nKind: {}\nTarget: {}\nBoundary: {}\n\n{}", ex.flow.flow, row.parents.join(" → "), n.label.as_deref().unwrap_or("No label"), n.src, n.id, n.kind.as_str(), n.target.as_deref().unwrap_or("—"), n.boundary.as_deref().unwrap_or("—"), n.code);
                    if let Some(call) = ex.context.calls.get(&n.id) {
                        text.push_str(&format!(
                            "\n\n{}\nReturn: {}\n{}\n{}",
                            call.signature,
                            call.return_type,
                            call.definition,
                            call.documentation.join("\n")
                        ));
                    }
                    text.push_str("\n\nStatic analysis: branches are alternatives. External / unresolved calls and traversal limits remain in analysis notes.");
                    return text;
                }
                return format!("{}\n\nChoose a step to inspect its source.", row.text);
            }
        }
        match self.tab {
            0 => PROJECT_ACTIONS.get(self.selected[0]).map(|(label, _, description)| format!("{label}\n\n{description}\n\n{}", if self.selected[0] == 5 { "Enter: refresh status" } else { "Enter: review options" })).unwrap_or_default(),
            1 => self.entries().get(self.selected[1]).map(|e| format!("{}\n\n{}\n{}\nLanguage: {}\n\nEnter / a: analysis options\nr: discover again\nF2: include tests\n\nOutput: {}\nResolver: {}", e.trigger, e.symbol, e.file, e.lang, self.snapshot.setting("output.flows_dir", ".wtflow/flows"), self.snapshot.setting("analysis.resolver", "auto"))).unwrap_or_else(|| "No matching entrypoints.\n\nPress r to discover routes and handlers.\nPress a to enter FILE#SYMBOL manually.".into()),
            2 => self.flows().get(self.selected[2]).map(|(p,f)| format!("{}\n\n{}#{}\n\n{}\nResolution: {}\n\nEnter: explore calls and branches\nr: refresh from source\nc: check\nl: labels\ne: export", f.flow, f.entry.file, f.entry.symbol, p.display(), f.resolution.as_str())).unwrap_or_else(|| "No saved flows. Choose an entrypoint or press a to analyze FILE#SYMBOL.".into()),
            3 => { let i = self.selected[3]; if let Some((key, fallback, help)) = SETTINGS.get(i) { format!("{key}\n\n{}\n\n{help}\n\nEnter: edit project default\n\nRun options override project settings. Relative destinations resolve from the project root.\n\nAdvanced configuration exposes module ownership, regex rules and indexer arguments.", self.snapshot.setting(key, fallback)) } else { "Edit any schema-supported dotted key. Values use YAML syntax.\n\nExamples:\nindex.python.project_name\nindex.typescript.args\nmodules\nrules\nignore".into() } }
            4 => "Operation output and diagnostics.\n\nIndex logs are stored under .wtflow/logs. Use Project → Refresh workspace status to update index freshness.\n\nCtrl+C cancels the active operation. Analysis stops at its next safe checkpoint; indexer processes are terminated.".into(),
            _ => catalog::commands().get(self.selected[5]).map(|(name, help)| {
                let description = PROJECT_ACTIONS.iter().find(|(_, command, _)| command == name).map(|(_, _, description)| *description).unwrap_or(help);
                format!("{name}\n\n{description}\n\nEnter: review options\nF5 runs the operation. Ctrl+Y copies its equivalent CLI command.")
            }).unwrap_or_default(),
        }
    }
    fn draw_details(&self, frame: &mut Frame, area: Rect, title: &str) {
        let block = panel(title);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let footer_height = if self.tab == 0 && self.explorer.is_none() {
            match inner.height {
                10.. => 6,
                8.. => 4,
                4.. => 2,
                _ => 0,
            }
        } else {
            0
        };
        let sections =
            Layout::vertical([Constraint::Min(0), Constraint::Length(footer_height)]).split(inner);
        frame.render_widget(
            Paragraph::new(clean(&self.details()))
                .scroll((self.details_offset, 0))
                .wrap(Wrap { trim: false }),
            sections[0],
        );
        if footer_height == 0 {
            return;
        }
        let footer = sections[1];
        if footer.height >= 4 {
            frame.render_widget(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(Color::DarkGray)),
                Rect::new(footer.x, footer.y, footer.width, 1),
            );
            frame.render_widget(
                Paragraph::new(format!(
                    "Project: {}",
                    clean(&self.root.display().to_string())
                ))
                .dim()
                .wrap(Wrap { trim: false }),
                Rect::new(footer.x, footer.y + 1, footer.width, footer.height - 3),
            );
        }
        frame.render_widget(
            Paragraph::new(format!("Index: {}", self.snapshot.status)).dim(),
            Rect::new(footer.x, footer.bottom() - 2, footer.width, 1),
        );
        frame.render_widget(
            Paragraph::new(format!(
                "{} entrypoints · {} saved flows",
                self.snapshot.entries.len(),
                self.snapshot.flows.len()
            ))
            .dim(),
            Rect::new(footer.x, footer.bottom() - 1, footer.width, 1),
        );
    }
    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        self.list_area = Rect::default();
        if area.width < 40 || area.height < 10 {
            frame.render_widget(
                Paragraph::new("wtflow\nResize to at least 40 × 10\nq: quit")
                    .wrap(Wrap { trim: false }),
                area,
            );
            return;
        }
        let compact = area.height < 16;
        let regions = Layout::vertical([
            Constraint::Length(if compact { 1 } else { 2 }),
            Constraint::Min(3),
            Constraint::Length(if compact { 1 } else { 2 }),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(vec![
                    Span::styled(" wtflow ", Style::default().fg(Color::Cyan).bold()),
                    Span::raw(clean(&self.root.display().to_string())),
                ]),
                Line::from(format!(" Index: {}", self.snapshot.status)).dim(),
            ]),
            regions[0],
        );
        let panels = if regions[1].width >= 95 {
            Layout::horizontal([Constraint::Percentage(54), Constraint::Percentage(46)])
                .split(regions[1])
        } else {
            Layout::horizontal([Constraint::Percentage(100), Constraint::Length(0)])
                .split(regions[1])
        };
        let rows: Vec<String> = if let Some(ex) = &self.explorer {
            explorer::rows(&ex.flow, &ex.expanded)
                .iter()
                .map(|r| r.text.clone())
                .collect()
        } else {
            match self.tab {
                0 => PROJECT_ACTIONS
                    .iter()
                    .enumerate()
                    .map(|(i, (label, _, _))| {
                        format!("{}{}", if i == 6 { "! " } else { "  " }, label)
                    })
                    .collect(),
                1 => self
                    .entries()
                    .iter()
                    .map(|e| {
                        format!(
                            "{}{}  ·  {}",
                            if self
                                .snapshot
                                .flows
                                .iter()
                                .any(|(_, f)| f.entry.file == e.file && f.entry.symbol == e.symbol)
                            {
                                "[saved] "
                            } else {
                                ""
                            },
                            if e.trigger.is_empty() {
                                &e.symbol
                            } else {
                                &e.trigger
                            },
                            e.file
                        )
                    })
                    .collect(),
                2 => self
                    .flows()
                    .iter()
                    .map(|(_, f)| format!("{}  ·  {}", f.flow, f.entry.file))
                    .collect(),
                3 => SETTINGS
                    .iter()
                    .map(|(key, default, _)| {
                        format!(
                            "{key}  {}",
                            self.snapshot.setting(key, default).replace('\n', " ")
                        )
                    })
                    .chain(std::iter::once("Advanced configuration…".into()))
                    .collect(),
                4 => self.activity.clone(),
                _ => catalog::commands()
                    .iter()
                    .map(|(name, _)| name.clone())
                    .collect(),
            }
        };
        let (selected, offset) = if let Some(ex) = &mut self.explorer {
            (&mut ex.selected, &mut ex.offset)
        } else {
            (&mut self.selected[self.tab], &mut self.offset)
        };
        *selected = (*selected).min(rows.len().saturating_sub(1));
        let mut state = ListState::default()
            .with_selected(Some(*selected))
            .with_offset(*offset);
        let title = if self.explorer.is_some() {
            " Flow path · ← parent · → expand · * expand all ".into()
        } else if self.tab == 1 || self.tab == 2 {
            format!(
                " {} · / {}{} ",
                TABS[self.tab],
                self.query,
                if self.tests { " · tests shown" } else { "" }
            )
        } else {
            format!(" {} ", TABS[self.tab])
        };
        if self.details && panels[1].width == 0 {
            self.draw_details(frame, panels[0], " Details · d return · PgUp/PgDn scroll ");
        } else {
            let list = List::new(rows.iter().map(|s| ListItem::new(clean(s))))
                .block(panel(&title))
                .highlight_style(Style::default().bg(Color::DarkGray).fg(Color::White).bold())
                .highlight_symbol("› ");
            frame.render_stateful_widget(list, panels[0], &mut state);
            if let Some(ex) = &mut self.explorer {
                ex.offset = state.offset();
            } else {
                self.offset = state.offset();
            }
            if rows.is_empty() {
                frame.render_widget(
                    Paragraph::new("No items yet.\nUse the actions shown in Details (d).")
                        .dim()
                        .wrap(Wrap { trim: false }),
                    panels[0].inner(Margin {
                        horizontal: 2,
                        vertical: 2,
                    }),
                );
            }
        }
        if !self.details || panels[1].width > 0 {
            self.list_area = panels[0].inner(Margin {
                horizontal: 1,
                vertical: 1,
            });
        }
        if panels[1].width > 0 {
            self.draw_details(frame, panels[1], " Details · Alt+↑/↓ scroll ");
        }
        let status = if let Some(job) = &self.job {
            format!(
                "◌ {} · {:.1}s · Ctrl+C cancel",
                job.phase,
                job.start.elapsed().as_secs_f32()
            )
        } else {
            self.status.clone()
        };
        frame.render_widget(
            Paragraph::new(clean(&status))
                .style(Style::default().fg(if self.status.starts_with("Failed") {
                    Color::Red
                } else {
                    Color::Cyan
                }))
                .wrap(Wrap { trim: false }),
            regions[2],
        );
        frame.render_widget(
            Paragraph::new(if self.searching {
                "Search: Ctrl+V paste · Alt+Y copy · Enter done"
            } else if area.width < 80 {
                "Tab panels  Enter open  d details  ? help  q quit"
            } else {
                "Tab section  Enter open  / search  Alt+Y copy  Ctrl+V paste  ? help  q quit"
            })
            .dim(),
            regions[3],
        );
        let navigation = TABS
            .iter()
            .enumerate()
            .map(|(i, title)| {
                let title = if area.width < 50 {
                    ["P", "E", "F", "S", "L", "A"][i]
                } else if area.width < 80 {
                    ["Proj", "Entries", "Flows", "Setup", "Logs", "Actions"][i]
                } else {
                    title
                };
                let style = if i == self.tab {
                    Style::default().fg(Color::Gray).underlined()
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                Span::styled(format!("{} {title} ", i + 1), style)
            })
            .collect::<Vec<_>>();
        frame.render_widget(Paragraph::new(Line::from(navigation)), regions[4]);
        if let Some(form) = &self.form {
            draw_form(frame, form, &self.root);
        }
        if self.help {
            let popup = popup(area, 78, 28);
            frame.render_widget(Clear, popup);
            frame.render_widget(Paragraph::new("Tab / Shift+Tab: section    1–6: jump to section\n↑ ↓ / j k: select    Page Up/Down: scroll\nEnter: open / expand    ←: collapse / parent\nEsc: back    *: expand entire flow\n/ : search    F2: include tests    d: details on narrow screens\n\ni: index    r: refresh / discover / update\na: analyze    e: export    c: check    l: labels\nCtrl+P: all CLI operations\n\nForms: Tab moves, Space toggles, Ctrl+U clears\nF5 / Ctrl+Enter: run    Ctrl+Y: copy CLI command\nEsc: close form without applying\n\nDrag text: release to copy (any pane or popup)\nAlt+Y / Ctrl+Shift+C / Ctrl+Insert: copy focused text\nCtrl+V / Shift+Insert / terminal Paste: paste\nSearch: line breaks become spaces; forms keep newlines\n\nCtrl+C: cancel job    q: quit when idle\n\nPaths and text are data; no shell commands are evaluated.\nPress any key to close.").block(panel(" Keyboard help ")).wrap(Wrap { trim: false }), popup);
        }
        self.selection.draw(frame);
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        if let Some(job) = self.job.take() {
            job.cancelled.store(true, Ordering::Relaxed);
            // Let indexers reap their process groups on terminal I/O failure too.
            let _ = job.done.recv_timeout(Duration::from_secs(5));
        }
    }
}

fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect()
}
fn panel(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(title.to_owned())
}
fn popup(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}
fn draw_form(frame: &mut Frame, form: &catalog::Form, root: &Path) {
    let area = popup(frame.area(), 100, (form.fields.len() as u16 + 12).max(16));
    frame.render_widget(Clear, area);
    frame.render_widget(panel(&format!(" {} · options ", form.name)), area);
    let inner = area.inner(Margin {
        horizontal: 2,
        vertical: 1,
    });
    let regions = Layout::vertical([
        Constraint::Length(if area.height < 16 { 1 } else { 2 }),
        Constraint::Min(2),
        Constraint::Length(if area.height < 16 { 1 } else { 3 }),
        Constraint::Length(if area.height < 16 { 1 } else { 2 }),
        Constraint::Length(if area.height < 16 { 1 } else { 2 }),
    ])
    .split(inner);
    frame.render_widget(
        Paragraph::new(clean(&form.about)).wrap(Wrap { trim: false }),
        regions[0],
    );
    let width = regions[1].width.saturating_sub(26) as usize;
    let items = form.fields.iter().map(|field| {
        let value = if field.toggle {
            if field.value == "true" {
                "[x] Space to toggle".into()
            } else {
                "[ ] Space to toggle".into()
            }
        } else if field.value.is_empty() {
            "<project default / omitted>".into()
        } else {
            clean(&field.value).replace('\n', " ")
        };
        let value: String = value
            .chars()
            .rev()
            .take(width)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        ListItem::new(format!(
            "{:<23} {}",
            format!(
                "{}{}",
                field.long.as_deref().unwrap_or(&field.id),
                if field.required { " *" } else { "" }
            ),
            value
        ))
    });
    let mut state = ListState::default().with_selected(Some(form.selected));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol("› ")
            .highlight_style(Style::default().bg(Color::DarkGray).fg(Color::White)),
        regions[1],
        &mut state,
    );
    let help = if form.error.is_empty() {
        form.fields
            .get(form.selected)
            .map(|f| f.help.clone())
            .unwrap_or_else(|| "No options. Press F5 to run.".into())
    } else {
        form.error.clone()
    };
    frame.render_widget(
        Paragraph::new(clean(&help))
            .style(Style::default().fg(if form.error.is_empty() {
                Color::Cyan
            } else {
                Color::Red
            }))
            .wrap(Wrap { trim: false }),
        regions[2],
    );
    frame.render_widget(
        Paragraph::new(clean(&form.equivalent(root)))
            .dim()
            .wrap(Wrap { trim: false }),
        regions[3],
    );
    frame.render_widget(
        Paragraph::new(if area.height < 16 {
            "Tab next · F5 run · Alt+Y copy · Ctrl+V paste"
        } else if form.name == "config" {
            "Tab field · Ctrl+U clear · F5 save default\nAlt+Y copy value · Ctrl+V paste · Ctrl+Y command · Esc cancel"
        } else {
            "Tab field · Ctrl+U clear · Space toggle · F5 run\nAlt+Y copy value · Ctrl+V paste · Ctrl+Y command · Esc cancel"
        })
        .wrap(Wrap { trim: false }),
        regions[4],
    );
}
struct Screen;
impl Screen {
    fn enter() -> Result<Self> {
        terminal::enable_raw_mode()?;
        let guard = Self;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            EnableMouseCapture,
            EnableBracketedPaste
        )?;
        Ok(guard)
    }
}
impl Drop for Screen {
    fn drop(&mut self) {
        let _ = execute!(
            io::stdout(),
            DisableMouseCapture,
            DisableBracketedPaste,
            LeaveAlternateScreen,
            crossterm::cursor::Show
        );
        let _ = terminal::disable_raw_mode();
    }
}
pub fn run(dir: &Path) -> Result<()> {
    anyhow::ensure!(
        io::stdin().is_terminal() && io::stdout().is_terminal(),
        "wtflow tui requires an interactive terminal; use CLI subcommands for scripts"
    );
    let root = RepositoryConfig::discover_root(dir)?;
    // The project is fixed for this TUI session. Workers never change process cwd.
    std::env::set_current_dir(&root)?;
    let _screen = Screen::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut workspace = Workspace::new(root);
    workspace.launch(None);
    loop {
        workspace.poll();
        terminal.draw(|frame| workspace.draw(frame))?;
        if event::poll(Duration::from_millis(60))? {
            match event::read()? {
                Event::Key(key) => match workspace.key(key) {
                    Ok(true) => break,
                    Ok(false) => {}
                    Err(error) => {
                        let message = format!("{error:#}");
                        if let Some(form) = &mut workspace.form {
                            form.error = message.clone();
                        }
                        workspace.status = format!("Failed: {message}");
                        workspace.activity.push(message);
                    }
                },
                Event::Paste(text) => workspace.paste(&text),
                Event::Mouse(mouse) => workspace.mouse(mouse),
                _ => {}
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layouts_fit_small_and_large_terminals() {
        for (width, height) in [(40, 10), (60, 10), (80, 24), (120, 35)] {
            let mut terminal =
                Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
            let mut ws = Workspace::new(PathBuf::from("/project"));
            for tab in 0..6 {
                ws.tab = tab;
                terminal.draw(|f| ws.draw(f)).unwrap();
            }
            ws.form("analyze");
            terminal.draw(|f| ws.draw(f)).unwrap();
        }
    }
    #[test]
    fn keyboard_preserves_filter_and_back_navigation() {
        let mut ws = Workspace::new(PathBuf::from("/project"));
        ws.tab = 1;
        for code in [KeyCode::Char('/'), KeyCode::Char('x'), KeyCode::Enter] {
            ws.key(KeyEvent::new(code, KeyModifiers::NONE)).unwrap();
        }
        assert_eq!(ws.query, "x");
        assert!(!ws.searching);
        ws.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();
        assert!(ws.query.is_empty());
    }
    #[test]
    fn every_form_field_pastes_and_copies_without_submitting_or_changing_toggles() {
        let mut ws = Workspace::new(PathBuf::from("/project"));
        let text = "café 🐈\r\nnext\tvalue\x1b\0";
        for (name, _) in catalog::commands() {
            ws.form(&name);
            let count = ws.form.as_ref().unwrap().fields.len();
            for i in 0..count {
                let form = ws.form.as_mut().unwrap();
                form.selected = i;
                form.error = "old validation error".into();
                let field = &mut form.fields[i];
                let before = field.value.clone();
                let toggle = field.toggle;
                ws.paste(text);
                let form = ws.form.as_ref().unwrap();
                let expected = if toggle {
                    before
                } else {
                    before + "café 🐈\nnext\tvalue"
                };
                assert_eq!(form.fields[i].value, expected, "{name} field {i}");
                assert_eq!(ws.copy_text(), expected, "{name} field {i}");
                assert!(ws.job.is_none());
                if !toggle {
                    assert!(form.error.is_empty());
                }
            }
        }
    }
    #[test]
    fn search_paste_resets_navigation_and_read_only_screens_ignore_it() {
        let mut ws = Workspace::new(PathBuf::from("/project"));
        for tab in 0..6 {
            ws.tab = tab;
            ws.searching = false;
            ws.query.clear();
            ws.selected[tab] = 10;
            ws.offset = 8;
            ws.paste("one\r\ntwo");
            if matches!(tab, 1 | 2) {
                assert_eq!(ws.query, "one two");
                assert_eq!(ws.selected[tab], 0);
                assert_eq!(ws.offset, 0);
                assert!(ws.searching);
                assert_eq!(ws.copy_text(), "one two");
                ws.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL))
                    .unwrap();
                assert!(ws.query.is_empty());
                ws.key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL))
                    .unwrap();
                assert!(ws.query.is_empty());
            } else {
                assert!(ws.query.is_empty());
                assert!(!ws.searching);
            }
        }
        ws.tab = 1;
        ws.help = true;
        ws.paste("ignore");
        assert!(ws.query.is_empty());
    }
    #[test]
    fn focused_copy_includes_full_details_activity_and_source() {
        let mut ws = Workspace::new(PathBuf::from("/project"));
        assert!(ws.copy_text().contains("Initialize project"));
        ws.tab = 3;
        assert!(ws.copy_text().contains("analysis.resolver"));
        ws.tab = 4;
        ws.activity
            .push("full untruncated diagnostic 🐈".repeat(20));
        assert_eq!(ws.copy_text(), ws.activity[0]);
        let flow = wtflow_core::yaml::load(
            include_str!("../../../testdata/golden/core.flow.yaml"),
            "test",
        )
        .unwrap();
        ws.open_flow(PathBuf::from("/not-found.flow.yaml"), flow);
        let expected = ws.details();
        assert!(!expected.is_empty());
        assert_eq!(ws.copy_text(), expected);
        ws.paste("must not edit flow");
        assert_eq!(ws.copy_text(), expected);
        assert!(ws.query.is_empty());
    }
    #[test]
    fn clicks_still_select_rows_and_drag_does_not_change_focus() {
        let mut ws = Workspace::new(PathBuf::from("/project"));
        let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(120, 35)).unwrap();
        terminal.draw(|frame| ws.draw(frame)).unwrap();
        let mouse = |kind, row| MouseEvent {
            kind,
            column: 3,
            row,
            modifiers: KeyModifiers::NONE,
        };
        let row = ws.list_area.y + 2;
        ws.mouse(mouse(MouseEventKind::Down(event::MouseButton::Left), row));
        ws.mouse(mouse(MouseEventKind::Up(event::MouseButton::Left), row));
        assert_eq!(ws.selected[0], 2);
        ws.mouse(mouse(MouseEventKind::Down(event::MouseButton::Left), row));
        ws.mouse(mouse(
            MouseEventKind::Drag(event::MouseButton::Left),
            row + 2,
        ));
        assert_eq!(ws.selected[0], 2);
        ws.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();
        assert!(ws.selection.text.is_none());
    }
}
