// Lightweight GUI shell for term-anim.
// It only edits script.json and shells out to those scripts

use eframe::egui;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

mod script_library;

#[derive(Serialize, Deserialize, Clone, Default)]
struct Turn {
    command: String,
    output: String,
}

// script.json's on-disk shape: an object, not a bare array, so it can also
// carry the prompt's user@host (optional - see build_prompt in banner.py).
#[derive(Serialize, Deserialize, Clone, Default)]
struct ScriptFile {
    #[serde(default)]
    user_host: String,
    turns: Vec<Turn>,
}

// Covers the SGR codes script.json actually uses: reset/bold/dim/italic/underline
// plus the 8 standard and 8 bright ANSI foreground colors.
const COLOR_TAGS: &[(&str, u8)] = &[
    ("reset", 0),
    ("bold", 1),
    ("dim", 2),
    ("italic", 3),
    ("underline", 4),
    ("black", 30),
    ("red", 31),
    ("green", 32),
    ("yellow", 33),
    ("blue", 34),
    ("magenta", 35),
    ("cyan", 36),
    ("white", 37),
    ("bright black / gray", 90),
    ("bright red", 91),
    ("bright green", 92),
    ("bright yellow", 93),
    ("bright blue", 94),
    ("bright magenta", 95),
    ("bright cyan", 96),
    ("bright white", 97),
];

// Curated 2-stop gradients for "Center on background"
// add_background.py itself is still generic (any two --bg/--bg-to
// colors work from the CLI); this is just the GUI's shortlist.
const BG_GRADIENTS: &[(&str, egui::Color32, egui::Color32)] = &[
    (
        "Midnight",
        egui::Color32::from_rgb(0x1a, 0x1b, 0x26),
        egui::Color32::from_rgb(0x2d, 0x2b, 0x55),
    ),
    (
        "Royal",
        egui::Color32::from_rgb(0x14, 0x1e, 0x30),
        egui::Color32::from_rgb(0x24, 0x3b, 0x55),
    ),
    (
        "Steel",
        egui::Color32::from_rgb(0x23, 0x25, 0x26),
        egui::Color32::from_rgb(0x41, 0x43, 0x45),
    ),
    (
        "Ocean",
        egui::Color32::from_rgb(0x21, 0x93, 0xb0),
        egui::Color32::from_rgb(0x6d, 0xd5, 0xed),
    ),
    (
        "Forest",
        egui::Color32::from_rgb(0x13, 0x4e, 0x5e),
        egui::Color32::from_rgb(0x71, 0xb2, 0x80),
    ),
    (
        "Mint",
        egui::Color32::from_rgb(0x00, 0xb0, 0x9b),
        egui::Color32::from_rgb(0x96, 0xc9, 0x3d),
    ),
    (
        "Sunset",
        egui::Color32::from_rgb(0xff, 0x7e, 0x5f),
        egui::Color32::from_rgb(0xfe, 0xb4, 0x7b),
    ),
    (
        "Fire",
        egui::Color32::from_rgb(0xcb, 0x2d, 0x3e),
        egui::Color32::from_rgb(0xef, 0x47, 0x3a),
    ),
    (
        "Peach",
        egui::Color32::from_rgb(0xff, 0xaf, 0xbd),
        egui::Color32::from_rgb(0xff, 0xc3, 0xa0),
    ),
    (
        "Grape",
        egui::Color32::from_rgb(0x65, 0x4e, 0xa3),
        egui::Color32::from_rgb(0xea, 0xaf, 0xc8),
    ),
];

// Banner-size presets. A uniform scale keeps the aspect ratio fixed, so
// every step is the same banner, just larger - see set_scale.py. Applied
// last, after any background frame.
const SCALE_PRESETS: &[(&str, f32)] = &[
    ("0.75x", 0.75),
    ("1x", 1.0),
    ("1.5x", 1.5),
    ("2x", 2.0),
    ("3x", 3.0),
];

enum WorkerMsg {
    Log(String),
    RecordDone(Result<PathBuf, String>),
    DetectDone(Result<(), String>),
}

// Held until the progress bar's fast-forward catch-up (see
// TermAnimApp::progress_fraction) finishes, so the bar always visually
// reaches 100% before the busy state actually clears - see poll_worker.
enum PendingDone {
    Record(Result<PathBuf, String>),
    Detect(Result<(), String>),
}

// How long the progress bar takes to race from wherever it was up to
// 100% once the real work has actually finished - see poll_worker
const CATCHUP_SECS: f32 = 0.4;

struct TermAnimApp {
    project_root: PathBuf,
    root_ok: bool,
    last_script_check: Instant,
    last_script_contents: Option<String>,
    user_host: String,
    turns: Vec<Turn>,
    themes: Vec<String>,
    selected_theme: usize,
    theme_filter: String,
    play_once: bool,
    opacity: f32,
    speed: f32,
    loop_delay: f32,
    show_chrome: bool,
    window_title: String,
    apply_background: bool,
    gradient_index: usize,
    padding: f32,
    scale_index: usize,
    busy: bool,
    busy_started: Option<Instant>,
    busy_estimate_secs: f32,
    pending_done: Option<PendingDone>,
    catchup_started: Option<Instant>,
    catchup_from: f32,
    last_error: Option<String>,
    last_output: Option<PathBuf>,
    status_log: String,
    // Name this script is filed under in the gitignored script library
    // (see script_library). Empty means "not named yet": a recording falls
    // back to suggest_name on the first turn's command.
    script_name: String,
    // The script picker: what the library held when it was last opened,
    // its search box, the saved script last opened into the editor, a pick
    // waiting on the discard-unsaved-changes confirm, and the last failed
    // pick.
    library: Vec<LibraryEntry>,
    library_filter: String,
    opened_script: Option<String>,
    pending_open: Option<String>,
    library_error: Option<String>,
    rx: Option<Receiver<WorkerMsg>>,
}

// The GUI displays and edits ANSI color codes as `{{codes}}` tags (e.g.
// `{{32}}` for green, `{{0}}` to reset) instead of raw ESC bytes
// script.json on disk still stores real ANSI escapes, since
// that's what banner.py types out; these two functions convert between the
// two forms at load/save time.
fn ansi_to_display(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b
            && bytes.get(i + 1) == Some(&b'[')
            && let Some(end) = s[i + 2..].find('m')
        {
            let codes = &s[i + 2..i + 2 + end];
            out.push_str("{{");
            out.push_str(codes);
            out.push_str("}}");
            i = i + 2 + end + 1;
            continue;
        }
        let ch = s[i..].chars().next().unwrap_or('\u{FFFD}');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn display_to_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    loop {
        match rest.find("{{") {
            Some(start) => match rest[start + 2..].find("}}") {
                Some(end) => {
                    out.push_str(&rest[..start]);
                    out.push('\x1b');
                    out.push('[');
                    out.push_str(&rest[start + 2..start + 2 + end]);
                    out.push('m');
                    rest = &rest[start + 2 + end + 2..];
                }
                None => {
                    out.push_str(rest);
                    break;
                }
            },
            None => {
                out.push_str(rest);
                break;
            }
        }
    }
    out
}

fn find_project_root() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let looks_like_root = |p: &Path| p.join("record.sh").exists() && p.join("themes").is_dir();
    if looks_like_root(&cwd) {
        return cwd;
    }
    match cwd.parent() {
        Some(parent) if looks_like_root(parent) => return parent.to_path_buf(),
        _ => (),
    }
    cwd
}

fn discover_themes(root: &Path) -> Vec<String> {
    let mut themes = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root.join("themes/palettes")) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                themes.push(stem.to_string());
            }
        }
    }
    themes.sort();
    themes
}

