// Durable, gitignored home for the scripts a user generates.
//
// While `script.json` at the repo root is the single script the editor is
// working on, this library keeps one file per named script so a user can
// come back to a previous one and tweak it instead of rebuilding it from
// scratch. `data/` is gitignored, so a user's own scripts are never
// committed.
//
// A library file is byte-for-byte the same shape as script.json (see
// ScriptFile in main.rs) - real ANSI escapes in turn.output, no extra
// fields, no envelope - so banner.py, record.sh and parse_script accept it
// with no conversion. Saving the same name again replaces that one file:
// the library holds the final version of each script, never its history.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

// How much of a generated default name to keep. Commands can be long, and a
// name has to stay a legal filename, so this truncates on a word boundary
// rather than cutting mid-word.
const MAX_SUGGESTED_NAME_CHARS: usize = 60;

// <repo root>/data/scripts - the directory itself may not exist yet; save
// creates it, and list/load treat its absence as "no scripts".
pub fn scripts_dir(root: &Path) -> PathBuf {
    root.join("data").join("scripts")
}

// Lowercase; whitespace and '_' become '-'; everything outside [a-z0-9-] is
// dropped; runs of '-' collapse; leading/trailing '-' trim. A name that
// slugifies to nothing at all (all punctuation, or all non-ASCII) falls back
// to "script", so a slug is always a usable filename and can never carry a
// path separator or "..".
pub fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        let mapped = if ch.is_whitespace() || ch == '_' {
            Some('-')
        } else {
            let lowered = ch.to_ascii_lowercase();
            (lowered.is_ascii_lowercase() || lowered.is_ascii_digit() || lowered == '-')
                .then_some(lowered)
        };
        let Some(mapped) = mapped else { continue };
        if mapped == '-' {
            // Collapse: only start a run of dashes once something is
            // already there to separate.
            if out.is_empty() || out.ends_with('-') {
                continue;
            }
        }
        out.push(mapped);
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "script".to_string()
    } else {
        out
    }
}

// The default name a recording is filed under when the user has not named
// the script: derived from the first turn's command, which is the most
// recognisable thing about a script. Empty (or a command with nothing
// nameable in it) still yields a usable name.
pub fn suggest_name(first_command: &str) -> String {
    let slug = slugify(first_command.trim());
    if slug.chars().count() <= MAX_SUGGESTED_NAME_CHARS {
        return slug;
    }
    let truncated: String = slug
        .chars()
        .take(MAX_SUGGESTED_NAME_CHARS)
        .collect::<String>();
    // Prefer cutting at a word boundary, but never end up with nothing.
    let truncated = match truncated.rfind('-') {
        Some(cut) if cut > 0 => truncated[..cut].to_string(),
        _ => truncated,
    };
    let truncated = truncated.trim_end_matches('-').to_string();
    if truncated.is_empty() {
        "script".to_string()
    } else {
        truncated
    }
}

// Script names (file stems), sorted, *.json only. A missing directory means
// no scripts yet rather than an error, so the app works on a fresh checkout.
// No caller yet: the follow-on task that lets a user pick a previous script
// codes against this.
#[allow(dead_code)]
pub fn list(root: &Path) -> Vec<String> {
    let dir = scripts_dir(root);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                return None;
            }
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .map(|stem| stem.to_string())
        })
        .collect();
    names.sort();
    names
}

// Writes contents to data/scripts/<slug>.json, replacing any previous
// version of that script, and returns the slug actually used. The write goes
// to a temp file in the same directory and is then renamed into place, so a
// crash or an external reader never sees a half-written script.
pub fn save(root: &Path, name: &str, contents: &str) -> Result<String, String> {
    let (slug, path) = script_path(root, name)?;
    let dir = scripts_dir(root);
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("failed to create {}: {e}", dir.display()))?;

    let temp = dir.join(temp_file_name(&slug));
    if let Err(e) = std::fs::write(&temp, contents) {
        let _ = std::fs::remove_file(&temp);
        return Err(format!("failed to write {}: {e}", temp.display()));
    }
    if let Err(e) = std::fs::rename(&temp, &path) {
        let _ = std::fs::remove_file(&temp);
        return Err(format!("failed to rename into {}: {e}", path.display()));
    }
    Ok(slug)
}