// Best-effort guess at "user@host", the same text a real shell prompt
// would show - used only as the initial value when there's no
// script.json yet to read one back from (see load_script below).
fn detect_username() -> Option<String> {
    std::env::var("USER")
        .ok()
        .or_else(|| std::env::var("USERNAME").ok())
        .or_else(|| {
            Command::new("whoami")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
        })
        .filter(|s| !s.is_empty())
}

fn detect_hostname() -> Option<String> {
    Command::new("hostname")
        .arg("-s")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

// A searchable ComboBox's button id. Must match how egui's
// ComboBox::from_id_salt derives its own internal id -
// make_persistent_id(IdSalt::new(salt)), not make_persistent_id(salt),
// which hashes differently.
fn combo_button_id(ui: &egui::Ui, salt: &str) -> egui::Id {
    ui.make_persistent_id(egui::IdSalt::new(salt))
}

fn theme_combo_button_id(ui: &egui::Ui) -> egui::Id {
    combo_button_id(ui, "theme_combo")
}

fn detect_user_host() -> String {
    match (detect_username(), detect_hostname()) {
        (Some(u), Some(h)) => format!("{u}@{h}"),
        (Some(u), None) => u,
        (None, Some(h)) => h,
        (None, None) => String::new(),
    }
}

// The window title a real terminal would show for this user_host
// ("user@host - zsh"). Empty user_host means no title at all.
fn default_window_title(user_host: &str) -> String {
    let user_host = user_host.trim();
    if user_host.is_empty() {
        String::new()
    } else {
        format!("{user_host} - zsh")
    }
}

fn effective_window_title(user_host: &str, typed: &str) -> String {
    let typed = typed.trim();
    if typed.is_empty() {
        default_window_title(user_host)
    } else {
        typed.to_string()
    }
}

// script.json's turns, converted to display form (see ansi_to_display),
// plus its user_host (the prompt's "user@host" - see banner.py's
// build_prompt).
fn parse_script(text: &str) -> Option<(String, Vec<Turn>)> {
    // Old script.json files are a bare `[...]` array of turns with no
    // user_host field at all - fall back to that shape if the new
    // object shape doesn't parse.
    let script: ScriptFile = serde_json::from_str(text)
        .or_else(|_| {
            serde_json::from_str::<Vec<Turn>>(text).map(|turns| ScriptFile {
                user_host: String::new(),
                turns,
            })
        })
        .ok()?;
    let turns = script
        .turns
        .into_iter()
        .map(|t| Turn {
            command: t.command,
            output: ansi_to_display(&t.output),
        })
        .collect();
    Some((script.user_host, turns))
}

fn serialize_script(user_host: &str, turns: &[Turn]) -> Result<String, String> {
    let script = ScriptFile {
        user_host: user_host.trim().to_string(),
        turns: turns
            .iter()
            .map(|turn| Turn {
                command: turn.command.clone(),
                output: display_to_ansi(&turn.output),
            })
            .collect(),
    };
    serde_json::to_string_pretty(&script)
        .map_err(|e| format!("failed to serialize script.json: {e}"))
}

fn write_script(path: &Path, user_host: &str, turns: &[Turn]) -> Result<String, String> {
    let contents = serialize_script(user_host, turns)?;
    std::fs::write(path, &contents)
        .map_err(|e| format!("failed to write {}: {e}", path.display()))?;
    Ok(contents)
}

fn load_script(root: &Path) -> (String, Vec<Turn>) {
    let path = root.join("script.json");
    let Ok(text) = std::fs::read_to_string(path) else {
        return (detect_user_host(), Vec::new());
    };
    parse_script(&text).unwrap_or_default()
}

// One saved script as the script picker lists it - see library_entries.
struct LibraryEntry {
    name: String,
    // Turn count and first command. None when the file can't be read or
    // parsed: the row still shows, hinted "unreadable", and picking it
    // reports the load error.
    summary: Option<(usize, String)>,
    modified: Option<SystemTime>,
}

// The saved scripts, newest first. Every file is read for its summary, so
// this runs when the picker opens rather than every frame.
fn library_entries(root: &Path) -> Vec<LibraryEntry> {
    let dir = script_library::scripts_dir(root);
    let mut entries: Vec<LibraryEntry> = script_library::list(root)
        .into_iter()
        .map(|name| {
            let summary = script_library::load(root, &name)
                .ok()
                .and_then(|text| parse_script(&text))
                .map(|(_, turns)| {
                    let first = turns
                        .first()
                        .map(|turn| turn.command.clone())
                        .unwrap_or_default();
                    (turns.len(), first)
                });
            let modified = std::fs::metadata(dir.join(format!("{name}.json")))
                .and_then(|m| m.modified())
                .ok();
            LibraryEntry {
                name,
                summary,
                modified,
            }
        })
        .collect();
    // list() is already sorted by name and the sort is stable, so ties keep
    // that order; a missing time sorts last.
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.modified));
    entries
}

fn format_age(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    match secs {
        0..60 => "just now".to_string(),
        60..3_600 => format!("{}m ago", secs / 60),
        3_600..86_400 => format!("{}h ago", secs / 3_600),
        86_400..604_800 => format!("{}d ago", secs / 86_400),
        _ => format!("{}w ago", secs / 604_800),
    }
}

// The dim text on the right of a picker row, e.g. "3 turns · whoami · 2h ago".
fn library_hint(entry: &LibraryEntry, now: SystemTime) -> String {
    const MAX_COMMAND_CHARS: usize = 32;
    let mut parts = Vec::new();
    match &entry.summary {
        Some((count, first)) => {
            parts.push(format!(
                "{count} turn{}",
                if *count == 1 { "" } else { "s" }
            ));
            if !first.is_empty() {
                if first.chars().count() > MAX_COMMAND_CHARS {
                    let cut: String = first.chars().take(MAX_COMMAND_CHARS - 3).collect();
                    parts.push(format!("{cut}..."));
                } else {
                    parts.push(first.clone());
                }
            }
        }
        None => parts.push("unreadable".to_string()),
    }
    if let Some(elapsed) = entry
        .modified
        .and_then(|modified| now.duration_since(modified).ok())
    {
        parts.push(format_age(elapsed));
    }
    parts.join(" · ")
}

// A saved script, converted to the editor's display form, or why it can't
// be opened.
fn load_library_script(root: &Path, name: &str) -> Result<(String, Vec<Turn>), String> {
    let text = script_library::load(root, name)?;
    parse_script(&text).ok_or_else(|| "not a valid script file".to_string())
}

// Whether the editor holds anything that isn't in script.json. Both sides go
// through serialize_script, so a script.json that was only reformatted by an
// external editor doesn't count as a change.
fn has_unsaved_edits(user_host: &str, turns: &[Turn], saved: Option<&str>) -> bool {
    let Ok(current) = serialize_script(user_host, turns) else {
        return true;
    };
    match saved.and_then(parse_script) {
        Some((saved_host, saved_turns)) => {
            serialize_script(&saved_host, &saved_turns).ok() != Some(current)
        }
        None => !turns.is_empty(),
    }
}

// The line under the script picker saying what a recording will be filed
// as. Re-using an existing name overwrites it silently, so that case has no
// caption at all.
fn script_name_caption(root: &Path, script_name: &str, turns: &[Turn]) -> Option<String> {
    if script_name.trim().is_empty() {
        let first = turns.first().map(|t| t.command.as_str()).unwrap_or("");
        return Some(format!(
            "Empty name: recording saves this as \"{}\" (from the first command).",
            script_library::suggest_name(first)
        ));
    }
    let slug = script_library::slugify(script_name);
    let exists = script_library::scripts_dir(root)
        .join(format!("{slug}.json"))
        .is_file();
    (!exists).then(|| format!("New script - recording saves it as \"{slug}\"."))
}

const SCRIPT_POLL_INTERVAL: Duration = Duration::from_millis(250);

// Detect edits from external editors, including atomic-save rename patterns.
fn script_contents_if_changed(path: &Path, last_contents: &mut Option<String>) -> Option<String> {
    let contents = std::fs::read_to_string(path).ok()?;
    if last_contents.as_deref() == Some(contents.as_str()) {
        return None;
    }
    *last_contents = Some(contents.clone());
    Some(contents)
}

fn color32_to_hex(c: egui::Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

// The typing-speed slider shows/edits this "display" value directly, but
// the actual --speed multiplier passed to banner.py is this value raised
// to a fractional power.
// chosen so display=20.0 produces an actual multiplier of only ~2 (20^(1/EXP)=2,
// since 2^EXP=20), applied symmetrically below 1.0 for slowing down too.
// This also allows display=1.0 to map to actual=1.0.
const SPEED_CURVE_EXPONENT: f32 = 4.321928; // log2(20)

fn display_speed_to_actual(display: f32) -> f32 {
    display.powf(1.0 / SPEED_CURVE_EXPONENT)
}

fn gradient_swatch(
    ui: &mut egui::Ui,
    from: egui::Color32,
    to: egui::Color32,
    selected: bool,
) -> egui::Response {
    let size = egui::vec2(44.0, 24.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let mut mesh = egui::Mesh::default();
        mesh.vertices
            .push(egui::epaint::Vertex::untextured(rect.left_top(), from));
        mesh.vertices
            .push(egui::epaint::Vertex::untextured(rect.right_top(), to));
        mesh.vertices
            .push(egui::epaint::Vertex::untextured(rect.right_bottom(), to));
        mesh.vertices
            .push(egui::epaint::Vertex::untextured(rect.left_bottom(), from));
        mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
        ui.painter().add(egui::Shape::mesh(mesh));
        let stroke = if selected {
            egui::Stroke::new(2.0, ui.visuals().selection.stroke.color)
        } else {
            egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color)
        };
        ui.painter()
            .rect_stroke(rect, 3.0, stroke, egui::StrokeKind::Inside);
    }
    response
}

// Runs `cmd` to completion. Deliberately doesn't capture stdout/stderr:
// record.sh runs termtosvg, which needs a REAL inherited terminal
// to actually capture a recording - piping its output silently
// produces an empty/static capture, and even just putting the child in
// its own process group breaks this the same way
fn run_step(mut cmd: Command, step_name: &str) -> Result<(), String> {
    let status = cmd
        .status()
        .map_err(|e| format!("failed to run {step_name}: {e}"))?;
    if !status.success() {
        return Err(format!("{step_name} exited with {status}"));
    }
    Ok(())
}

fn reveal_in_file_manager(path: &Path) {
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open").arg("-R").arg(path).spawn();
    }
}

impl TermAnimApp {
    fn new() -> Self {
        Self::with_root(find_project_root())
    }

    fn with_root(project_root: PathBuf) -> Self {
        let root_ok = project_root.join("record.sh").exists();
        let themes = discover_themes(&project_root);
        let library = library_entries(&project_root);
        let (user_host, turns) = load_script(&project_root);
        let last_script_contents = std::fs::read_to_string(project_root.join("script.json")).ok();
        // The window-title field starts empty: its hint is derived
        // live from user_host - see effective_window_title. Empty
        // lets the hint show and keeps the two fields in sync until
        // the user types an override.
        let window_title = String::new();
        Self {
            project_root,
            root_ok,
            last_script_check: Instant::now(),
            last_script_contents,
            user_host,
            turns,
            selected_theme: 0,
            themes,
            theme_filter: String::new(),
            play_once: false,
            opacity: 1.0,
            speed: 1.0,
            loop_delay: 5.0,
            show_chrome: true,
            window_title,
            apply_background: false,
            gradient_index: 0,
            padding: 30.0,
            scale_index: 1,
            busy: false,
            busy_started: None,
            busy_estimate_secs: 1.0,
            pending_done: None,
            catchup_started: None,
            catchup_from: 0.0,
            last_error: None,
            last_output: None,
            status_log: String::new(),
            script_name: String::new(),
            library,
            library_filter: String::new(),
            opened_script: None,
            pending_open: None,
            library_error: None,
            rx: None,
        }
    }

    // A pick from the script picker. Asks first when it would throw away
    // edits that exist nowhere else.
    fn request_open_script(&mut self, name: String) {
        if has_unsaved_edits(
            &self.user_host,
            &self.turns,
            self.last_script_contents.as_deref(),
        ) {
            self.pending_open = Some(name);
        } else {
            self.open_library_script(&name);
        }
    }

    fn open_library_script(&mut self, name: &str) {
        match load_library_script(&self.project_root, name) {
            Ok((user_host, turns)) => {
                self.user_host = user_host;
                self.turns = turns;
                self.script_name = name.to_string();
                self.opened_script = Some(name.to_string());
                self.library_error = None;
                // Write it straight through to script.json, which also moves
                // the poller's baseline to these exact bytes. Without this the
                // next poll sees script.json disagree with the editor: it
                // either reports our own write as an external change, or, if
                // script.json was left alone, swaps the picked script back
                // out the next time anything touches that file.
                match self.save_script_file() {
                    Ok(()) => self
                        .status_log
                        .push_str(&format!("opened saved script \"{name}\"\n")),
                    Err(e) => {
                        self.status_log.push_str(&format!("error: {e}\n"));
                        self.library_error = Some(e);
                    }
                }
            }
            Err(e) => {
                let message = format!("Couldn't open \"{name}\": {e}");
                self.status_log.push_str(&format!("error: {message}\n"));
                self.library_error = Some(message);
            }
        }
    }

    fn save_script_file(&mut self) -> Result<(), String> {
        let path = self.project_root.join("script.json");
        let contents = write_script(&path, &self.user_host, &self.turns)?;
        self.last_script_contents = Some(contents);
        self.last_script_check = Instant::now();
        Ok(())
    }

    fn theme_arg(&self) -> String {
        let base = self
            .themes
            .get(self.selected_theme)
            .cloned()
            .unwrap_or_else(|| "kanagawa_wave".to_string());
        if self.play_once {
            format!("{base}_once")
        } else {
            base
        }
    }

    fn progress_fraction(&self) -> f32 {
        if let Some(catchup_started) = self.catchup_started {
            let t = (catchup_started.elapsed().as_secs_f32() / CATCHUP_SECS).min(1.0);
            egui::lerp(self.catchup_from..=1.0, t)
        } else if let Some(started) = self.busy_started {
            (started.elapsed().as_secs_f32() / self.busy_estimate_secs).min(0.97)
        } else {
            0.0
        }
    }