// The raw contents of a saved script. Takes a script name, not a path.
// No caller yet: see the note on list.
#[allow(dead_code)]
pub fn load(root: &Path, name: &str) -> Result<String, String> {
    let (_, path) = script_path(root, name)?;
    std::fs::read_to_string(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            format!(
                "no saved script named {name:?} in {}",
                scripts_dir(root).display()
            )
        } else {
            format!("failed to read {}: {e}", path.display())
        }
    })
}

// Resolves a script name to the one file it names. The name is slugified, and
// the result is checked against the library directory, so save/load can never
// be pointed at a path outside data/scripts/.
fn script_path(root: &Path, name: &str) -> Result<(String, PathBuf), String> {
    let slug = slugify(name);
    let dir = scripts_dir(root);
    let path = dir.join(format!("{slug}.json"));
    // Defense in depth: slugify already strips separators and dots, so this
    // fires only if that guarantee regresses.
    if path.parent() != Some(dir.as_path()) {
        return Err(format!(
            "script name {name:?} does not resolve inside {}",
            dir.display()
        ));
    }
    Ok((slug, path))
}

// Hidden and .tmp-suffixed so a concurrent list() never picks it up.
fn temp_file_name(slug: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!(".{slug}.{}-{nanos}.tmp", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ScriptFile;

    fn temp_root(tag: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("term-anim-library-{tag}-{unique}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    // A script.json-shaped payload with real ANSI escapes, as a recording
    // writes it.
    fn script_json() -> String {
        let script = ScriptFile {
            user_host: "kareem@offline-studios".to_string(),
            turns: vec![
                crate::Turn {
                    command: "whoami".to_string(),
                    output: "\x1b[1;36mKareem\x1b[0m".to_string(),
                },
                crate::Turn {
                    command: "ls".to_string(),
                    output: "\x1b[34mthemes\x1b[0m banner.py".to_string(),
                },
            ],
        };
        serde_json::to_string_pretty(&script).unwrap()
    }

    #[test]
    fn slugify_normalises_names() {
        assert_eq!(slugify("My First Script"), "my-first-script");
        assert_eq!(slugify("snake_case_name"), "snake-case-name");
        assert_eq!(slugify("  Mixed   CASE  "), "mixed-case");
        assert_eq!(slugify("already-fine"), "already-fine");
        assert_eq!(slugify("runs---of---dashes"), "runs-of-dashes");
        assert_eq!(
            slugify("dash - underscore _ mixed"),
            "dash-underscore-mixed"
        );
        assert_eq!(slugify("tabs\tand\nnewlines"), "tabs-and-newlines");
        assert_eq!(slugify("100% done!"), "100-done");
        assert_eq!(slugify("v2"), "v2");
    }

    #[test]
    fn slugify_falls_back_for_unnameable_input() {
        assert_eq!(slugify(""), "script");
        assert_eq!(slugify("   "), "script");
        assert_eq!(slugify("!!!...???"), "script");
        assert_eq!(slugify("日本語"), "script");
        assert_eq!(slugify("-_-"), "script");
    }

    #[test]
    fn slugify_strips_anything_that_could_escape_the_library() {
        for name in [
            "../escape",
            "../../etc/passwd",
            "/etc/passwd",
            "..",
            ".",
            "a/../b",
            "..\\windows\\system32",
            "~/secrets",
        ] {
            let slug = slugify(name);
            assert!(
                !slug.contains('/') && !slug.contains('\\') && !slug.contains(".."),
                "slug {slug:?} from {name:?} still carries path syntax"
            );
            assert!(
                slug.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "slug {slug:?} from {name:?} has characters outside [a-z0-9-]"
            );
        }
    }

    #[test]
    fn save_load_list_round_trip_preserves_bytes_and_ansi() {
        let root = temp_root("round-trip");
        let contents = script_json();
        let slug = save(&root, "Round Trip Script", &contents).unwrap();
        assert_eq!(slug, "round-trip-script");

        assert_eq!(list(&root), vec!["round-trip-script".to_string()]);
        let loaded = load(&root, "Round Trip Script").unwrap();
        assert_eq!(loaded, contents, "library file bytes changed");

        // And the bytes parse back into the app's own script shape with the
        // escapes intact, so opening it needs no conversion.
        let (user_host, turns) = crate::parse_script(&loaded).expect("library file must parse");
        assert_eq!(user_host, "kareem@offline-studios");
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].command, "whoami");
        assert_eq!(
            crate::display_to_ansi(&turns[0].output),
            "\x1b[1;36mKareem\x1b[0m"
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn save_twice_overwrites_leaving_one_final_version() {
        let root = temp_root("overwrite");
        save(&root, "My Script", "{\"turns\":[]}").unwrap();
        save(&root, "my-script", "{\"turns\":[1]}").unwrap();

        let dir = scripts_dir(&root);
        let files: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(files, vec!["my-script.json".to_string()], "{files:?}");
        assert_eq!(list(&root), vec!["my-script".to_string()]);
        assert_eq!(load(&root, "my-script").unwrap(), "{\"turns\":[1]}");

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_directory_lists_empty_and_is_created_on_save() {
        let root = temp_root("missing-dir");
        assert!(!scripts_dir(&root).exists());
        assert!(
            list(&root).is_empty(),
            "list must not error on a fresh root"
        );
        assert!(load(&root, "anything").is_err());

        save(&root, "first", "{}").unwrap();
        assert!(scripts_dir(&root).is_dir());
        assert_eq!(list(&root), vec!["first".to_string()]);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn list_ignores_non_json_entries() {
        let root = temp_root("ignore");
        save(&root, "kept", "{}").unwrap();
        let dir = scripts_dir(&root);
        std::fs::write(dir.join("notes.txt"), "not a script").unwrap();
        std::fs::write(dir.join("README"), "not a script").unwrap();
        std::fs::write(dir.join("script.json.bak"), "not a script").unwrap();
        std::fs::create_dir_all(dir.join("subdir.json")).unwrap();

        assert_eq!(list(&root), vec!["kept".to_string()]);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn names_that_look_like_paths_stay_inside_the_library() {
        let root = temp_root("escape");
        let slug = save(&root, "../../escape", "{}").unwrap();

        let written = scripts_dir(&root).join(format!("{slug}.json"));
        assert_eq!(written.parent(), Some(scripts_dir(&root).as_path()));
        assert!(written.is_file(), "{} missing", written.display());
        assert!(!root.join("../../escape.json").exists());
        assert!(!root.join("escape.json").exists());
        assert!(load(&root, "/etc/passwd").is_err());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn suggest_name_comes_from_the_first_command() {
        assert_eq!(suggest_name(""), "script");
        assert_eq!(suggest_name("   "), "script");
        assert_eq!(suggest_name("whoami"), "whoami");
        assert_eq!(
            suggest_name("npx create-react-app my-app"),
            "npx-create-react-app-my-app"
        );
        assert_eq!(suggest_name("  git status  "), "git-status");
        assert_eq!(suggest_name("!!!"), "script");
    }

    #[test]
    fn suggest_name_keeps_names_a_legal_filename_length() {
        let long = "deploy --with-a-really-long-flag ".repeat(40);
        let name = suggest_name(&long);
        assert!(name.len() <= MAX_SUGGESTED_NAME_CHARS, "{name:?}");
        assert!(!name.ends_with('-'), "{name:?}");
        assert!(!name.is_empty());

        let root = temp_root("long-name");
        save(&root, &name, "{}").unwrap();
        assert_eq!(list(&root), vec![name]);
        std::fs::remove_dir_all(root).unwrap();
    }
}