    fn start_recording(&mut self) {
        if self.busy {
            return;
        }
        let root = self.project_root.clone();
        let theme = self.theme_arg();
        let speed = display_speed_to_actual(self.speed);
        let opacity = self.opacity;
        let loop_delay_ms = (self.loop_delay * 1000.0).round() as i64;
        let show_chrome = self.show_chrome;
        let window_title = effective_window_title(&self.user_host, &self.window_title);
        let user_host = self.user_host.trim().to_string();
        let turns: Vec<Turn> = self
            .turns
            .iter()
            .map(|t| Turn {
                command: t.command.clone(),
                output: display_to_ansi(&t.output),
            })
            .collect();
        let apply_background = self.apply_background;
        let (gradient_name, gradient_from, gradient_to) = BG_GRADIENTS[self.gradient_index];
        let bg_from = color32_to_hex(gradient_from);
        let bg_to = color32_to_hex(gradient_to);
        let padding = self.padding;
        let scale = SCALE_PRESETS[self.scale_index].1;
        let script_name = self.script_name.clone();

        // Rough estimate for the progress bar, not meant to be exact:
        let total_chars: usize = turns.iter().map(|t| t.command.chars().count()).sum();
        let typing_secs = total_chars as f32 * 0.085 / speed;
        let pause_secs = turns.len() as f32 * 1.2 / speed;
        let outro_secs = 5.0;
        let overhead_secs = 4.0;
        self.busy_estimate_secs = (typing_secs + pause_secs + outro_secs + overhead_secs).max(1.0);
        self.busy_started = Some(Instant::now());
        self.catchup_started = None;
        self.pending_done = None;

        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.busy = true;
        self.last_error = None;
        self.last_output = None;
        self.status_log.clear();

        thread::spawn(move || {
            let result = (|| -> Result<PathBuf, String> {
                let script_path = root.join("script.json");
                let script = ScriptFile { user_host, turns };
                let json = serde_json::to_string_pretty(&script)
                    .map_err(|e| format!("failed to serialize script.json: {e}"))?;
                std::fs::write(&script_path, &json)
                    .map_err(|e| format!("failed to write script.json: {e}"))?;
                tx.send(WorkerMsg::Log(format!("wrote {}", script_path.display())))
                    .ok();

                // File the very same script into the gitignored library, so
                // the two files can never disagree. A failure here is only
                // a warning: the recording and the render still go ahead.
                let name = if script_name.trim().is_empty() {
                    script_library::suggest_name(
                        script
                            .turns
                            .first()
                            .map(|turn| turn.command.as_str())
                            .unwrap_or(""),
                    )
                } else {
                    script_name.clone()
                };
                match script_library::save(&root, &name, &json) {
                    Ok(slug) => tx
                        .send(WorkerMsg::Log(format!(
                            "saved script to {}",
                            script_library::scripts_dir(&root)
                                .join(format!("{slug}.json"))
                                .display()
                        )))
                        .ok(),
                    Err(e) => tx
                        .send(WorkerMsg::Log(format!(
                            "warning: could not save script to the library: {e}"
                        )))
                        .ok(),
                };

                tx.send(WorkerMsg::Log(format!(
                    "running: ./record.sh {theme} {speed} {loop_delay_ms}"
                )))
                .ok();
                let mut cmd = Command::new("bash");
                cmd.arg("record.sh")
                    .arg(&theme)
                    .arg(speed.to_string())
                    .arg(loop_delay_ms.to_string())
                    .current_dir(&root);
                run_step(cmd, "record.sh")?;
                tx.send(WorkerMsg::Log("record.sh finished".to_string()))
                    .ok();

                let mut current = format!("banner-{theme}.svg");

                // Must run before opacity/background-framing below: both
                // of those locate the outer <svg>'s own viewBox/width by
                // regex, which only works on the chrome layout as
                // rendered - background-framing in particular nests the
                // whole file inside a new wrapper <svg>, which would
                // confuse strip_chrome.py's own outer-viewBox search if
                // run afterward.
                if !show_chrome {
                    tx.send(WorkerMsg::Log("stripping window chrome".to_string()))
                        .ok();
                    let mut cmd = Command::new("uv");
                    cmd.arg("run")
                        .arg("themes/strip_chrome.py")
                        .arg(&current)
                        .current_dir(&root);
                    run_step(cmd, "strip_chrome.py")?;
                    let stem = Path::new(&current)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("banner")
                        .to_string();
                    current = format!("{stem}-chromeless.svg");
                } else if !window_title.is_empty() {
                    tx.send(WorkerMsg::Log(format!(
                        "setting window title: {window_title}"
                    )))
                    .ok();
                    let mut cmd = Command::new("uv");
                    cmd.arg("run")
                        .arg("themes/set_window_title.py")
                        .arg(&current)
                        .arg("--title")
                        .arg(&window_title)
                        .current_dir(&root);
                    run_step(cmd, "set_window_title.py")?;
                    let stem = Path::new(&current)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("banner")
                        .to_string();
                    current = format!("{stem}-titled.svg");
                }

                if (opacity - 1.0).abs() > 0.001 {
                    tx.send(WorkerMsg::Log(format!("applying opacity {opacity:.2}")))
                        .ok();
                    let mut cmd = Command::new("uv");
                    cmd.arg("run")
                        .arg("themes/set_terminal_opacity.py")
                        .arg(&current)
                        .arg("--opacity")
                        .arg(opacity.to_string())
                        .current_dir(&root);
                    run_step(cmd, "set_terminal_opacity.py")?;
                    let stem = Path::new(&current)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("banner")
                        .to_string();
                    current = format!("{stem}-opacity.svg");
                }

                if apply_background {
                    tx.send(WorkerMsg::Log(format!(
                        "applying background gradient \"{gradient_name}\" (padding {padding})"
                    )))
                    .ok();
                    let mut cmd = Command::new("uv");
                    cmd.arg("run")
                        .arg("themes/add_background.py")
                        .arg(&current)
                        .arg("--bg")
                        .arg(&bg_from)
                        .arg("--bg-to")
                        .arg(&bg_to)
                        .arg("--padding")
                        .arg(padding.to_string())
                        .current_dir(&root);
                    run_step(cmd, "add_background.py")?;
                    let stem = Path::new(&current)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("banner")
                        .to_string();
                    current = format!("{stem}-framed.svg");
                }

                if (scale - 1.0).abs() > 0.001 {
                    tx.send(WorkerMsg::Log(format!("scaling banner {scale}x")))
                        .ok();
                    let mut cmd = Command::new("uv");
                    cmd.arg("run")
                        .arg("themes/set_scale.py")
                        .arg(&current)
                        .arg("--scale")
                        .arg(scale.to_string())
                        .current_dir(&root);
                    run_step(cmd, "set_scale.py")?;
                    let stem = Path::new(&current)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("banner")
                        .to_string();
                    current = format!("{stem}-scaled.svg");
                }

                Ok(root.join(current))
            })();
            tx.send(WorkerMsg::RecordDone(result)).ok();
        });
    }

    fn start_detect(&mut self) {
        if self.busy {
            return;
        }
        let root = self.project_root.clone();

        self.busy_estimate_secs = 3.0;
        self.busy_started = Some(Instant::now());
        self.catchup_started = None;
        self.pending_done = None;
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.busy = true;
        self.last_error = None;
        self.status_log.clear();

        thread::spawn(move || {
            let result = (|| -> Result<(), String> {
                // Needs this process's own stdin to be a real terminal:
                // the script writes raw OSC escape queries to it and reads
                // the reply on the same fd - only works if this app's own
                // stdin is the actual terminal device (i.e. launched with
                // `cargo run`, not from Finder).
                tx.send(WorkerMsg::Log(
                    "running: uv run themes/detect_terminal_theme.py".to_string(),
                ))
                .ok();
                let mut cmd = Command::new("uv");
                cmd.arg("run")
                    .arg("themes/detect_terminal_theme.py")
                    .current_dir(&root);
                run_step(cmd, "detect_terminal_theme.py").map_err(|_| {
                    "detection failed - check the terminal that launched this app for \
                     details. This window likely wasn't launched from a real terminal \
                     (needed for the OSC query), or your terminal doesn't support it \
                     (Apple's Terminal.app doesn't)."
                        .to_string()
                })?;

                tx.send(WorkerMsg::Log(
                    "running: uv run themes/build_themes.py live".to_string(),
                ))
                .ok();
                let mut cmd = Command::new("uv");
                cmd.arg("run")
                    .arg("themes/build_themes.py")
                    .arg("live")
                    .current_dir(&root);
                run_step(cmd, "build_themes.py")?;

                Ok(())
            })();
            tx.send(WorkerMsg::DetectDone(result)).ok();
        });
    }

    fn poll_script_changes(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(SCRIPT_POLL_INTERVAL);
        if self.last_script_check.elapsed() < SCRIPT_POLL_INTERVAL {
            return;
        }
        self.last_script_check = Instant::now();
        let Some(contents) = script_contents_if_changed(
            &self.project_root.join("script.json"),
            &mut self.last_script_contents,
        ) else {
            return;
        };
        if let Some((user_host, turns)) = parse_script(&contents) {
            self.user_host = user_host;
            self.turns = turns;
            self.status_log
                .push_str("reloaded script.json after external change\n");
        }
    }

    fn poll_worker(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.rx {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    WorkerMsg::Log(line) => {
                        self.status_log.push_str(&line);
                        self.status_log.push('\n');
                    }
                    WorkerMsg::RecordDone(result) => {
                        self.catchup_from = self.progress_fraction();
                        self.catchup_started = Some(Instant::now());
                        self.pending_done = Some(PendingDone::Record(result));
                    }
                    WorkerMsg::DetectDone(result) => {
                        self.catchup_from = self.progress_fraction();
                        self.catchup_started = Some(Instant::now());
                        self.pending_done = Some(PendingDone::Detect(result));
                    }
                }
            }
        }

        if let Some(catchup_started) = self.catchup_started
            && catchup_started.elapsed().as_secs_f32() >= CATCHUP_SECS
        {
            match self.pending_done.take() {
                Some(PendingDone::Record(Ok(path))) => {
                    self.library = library_entries(&self.project_root);
                    self.status_log
                        .push_str(&format!("done: {}\n", path.display()));
                    self.last_output = Some(path);
                }
                Some(PendingDone::Record(Err(e))) => {
                    // The script was filed before record.sh ran, so the
                    // library may have changed even though recording failed.
                    self.library = library_entries(&self.project_root);
                    self.status_log.push_str(&format!("error: {e}\n"));
                    self.last_error = Some(e);
                }
                Some(PendingDone::Detect(Ok(()))) => {
                    self.themes = discover_themes(&self.project_root);
                    if let Some(idx) = self.themes.iter().position(|t| t == "live") {
                        self.selected_theme = idx;
                    }
                    self.status_log
                        .push_str("done: detected live theme from terminal\n");
                }
                Some(PendingDone::Detect(Err(e))) => {
                    self.status_log.push_str(&format!("error: {e}\n"));
                    self.last_error = Some(e);
                }
                None => {}
            }
            self.busy = false;
            self.busy_started = None;
            self.catchup_started = None;
            self.rx = None;
        }

        if self.busy {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }
}

impl eframe::App for TermAnimApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_script_changes(&ctx);
        self.poll_worker(&ctx);

        egui::Panel::top("top").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("term-anim");
                ui.separator();
                ui.label(format!("project: {}", self.project_root.display()));
            });
            if !self.root_ok {
                ui.colored_label(
                    egui::Color32::from_rgb(220, 80, 80),
                    "Could not find record.sh / themes/ near the current directory - \
                     run this from inside term-anim/, or from term-anim/gui/.",
                );
            }
        });

        egui::Panel::bottom("log").show(ui, |ui| {
            egui::CollapsingHeader::new("Log")
                .default_open(false)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(200.0)
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(egui::RichText::new(&self.status_log).monospace())
                                    .selectable(true),
                            );
                        });
                });
        });

        egui::Panel::right("settings")
            .min_size(300.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("Settings");
                    ui.add_space(8.0);

                    if ui
                        .add_enabled(
                            !self.busy && self.root_ok,
                            egui::Button::new("Detect current terminal theme"),
                        )
                        .clicked()
                    {
                        self.start_detect();
                    }
                    ui.label(
                        egui::RichText::new(
                            "Needs this window launched from a real terminal (e.g. `cargo run`); \
                         doesn't work in Apple's Terminal.app.",
                        )
                        .small()
                        .weak(),
                    );

                    ui.add_space(12.0);
                    ui.label("Theme");
                    let current_name = self
                        .themes
                        .get(self.selected_theme)
                        .cloned()
                        .unwrap_or_else(|| "(none found)".to_string());
                    let combo_button_id = theme_combo_button_id(ui);
                    let popup_id = combo_button_id.with("popup");
                    let combo_was_open = egui::ComboBox::is_open(&ctx, combo_button_id);
                    let mut theme_picked = false;
                    egui::ComboBox::from_id_salt("theme_combo")
                        .selected_text(current_name)
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                        .show_ui(ui, |ui| {
                            if !combo_was_open {
                                self.theme_filter.clear();
                            }
                            let search_response = ui.add(
                                egui::TextEdit::singleline(&mut self.theme_filter)
                                    .hint_text("search themes...")
                                    .desired_width(f32::INFINITY),
                            );
                            if !combo_was_open {
                                search_response.request_focus();
                            }
                            ui.separator();
                            let filter = self.theme_filter.to_lowercase();
                            egui::ScrollArea::vertical()
                                .max_height(320.0)
                                .show(ui, |ui| {
                                    for (i, theme) in self.themes.iter().enumerate() {
                                        if !(filter.is_empty()
                                            || theme.to_lowercase().contains(&filter))
                                        {
                                            continue;
                                        }
                                        if ui
                                            .selectable_value(&mut self.selected_theme, i, theme)
                                            .clicked()
                                        {
                                            theme_picked = true;
                                        }
                                    }
                                });
                        });
                    if theme_picked {
                        egui::Popup::close_id(&ctx, popup_id);
                    }
                    ui.label(
                        egui::RichText::new(format!("{} themes available", self.themes.len()))
                            .small()
                            .weak(),
                    );

                    ui.add_space(12.0);
                    ui.label("Terminal opacity");
                    ui.add(egui::Slider::new(&mut self.opacity, 0.1..=1.0));

                    ui.add_space(12.0);
                    ui.label("Banner size");
                    egui::ComboBox::from_id_salt("banner_size")
                        .selected_text(SCALE_PRESETS[self.scale_index].0)
                        .show_ui(ui, |ui| {
                            for (i, (label, _)) in SCALE_PRESETS.iter().enumerate() {
                                ui.selectable_value(&mut self.scale_index, i, *label);
                            }
                        });
                    ui.label(
                        egui::RichText::new("Uniform scale - the aspect ratio never changes.")
                            .small()
                            .weak(),
                    );

                    ui.add_space(12.0);
                    ui.label("Typing speed");
                    // The slider drags across 0.2..20.0; typing a number
                    // directly (double-click, or the little text field) isn't
                    // clamped to that, only to the wider 0.01..50.0
                    // floor/ceiling clamped below.
                    ui.add(
                        egui::Slider::new(&mut self.speed, 0.2..=20.0)
                            .clamping(egui::SliderClamping::Never)
                            .text("multiplier"),
                    );
                    self.speed = self.speed.clamp(0.01, 50.0);

                    ui.add_space(12.0);
                    ui.add_enabled_ui(!self.play_once, |ui| {
                        ui.label("Loop delay (seconds before replay)");
                        ui.add(
                            egui::Slider::new(&mut self.loop_delay, 0.5..=30.0)
                                .clamping(egui::SliderClamping::Never),
                        );
                        self.loop_delay = self.loop_delay.clamp(0.0, 600.0);
                    });
                    if self.play_once {
                        ui.label(
                            egui::RichText::new(
                                "Ignored - \"play once\" freezes forever instead of looping.",
                            )
                            .small()
                            .weak(),
                        );
                    }

                    ui.add_space(12.0);
                    ui.add_enabled_ui(self.apply_background, |ui| {
                        ui.label("Background gradient");
                        egui::Grid::new("bg_gradients")
                            .spacing(egui::vec2(6.0, 6.0))
                            .show(ui, |ui| {
                                for (i, (name, from, to)) in BG_GRADIENTS.iter().enumerate() {
                                    let selected = self.gradient_index == i;
                                    if gradient_swatch(ui, *from, *to, selected)
                                        .on_hover_text(*name)
                                        .clicked()
                                    {
                                        self.gradient_index = i;
                                    }
                                    if (i + 1) % 5 == 0 {
                                        ui.end_row();
                                    }
                                }
                            });
                        ui.label(
                            egui::RichText::new(BG_GRADIENTS[self.gradient_index].0)
                                .small()
                                .weak(),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.padding, 0.0..=200.0).text("padding px"),
                        );
                    });

                    ui.add_space(16.0);
                    ui.separator();
                    ui.add_space(8.0);

                    ui.add_enabled_ui(!self.busy, |ui| {
                        ui.checkbox(&mut self.show_chrome, "macOS window chrome (title bar)");
                        ui.add_enabled_ui(self.show_chrome, |ui| {
                            ui.horizontal(|ui| {
                                ui.label("Title");
                                let hint = default_window_title(&self.user_host);
                                let hint = if hint.is_empty() {
                                    "user@host - shell".to_string()
                                } else {
                                    hint
                                };
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.window_title)
                                        .hint_text(hint)
                                        .desired_width(f32::INFINITY),
                                );
                            });
                        });
                        ui.label(
                            egui::RichText::new(
                                "Off: no title bar, no traffic lights, no padding - the \
                             terminal output fills the entire frame.",
                            )
                            .small()
                            .weak(),
                        );

                        ui.add_space(8.0);
                        ui.checkbox(&mut self.apply_background, "Center on background");
                        ui.label(
                            egui::RichText::new("Color and padding are up in Settings above.")
                                .small()
                                .weak(),
                        );

                        ui.add_space(8.0);
                        ui.checkbox(&mut self.play_once, "Play once (freeze on last frame)");
                    });

                    ui.add_space(16.0);
                    ui.horizontal(|ui| {
                        let button = egui::Button::new("Record");
                        if ui.add_enabled(!self.busy && self.root_ok, button).clicked() {
                            self.start_recording();
                        }
                        if self.busy {
                            ui.add(
                                egui::ProgressBar::new(self.progress_fraction())
                                    .desired_width(120.0)
                                    .desired_height(6.0),
                            );
                        }
                    });

                    if let Some(err) = &self.last_error {
                        ui.add_space(8.0);
                        ui.colored_label(egui::Color32::from_rgb(220, 80, 80), err);
                    }

                    if let Some(path) = self.last_output.clone() {
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("Show in Finder").clicked() {
                                reveal_in_file_manager(&path);
                            }
                            if ui.button("Open in browser").clicked() {
                                let url = format!("file://{}", path.display());
                                if let Err(e) = webbrowser::open(&url) {
                                    self.last_error = Some(format!("failed to open browser: {e}"));
                                }
                            }
                        });
                    }
                });
            });

        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Turns");
            ui.label("Each turn is a command that gets typed out, followed by its output.");
            // Recording writes script.json and the library from the worker
            // thread, so no picking while it runs.
            ui.add_enabled_ui(!self.busy && self.root_ok, |ui| {
                let mut picked: Option<String> = None;
                ui.horizontal(|ui| {
                    ui.label("Script");
                    let combo_button_id = combo_button_id(ui, "script_combo");
                    let popup_id = combo_button_id.with("popup");
                    let combo_was_open = egui::ComboBox::is_open(&ctx, combo_button_id);
                    let selected_text: egui::WidgetText = match &self.opened_script {
                        Some(name) => name.as_str().into(),
                        None if self.library.is_empty() => {
                            egui::RichText::new("(no saved scripts)").weak().into()
                        }
                        None => egui::RichText::new("choose...").weak().into(),
                    };
                    egui::ComboBox::from_id_salt("script_combo")
                        .selected_text(selected_text)
                        .width(160.0)
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                        .show_ui(ui, |ui| {
                            if !combo_was_open {
                                self.library = library_entries(&self.project_root);
                                self.library_filter.clear();
                            }
                            if self.library.is_empty() {
                                ui.label(
                                    egui::RichText::new(
                                        "Nothing saved yet. Every recording files the \
                                         current script here.",
                                    )
                                    .small()
                                    .weak(),
                                );
                                return;
                            }
                            ui.set_min_width(340.0);
                            let search_response = ui.add(
                                egui::TextEdit::singleline(&mut self.library_filter)
                                    .hint_text("search scripts...")
                                    .desired_width(f32::INFINITY),
                            );
                            if !combo_was_open {
                                search_response.request_focus();
                            }
                            ui.separator();
                            let filter = self.library_filter.to_lowercase();
                            let now = SystemTime::now();
                            egui::ScrollArea::vertical()
                                .max_height(320.0)
                                .show(ui, |ui| {
                                    for entry in &self.library {
                                        if !(filter.is_empty()
                                            || entry.name.to_lowercase().contains(&filter))
                                        {
                                            continue;
                                        }
                                        let selected = self.opened_script.as_deref()
                                            == Some(entry.name.as_str());
                                        let hint = egui::RichText::new(library_hint(entry, now))
                                            .small()
                                            .weak();
                                        let row =
                                            egui::Button::selectable(selected, entry.name.as_str())
                                                .right_text(hint)
                                                .min_size(egui::vec2(ui.available_width(), 0.0));
                                        if ui.add(row).clicked() {
                                            picked = Some(entry.name.clone());
                                        }
                                    }
                                });
                        });
                    if picked.is_some() {
                        egui::Popup::close_id(&ctx, popup_id);
                    }
                    ui.label("Name");
                    let first_command = self.turns.first().map(|t| t.command.as_str());
                    ui.add(
                        egui::TextEdit::singleline(&mut self.script_name)
                            .hint_text(script_library::suggest_name(first_command.unwrap_or("")))
                            .desired_width(190.0),
                    );
                });
                if let Some(caption) =
                    script_name_caption(&self.project_root, &self.script_name, &self.turns)
                {
                    ui.label(egui::RichText::new(caption).small().weak());
                }
                if let Some(err) = &self.library_error {
                    ui.colored_label(egui::Color32::from_rgb(220, 80, 80), err);
                }
                if let Some(name) = picked {
                    self.request_open_script(name);
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Add turn").clicked() {
                    self.turns.push(Turn::default());
                }
                ui.separator();
                ui.add(
                    egui::TextEdit::singleline(&mut self.user_host)
                        .hint_text("user@host")
                        .desired_width(180.0),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(!self.busy && self.root_ok, egui::Button::new("Save"))
                        .on_hover_text("Write the current prompt and turns to script.json")
                        .clicked()
                    {
                        match self.save_script_file() {
                            Ok(()) => {
                                self.last_error = None;
                                self.status_log.push_str("saved script.json\n");
                            }
                            Err(error) => self.last_error = Some(error),
                        }
                    }
                });
            });
            ui.add_space(8.0);
            ui.separator();

            let mut remove_index: Option<usize> = None;
            let mut move_up: Option<usize> = None;
            let mut move_down: Option<usize> = None;

            egui::ScrollArea::vertical().show(ui, |ui| {
                for i in 0..self.turns.len() {
                    ui.push_id(i, |ui| {
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(format!("Turn {}", i + 1));
                                if ui.small_button("up").clicked() {
                                    move_up = Some(i);
                                }
                                if ui.small_button("down").clicked() {
                                    move_down = Some(i);
                                }
                                if ui.small_button("remove").clicked() {
                                    remove_index = Some(i);
                                }
                            });
                            ui.label("Command");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.turns[i].command)
                                    .desired_width(f32::INFINITY),
                            );
                            ui.label("Output");
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("insert color:").small().weak());
                                egui::ComboBox::from_id_salt("insert_color")
                                    .selected_text("choose...")
                                    .show_ui(ui, |ui| {
                                        for (label, code) in COLOR_TAGS {
                                            if ui.selectable_label(false, *label).clicked() {
                                                self.turns[i]
                                                    .output
                                                    .push_str(&format!("{{{{{code}}}}}"));
                                            }
                                        }
                                    });
                            });
                            ui.add(
                                egui::TextEdit::multiline(&mut self.turns[i].output)
                                    .desired_rows(3)
                                    .desired_width(f32::INFINITY)
                                    .font(egui::TextStyle::Monospace),
                            );
                        });
                    });
                    ui.add_space(6.0);
                }
                if self.turns.is_empty() {
                    ui.weak("No turns yet - click \"Add turn\" above.");
                }
            });

            if let Some(i) = remove_index {
                self.turns.remove(i);
            }
            if let Some(i) = move_up
                && i > 0
            {
                self.turns.swap(i, i - 1);
            }
            if let Some(i) = move_down
                && i + 1 < self.turns.len()
            {
                self.turns.swap(i, i + 1);
            }
        });

        if let Some(name) = self.pending_open.clone() {
            let modal = egui::Modal::new(egui::Id::new("discard_unsaved_modal")).show(&ctx, |ui| {
                ui.set_width(380.0);
                ui.heading("Discard unsaved changes?");
                ui.label(format!(
                    "The editor has changes that aren't in script.json. Opening \"{name}\" \
                     replaces them."
                ));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Discard and open").clicked() {
                        Some(true)
                    } else if ui.button("Cancel").clicked() {
                        Some(false)
                    } else {
                        None
                    }
                })
                .inner
            });
            match modal.inner {
                Some(true) => {
                    self.pending_open = None;
                    self.open_library_script(&name);
                }
                Some(false) => self.pending_open = None,
                None if modal.should_close() => self.pending_open = None,
                None => {}
            }
        }
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "term-anim",
        options,
        Box::new(|_cc| Ok(Box::new(TermAnimApp::new()))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_poll_detects_external_atomic_replacement() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("term-anim-watch-{unique}"));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("script.json");
        std::fs::write(&path, r#"{"user_host":"before","turns":[]}"#).unwrap();
        let mut last_contents = std::fs::read_to_string(&path).ok();

        let replacement = dir.join("script.json.tmp");
        std::fs::write(
            &replacement,
            r#"{"user_host":"after","turns":[{"command":"whoami","output":"Kareem"}]}"#,
        )
        .unwrap();
        std::fs::rename(&replacement, &path).unwrap();

        let contents = script_contents_if_changed(&path, &mut last_contents)
            .expect("poll did not report the external file replacement");
        let (user_host, turns) = parse_script(&contents).expect("updated script should parse");
        assert_eq!(user_host, "after");
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].command, "whoami");
        assert!(script_contents_if_changed(&path, &mut last_contents).is_none());

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn save_writes_current_prompt_turns_and_ansi_colors() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("term-anim-save-{unique}.json"));
        let turns = vec![Turn {
            command: "whoami".to_string(),
            output: "{{1;36}}Kareem{{0}}".to_string(),
        }];
        write_script(&path, " kareem@offline-studios ", &turns).unwrap();
        let json = std::fs::read_to_string(&path).unwrap();
        let on_disk: ScriptFile = serde_json::from_str(&json).unwrap();
        assert_eq!(on_disk.user_host, "kareem@offline-studios");
        assert_eq!(on_disk.turns[0].output, "\x1b[1;36mKareem\x1b[0m");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn ansi_display_round_trip() {
        let cases = [
            "\x1b[1;32mAlice\x1b[0m \u{b7} Shipping @ \x1b[36mAcme Corp\x1b[0m",
            "\x1b[1;32m\u{25cf}\x1b[0m \x1b[1mexample.service\x1b[0m - part",
            "no escapes at all, just plain text",
            "",
        ];
        for original in cases {
            let display = ansi_to_display(original);
            assert!(
                !display.contains('\u{1b}'),
                "display form still has raw ESC: {display:?}"
            );
            let round_tripped = display_to_ansi(&display);
            assert_eq!(
                round_tripped, original,
                "round-trip mismatch for {original:?}"
            );
        }
    }

    #[test]
    fn display_to_ansi_produces_expected_bytes() {
        assert_eq!(display_to_ansi("{{32}}text{{0}}"), "\x1b[32mtext\x1b[0m");
    }

    #[test]
    fn speed_curve_preserves_1x_and_hits_the_20x_anchor() {
        assert!((display_speed_to_actual(1.0) - 1.0).abs() < 0.001);
        // display=20.0 should land close to actual=2 (the corrected
        // anchor: selecting "20x" on the slider should feel like roughly
        // a 2x actual multiplier, not a literal 20x).
        assert!((display_speed_to_actual(20.0) - 2.0).abs() < 0.05);
        // Symmetric below 1.0: display=0.05 should be the same distance
        // in log-space below 1.0 as display=20.0 is above it.
        let above = display_speed_to_actual(20.0).ln();
        let below = display_speed_to_actual(0.05).ln();
        assert!(
            (above + below).abs() < 0.001,
            "not symmetric: {above} vs {below}"
        );
    }

    #[test]
    fn default_title_tracks_user_host() {
        assert_eq!(default_window_title(""), "");
        assert_eq!(default_window_title("   "), "");
        assert_eq!(default_window_title("user@host"), "user@host - zsh");
        assert_eq!(default_window_title(" user@host "), "user@host - zsh");
    }

    #[test]
    fn typed_title_overrides_the_default() {
        assert_eq!(effective_window_title("user@host", ""), "user@host - zsh");
        assert_eq!(
            effective_window_title("user@host", "   "),
            "user@host - zsh"
        );
        assert_eq!(effective_window_title("user@host", "Custom"), "Custom");
        assert_eq!(effective_window_title("", "Custom"), "Custom");
        assert_eq!(effective_window_title("", ""), "");
    }

    #[test]
    fn theme_combo_button_id_matches_egui_internal_id() {
        // The theme search reads ComboBox::is_open() to know whether the
        // popup just opened (so it can focus/clear the search box once, not
        // every frame). That only works if the id we hold equals the id
        // ComboBox::from_id_salt builds internally -
        // make_persistent_id(IdSalt::new(salt)), not make_persistent_id(salt).
        let ctx = eframe::egui::Context::default();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            // What our code computes.
            let ours = theme_combo_button_id(ui);
            // What ComboBox::from_id_salt("theme_combo") computes internally.
            let internal = ui.make_persistent_id(eframe::egui::IdSalt::new("theme_combo"));
            eframe::egui::Popup::open_id(ui.ctx(), internal.with("popup"));
            assert!(
                eframe::egui::ComboBox::is_open(ui.ctx(), ours),
                "our combo id does not see the popup ComboBox opens"
            );
        });
        // A headless run produces texture deltas; drop them cleanly instead
        // of letting the debug assertion in epaint fire on drop.
        output.textures_delta.clear();
    }

    fn temp_project(tag: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("term-anim-picker-{tag}-{unique}"));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn turn(command: &str, output: &str) -> Turn {
        Turn {
            command: command.to_string(),
            output: output.to_string(),
        }
    }

    fn save_to_library(root: &Path, name: &str, turns: &[Turn]) {
        let json = serialize_script("kareem@offline-studios", turns).unwrap();
        script_library::save(root, name, &json).unwrap();
    }

    fn set_age(root: &Path, name: &str, age: Duration) {
        let path = script_library::scripts_dir(root).join(format!("{name}.json"));
        let file = std::fs::File::options().write(true).open(path).unwrap();
        file.set_modified(SystemTime::now() - age).unwrap();
    }

    #[test]
    fn library_entries_are_newest_first_with_summaries() {
        let root = temp_project("entries");
        save_to_library(&root, "old", &[turn("whoami", "")]);
        save_to_library(&root, "newest", &[turn("ls", ""), turn("pwd", "")]);
        save_to_library(&root, "middle", &[]);
        set_age(&root, "old", Duration::from_secs(3 * 86_400));
        set_age(&root, "middle", Duration::from_secs(2 * 3_600));
        std::fs::write(
            script_library::scripts_dir(&root).join("broken.json"),
            "not json",
        )
        .unwrap();
        set_age(&root, "broken", Duration::from_secs(30 * 86_400));

        let entries = library_entries(&root);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["newest", "middle", "old", "broken"]);

        let now = SystemTime::now();
        assert!(
            library_hint(&entries[0], now).starts_with("2 turns · ls · "),
            "{}",
            library_hint(&entries[0], now)
        );
        assert_eq!(library_hint(&entries[1], now), "0 turns · 2h ago");
        assert_eq!(library_hint(&entries[2], now), "1 turn · whoami · 3d ago");
        assert_eq!(library_hint(&entries[3], now), "unreadable · 4w ago");

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn library_hint_shortens_long_first_commands() {
        let entry = LibraryEntry {
            name: "long".to_string(),
            summary: Some((1, "x".repeat(50))),
            modified: None,
        };
        let hint = library_hint(&entry, SystemTime::now());
        assert_eq!(hint, format!("1 turn · {}...", "x".repeat(29)));
    }

    #[test]
    fn format_age_picks_the_largest_whole_unit() {
        assert_eq!(format_age(Duration::from_secs(0)), "just now");
        assert_eq!(format_age(Duration::from_secs(59)), "just now");
        assert_eq!(format_age(Duration::from_secs(60)), "1m ago");
        assert_eq!(format_age(Duration::from_secs(20 * 60)), "20m ago");
        assert_eq!(format_age(Duration::from_secs(3_600)), "1h ago");
        assert_eq!(format_age(Duration::from_secs(86_399)), "23h ago");
        assert_eq!(format_age(Duration::from_secs(86_400)), "1d ago");
        assert_eq!(format_age(Duration::from_secs(604_800)), "1w ago");
        assert_eq!(format_age(Duration::from_secs(10 * 604_800)), "10w ago");
    }

    #[test]
    fn unsaved_edits_ignore_formatting_but_not_content() {
        let turns = vec![turn("whoami", "{{1;36}}Kareem{{0}}")];
        let saved = serialize_script("me@host", &turns).unwrap();
        assert!(!has_unsaved_edits("me@host", &turns, Some(&saved)));

        // Same script, reformatted by an external editor.
        let compact: ScriptFile = serde_json::from_str(&saved).unwrap();
        let compact = serde_json::to_string(&compact).unwrap();
        assert_ne!(compact, saved);
        assert!(!has_unsaved_edits("me@host", &turns, Some(&compact)));

        let edited = vec![turn("whoami", "{{1;36}}Someone else{{0}}")];
        assert!(has_unsaved_edits("me@host", &edited, Some(&saved)));
        assert!(has_unsaved_edits("other@host", &turns, Some(&saved)));

        // No script.json yet: only real turns are worth asking about.
        assert!(!has_unsaved_edits("me@host", &[], None));
        assert!(has_unsaved_edits("me@host", &turns, None));
    }

    #[test]
    fn name_caption_suggests_announces_new_and_stays_silent_on_overwrite() {
        let root = temp_project("caption");
        let turns = vec![turn("git status", "")];
        assert_eq!(
            script_name_caption(&root, "  ", &turns).unwrap(),
            "Empty name: recording saves this as \"git-status\" (from the first command)."
        );
        assert_eq!(
            script_name_caption(&root, "Launch Banner", &turns).unwrap(),
            "New script - recording saves it as \"launch-banner\"."
        );
        save_to_library(&root, "launch-banner", &turns);
        assert_eq!(script_name_caption(&root, "Launch Banner", &turns), None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn opening_a_saved_script_syncs_script_json_without_tripping_the_poller() {
        let root = temp_project("open");
        let script_json = root.join("script.json");
        write_script(&script_json, "before@host", &[turn("ls", "")]).unwrap();
        let library_turns = vec![turn("whoami", "{{1;36}}Kareem{{0}}")];
        save_to_library(&root, "launch-banner", &library_turns);

        let mut app = TermAnimApp::with_root(root.clone());
        app.request_open_script("launch-banner".to_string());

        assert_eq!(app.pending_open, None, "nothing unsaved, so no confirm");
        assert_eq!(app.user_host, "kareem@offline-studios");
        assert_eq!(app.turns.len(), 1);
        assert_eq!(app.turns[0].output, "{{1;36}}Kareem{{0}}");
        assert_eq!(app.script_name, "launch-banner");
        assert_eq!(app.opened_script.as_deref(), Some("launch-banner"));
        assert_eq!(
            std::fs::read_to_string(&script_json).unwrap(),
            script_library::load(&root, "launch-banner").unwrap(),
            "script.json must hold the opened script"
        );
        assert!(
            script_contents_if_changed(&script_json, &mut app.last_script_contents).is_none(),
            "the poller would report the app's own write as an external change"
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn opening_over_unsaved_edits_waits_for_confirmation() {
        let root = temp_project("dirty");
        write_script(&root.join("script.json"), "me@host", &[turn("ls", "")]).unwrap();
        save_to_library(&root, "other", &[turn("whoami", "")]);

        let mut app = TermAnimApp::with_root(root.clone());
        app.turns[0].command = "ls -la".to_string();
        app.request_open_script("other".to_string());

        assert_eq!(app.pending_open.as_deref(), Some("other"));
        assert_eq!(
            app.turns[0].command, "ls -la",
            "edits must survive until confirmed"
        );
        assert_eq!(app.opened_script, None);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_failed_open_leaves_the_editor_and_script_json_alone() {
        let root = temp_project("fail");
        let script_json = root.join("script.json");
        write_script(&script_json, "me@host", &[turn("ls", "")]).unwrap();
        let before = std::fs::read_to_string(&script_json).unwrap();
        save_to_library(&root, "broken", &[]);
        std::fs::write(
            script_library::scripts_dir(&root).join("broken.json"),
            "not json",
        )
        .unwrap();

        let mut app = TermAnimApp::with_root(root.clone());
        app.request_open_script("broken".to_string());
        assert_eq!(
            app.library_error.as_deref(),
            Some("Couldn't open \"broken\": not a valid script file")
        );
        app.request_open_script("deleted".to_string());
        let error = app.library_error.clone().unwrap();
        assert!(
            error.starts_with("Couldn't open \"deleted\": no saved script"),
            "{error}"
        );

        assert_eq!(app.turns.len(), 1);
        assert_eq!(app.turns[0].command, "ls");
        assert_eq!(app.opened_script, None);
        assert_eq!(std::fs::read_to_string(&script_json).unwrap(), before);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn run_step_succeeds_on_zero_exit() {
        let result = run_step(Command::new("true"), "true");
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn run_step_reports_nonzero_exit() {
        let result = run_step(Command::new("false"), "false");
        let err = result.unwrap_err();
        assert!(err.contains("false"), "{err}");
    }
}
