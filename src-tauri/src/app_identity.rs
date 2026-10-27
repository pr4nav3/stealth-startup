//! Durable app identity. An executable stem or a captured app name can be shared by different
//! apps and can change between versions, so each sample carries an identity built from native
//! metadata (Windows: the package's AUMID, else version info or the install path; macOS: the
//! bundle identifier). `AppKeys` turns identities into the app keys spans and rules store.
//! Keys stay the old stem where that is safe, so existing history and rules keep matching.

use crate::attribution::{SUPERVISOR_KEY, UNKNOWN_KEY};
use std::collections::{HashMap, HashSet};

/// One program that runs many apps. Its stem names no single app, so history kept under it
/// may be several apps mixed, and no new identity inherits it.
const SHARED_HOSTS: &[&str] = &[
    "java", "javaw", "python", "pythonw", "py", "pyw", "node", "electron", "wscript", "cscript",
    "mshta", "rundll32", "dllhost", "applicationframehost",
];

/// Keys Supervisor's own rows use. A captured app never takes one.
const RESERVED_KEYS: &[&str] = &[SUPERVISOR_KEY, UNKNOWN_KEY, "other-websites"];

/// Version info that names a runtime, not the app built on it.
const RUNTIME_PRODUCTS: &[&str] = &["electron", "nw.js", "chromium embedded framework"];

/// The stem a key was made from: `app#2` is the second app whose program is called `app`.
pub fn stem(key: &str) -> &str {
    key.split_once('#').map_or(key, |(stem, _)| stem)
}

/// "Name (2)" for the second app with the same program name, so two rows never look alike.
pub fn display_label(key: &str, label: &str) -> String {
    match key.split_once('#') {
        Some((_, n)) if !n.is_empty() => format!("{label} ({n})"),
        _ => label.to_string(),
    }
}

pub fn is_shared_host(stem: &str) -> bool {
    SHARED_HOSTS.contains(&stem.to_ascii_lowercase().as_str())
}

/// What a Windows executable's version resource says about who made it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VersionNames {
    pub company: Option<String>,
    pub product: Option<String>,
}

/// A packaged app is its AUMID, which survives updates (the install folder does not). Other
/// programs are their name plus who made them, which survives moves and updates; without
/// version info, the install path with version folders ignored. A shared host adds the
/// script it runs.
pub fn windows_identity(
    path: &str,
    aumid: Option<&str>,
    version: Option<&VersionNames>,
    script: Option<&str>,
) -> String {
    if let Some(aumid) = aumid.map(str::trim).filter(|aumid| !aumid.is_empty()) {
        return format!("aumid:{}", aumid.to_lowercase());
    }
    let stem = file_stem(path).to_lowercase();
    let clean = |value: &Option<String>| {
        value.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(str::to_lowercase)
    };
    let company = version.and_then(|v| clean(&v.company));
    let product = version.and_then(|v| clean(&v.product)).map(|p| product_family(&p));
    let names_the_app = (company.is_some() || product.is_some())
        && !product.as_deref().is_some_and(|p| RUNTIME_PRODUCTS.contains(&p));
    let base = if names_the_app {
        format!("exe:{stem}|{}|{}", company.unwrap_or_default(), product.unwrap_or_default())
    } else {
        format!("path:{}", normalize_path(path))
    };
    match script.filter(|_| is_shared_host(&stem)) {
        Some(script) => format!("{base}|{script}"),
        None => base,
    }
}

/// The bundle identifier survives renames and updates; the bundle path is the fallback.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn macos_identity(bundle_id: Option<&str>, path: &str) -> String {
    match bundle_id.map(str::trim).filter(|id| !id.is_empty()) {
        Some(id) => format!("bundle:{}", id.to_lowercase()),
        None => fallback_identity(path, ""),
    }
}

/// For a sample whose platform gave no better identity.
pub fn fallback_identity(path: &str, stem: &str) -> String {
    if path.trim().is_empty() {
        format!("name:{}", stem.to_lowercase())
    } else {
        format!("path:{}", normalize_path(path))
    }
}

/// Lowercase, one separator, and version folders (`app-1.0.9028`, `17.0.2`) as `*`, so an
/// update that installs into a new folder stays the same program.
pub fn normalize_path(path: &str) -> String {
    let path = path.trim().trim_matches('"');
    let path = path.strip_prefix(r"\\?\").unwrap_or(path);
    path.replace('/', "\\")
        .split('\\')
        .map(|segment| if is_version_segment(segment) { "*".to_string() } else { segment.to_lowercase() })
        .collect::<Vec<_>>()
        .join("\\")
}

fn is_version_segment(segment: &str) -> bool {
    let rest = segment
        .trim_start_matches(|ch: char| ch.is_ascii_alphabetic())
        .trim_start_matches(['-', '_', ' ']);
    let core = rest.split(['_', '-', '+', ' ']).next().unwrap_or("");
    core.contains('.') && core.split('.').all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

/// "adobe photoshop 2024" and "adobe photoshop 2025" are one product.
fn product_family(product: &str) -> String {
    let mut words: Vec<&str> = product.split_whitespace().collect();
    while words.len() > 1 && words.last().is_some_and(|word| is_version_word(word)) {
        words.pop();
    }
    words.join(" ")
}

fn is_version_word(word: &str) -> bool {
    let word = word.trim_start_matches(['v', 'V']).trim_matches(['(', ')']);
    !word.is_empty() && word.bytes().all(|b| b.is_ascii_digit() || b == b'.')
}

fn file_stem(path: &str) -> &str {
    let name = path.rsplit(['\\', '/']).next().unwrap_or(path);
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

/// What a shared host runs, from its command line: the jar, module, main class, or script.
/// `None` when the host is not shared or its command line names nothing.
pub fn host_script(host_stem: &str, command_line: &str) -> Option<String> {
    let host = host_stem.to_ascii_lowercase();
    let args = split_command_line(command_line);
    let mut args = args.iter().skip(1).map(String::as_str);
    let found = match host.as_str() {
        "java" | "javaw" => {
            const WITH_VALUE: &[&str] = &["-cp", "-classpath", "--class-path", "-p", "--module-path",
                "--add-modules", "--add-opens", "--add-exports", "-splash"];
            let mut found = None;
            while let Some(arg) = args.next() {
                match arg {
                    "-jar" => { found = args.next().map(|jar| format!("jar:{}", script_path(jar))); break; }
                    "-m" | "--module" => { found = args.next().map(|m| format!("module:{}", m.to_lowercase())); break; }
                    _ if WITH_VALUE.contains(&arg) => { args.next(); }
                    _ if arg.starts_with('-') => {}
                    class => { found = Some(format!("class:{}", class.to_lowercase())); break; }
                }
            }
            found
        }
        "python" | "pythonw" | "py" | "pyw" => {
            let mut found = None;
            while let Some(arg) = args.next() {
                match arg {
                    "-m" => { found = args.next().map(|m| format!("module:{}", m.to_lowercase())); break; }
                    "-c" => { found = Some("inline".to_string()); break; }
                    "-W" | "-X" | "-Q" => { args.next(); }
                    _ if arg.starts_with('-') => {}
                    script => { found = Some(script_path(script)); break; }
                }
            }
            found
        }
        "node" | "electron" => {
            const WITH_VALUE: &[&str] = &["-r", "--require", "--loader", "--import", "--eval", "-e"];
            let mut found = None;
            while let Some(arg) = args.next() {
                match arg {
                    _ if WITH_VALUE.contains(&arg) => { args.next(); }
                    _ if arg.starts_with('-') => {}
                    script => { found = Some(script_path(script)); break; }
                }
            }
            found
        }
        "wscript" | "cscript" | "mshta" => args.find(|arg| !arg.starts_with('/')).map(script_path),
        "rundll32" => args.next().map(|entry| {
            let (dll, rest) = entry.split_once(',').unwrap_or((entry, ""));
            let dll = dll.rsplit(['\\', '/']).next().unwrap_or(dll);
            format!("{},{}", dll, rest.trim()).to_lowercase()
        }),
        "dllhost" => args.find_map(|arg| {
            let lower = arg.to_ascii_lowercase();
            lower.strip_prefix("/processid:").map(str::to_string)
        }),
        _ => None,
    };
    found.filter(|script| !script.is_empty())
}

/// The name people would call a hosted app: the script or jar's file name, or its folder
/// when the file is a generic entry point (`__main__.py`, `index.js`).
pub fn host_script_label(script: &str) -> String {
    const GENERIC: &[&str] = &["__main__", "main", "index", "app", "run", "start"];
    if let Some(module) = script.strip_prefix("module:") {
        return module.to_string();
    }
    if let Some(class) = script.strip_prefix("class:") {
        return class.rsplit('.').next().unwrap_or(class).to_string();
    }
    let script = script.strip_prefix("jar:").unwrap_or(script);
    let mut parts = script.rsplit(['\\', '/']).filter(|part| !part.is_empty());
    let name = parts.next().unwrap_or(script);
    let stem = name.rsplit_once('.').filter(|(stem, _)| !stem.is_empty()).map_or(name, |(stem, _)| stem);
    match parts.next() {
        Some(folder) if GENERIC.contains(&stem) => folder.to_string(),
        _ => stem.to_string(),
    }
}

fn script_path(arg: &str) -> String {
    if arg.contains(['\\', '/']) { normalize_path(arg) } else { arg.trim_matches('"').to_lowercase() }
}

/// Windows command-line splitting, enough for program arguments: spaces separate, double
/// quotes group.
fn split_command_line(line: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut any = false;
    for ch in line.chars() {
        match ch {
            '"' => { quoted = !quoted; any = true; }
            ch if ch.is_whitespace() && !quoted => {
                if any { args.push(std::mem::take(&mut current)); any = false; }
            }
            ch => { current.push(ch); any = true; }
        }
    }
    if any {
        args.push(current);
    }
    args
}

/// Identities and the app keys they were given. A new identity takes its bare stem only when
/// nothing holds it, it is not a shared host or a Supervisor key, and the stem's history fits
/// (the caller compares labels). Otherwise it gets `stem#2`, `stem#3`... Two identities never
/// share a key.
#[derive(Debug, Default)]
pub struct AppKeys {
    by_identity: HashMap<String, String>,
    taken: HashSet<String>,
}

impl AppKeys {
    /// Adds a saved assignment. Returns false (and adds nothing) if the key is already used.
    pub fn insert(&mut self, identity: &str, key: &str) -> bool {
        if self.taken.contains(key) || self.by_identity.contains_key(identity) {
            return false;
        }
        self.taken.insert(key.to_string());
        self.by_identity.insert(identity.to_string(), key.to_string());
        true
    }

    pub fn get(&self, identity: &str) -> Option<&str> {
        self.by_identity.get(identity).map(String::as_str)
    }

    /// Keys claimed by more than one identity's stem, which a stem alone can't tell apart.
    pub fn stem_is_shared(&self, stem: &str) -> bool {
        self.taken.iter().filter(|key| self::stem(key) == stem).count() > 1
    }

    /// The key for a new identity. Call `get` first; this always assigns.
    pub fn claim(&mut self, identity: &str, stem: &str, stem_history_fits: bool) -> String {
        let stem = clean_stem(stem);
        let bare = !is_shared_host(&stem)
            && !RESERVED_KEYS.contains(&stem.as_str())
            && !self.taken.contains(&stem)
            && stem_history_fits;
        let key = if bare {
            stem
        } else {
            (2..).map(|n| format!("{stem}#{n}")).find(|key| !self.taken.contains(key)).expect("a free key")
        };
        self.insert(identity, &key);
        key
    }
}

/// Lowercase, never empty, and free of the `#` that marks a numbered key.
pub fn clean_stem(stem: &str) -> String {
    let stem = stem.trim().to_lowercase().replace('#', "_");
    if stem.is_empty() { "app".to_string() } else { stem }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(company: &str, product: &str) -> VersionNames {
        VersionNames { company: Some(company.into()), product: Some(product.into()) }
    }

    #[test]
    fn store_apps_keep_one_identity_across_updates_and_never_share_one() {
        let aumid = "SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify";
        let old = windows_identity(r"C:\Program Files\WindowsApps\SpotifyAB.SpotifyMusic_1.2.3.0_x64__zpdnekdrzrea0\Spotify.exe", Some(aumid), None, None);
        let new = windows_identity(r"C:\Program Files\WindowsApps\SpotifyAB.SpotifyMusic_1.3.0.0_x64__zpdnekdrzrea0\Spotify.exe", Some(aumid), None, None);
        assert_eq!(old, new);
        // Two packages that both ship an App.exe.
        let a = windows_identity(r"C:\Program Files\WindowsApps\A_1.0_x64__x\App.exe", Some("A_x!App"), None, None);
        let b = windows_identity(r"C:\Program Files\WindowsApps\B_1.0_x64__y\App.exe", Some("B_y!App"), None, None);
        assert_ne!(a, b);
    }

    #[test]
    fn same_program_name_from_different_makers_is_two_apps() {
        let a = windows_identity(r"C:\Games\Alpha\launcher.exe", None, Some(&named("Alpha Ltd", "Alpha")), None);
        let b = windows_identity(r"C:\Games\Beta\launcher.exe", None, Some(&named("Beta Inc", "Beta")), None);
        assert_ne!(a, b);
        // Moved or updated, it is still the same app.
        let moved = windows_identity(r"D:\Alpha\launcher.exe", None, Some(&named("Alpha Ltd", "Alpha")), None);
        assert_eq!(a, moved);
        // One maker's two programs stay apart (Word and Excel are both "Microsoft Office").
        let word = windows_identity(r"C:\Office\WINWORD.EXE", None, Some(&named("Microsoft", "Microsoft Office")), None);
        let excel = windows_identity(r"C:\Office\EXCEL.EXE", None, Some(&named("Microsoft", "Microsoft Office")), None);
        assert_ne!(word, excel);
    }

    #[test]
    fn electron_apps_with_default_version_info_use_their_path() {
        let electron = named("GitHub, Inc.", "Electron");
        let a = windows_identity(r"C:\Users\me\AppData\Local\Programs\notes\app.exe", None, Some(&electron), None);
        let b = windows_identity(r"C:\Users\me\AppData\Local\Programs\chat\app.exe", None, Some(&electron), None);
        assert_ne!(a, b);
        assert!(a.starts_with("path:"), "{a}");
    }

    #[test]
    fn squirrel_update_folders_are_the_same_program() {
        let old = windows_identity(r"C:\Users\me\AppData\Local\Discord\app-1.0.9028\Discord.exe", None, None, None);
        let new = windows_identity(r"C:\Users\me\AppData\Local\Discord\app-1.0.9030\Discord.exe", None, None, None);
        assert_eq!(old, new);
        assert_eq!(normalize_path(r"\\?\C:\Tools\17.0.2\bin\x.exe"), r"c:\tools\*\bin\x.exe");
        assert_eq!(normalize_path(r"C:\node_modules\Office16\x.exe"), r"c:\node_modules\office16\x.exe");
    }

    #[test]
    fn yearly_product_names_are_one_product() {
        let a = windows_identity(r"C:\Adobe\Adobe Photoshop 2024\Photoshop.exe", None, Some(&named("Adobe", "Adobe Photoshop 2024")), None);
        let b = windows_identity(r"C:\Adobe\Adobe Photoshop 2025\Photoshop.exe", None, Some(&named("Adobe", "Adobe Photoshop 2025")), None);
        assert_eq!(a, b);
        assert_eq!(product_family("7-zip"), "7-zip");
    }

    #[test]
    fn hosts_are_told_apart_by_what_they_run() {
        let java = named("Oracle Corporation", "Java(TM) Platform SE 17");
        let jar = |cmd: &str| {
            let script = host_script("javaw", cmd);
            windows_identity(r"C:\Program Files\Java\jdk-17\bin\javaw.exe", None, Some(&java), script.as_deref())
        };
        let a = jar(r#""C:\Program Files\Java\jdk-17\bin\javaw.exe" -Xmx2G -jar "C:\Apps\Planner.jar""#);
        let b = jar(r#"javaw.exe -jar C:\Apps\Sketch.jar"#);
        assert_ne!(a, b);
        assert_eq!(a, jar(r#"javaw -jar "C:\Apps\Planner.jar" --open today"#));
        assert_eq!(host_script("java", "java -cp lib/* com.example.Main arg").as_deref(), Some("class:com.example.main"));
        assert_eq!(host_script("python", "python.exe -u -m http.server").as_deref(), Some("module:http.server"));
        assert_eq!(host_script("pythonw", r#"pythonw.exe "C:\Tools\notes\notes.pyw""#).as_deref(), Some(r"c:\tools\notes\notes.pyw"));
        assert_eq!(host_script("py", "py -3.12 game.py").as_deref(), Some("game.py"));
        assert_eq!(host_script("electron", "electron.exe --inspect .").as_deref(), Some("."));
        assert_eq!(host_script("rundll32", r"rundll32.exe C:\Windows\System32\shell32.dll,Control_RunDLL").as_deref(), Some("shell32.dll,control_rundll"));
        assert_eq!(host_script("javaw", "javaw.exe"), None);
        // An app that is not a host keeps one identity whatever its arguments.
        assert_eq!(
            windows_identity(r"C:\Code\Code.exe", None, Some(&named("Microsoft", "Visual Studio Code")), Some("x")),
            windows_identity(r"C:\Code\Code.exe", None, Some(&named("Microsoft", "Visual Studio Code")), None),
        );
        assert_eq!(host_script_label(r"c:\tools\notes\notes.pyw"), "notes");
        assert_eq!(host_script_label(r"c:\tools\planner\__main__.py"), "planner");
        assert_eq!(host_script_label(r"jar:c:\apps\planner.jar"), "planner");
        assert_eq!(host_script_label("module:http.server"), "http.server");
        assert_eq!(host_script_label("class:com.example.main"), "main");
    }

    #[test]
    fn macos_apps_are_their_bundle_id() {
        // Renamed between versions, still one app.
        assert_eq!(
            macos_identity(Some("com.microsoft.VSCode"), "/Applications/Visual Studio Code.app"),
            macos_identity(Some("com.microsoft.vscode"), "/Applications/Code.app"),
        );
        // Same name, different bundles.
        assert_ne!(
            macos_identity(Some("com.a.notes"), "/Applications/Notes.app"),
            macos_identity(Some("com.apple.Notes"), "/System/Applications/Notes.app"),
        );
        assert_eq!(macos_identity(None, "/Applications/Tool.app"), "path:\\applications\\tool.app");
    }

    #[test]
    fn distinct_identities_never_share_a_key() {
        let mut keys = AppKeys::default();
        assert_eq!(keys.claim("exe:app|alpha|alpha", "App", true), "app");
        assert_eq!(keys.claim("exe:app|beta|beta", "app", true), "app#2");
        assert_eq!(keys.claim("exe:app|gamma|gamma", "app", true), "app#3");
        assert!(keys.stem_is_shared("app"));
        assert_eq!(stem("app#3"), "app");
        assert_eq!(display_label("app#2", "App"), "App (2)");
        assert_eq!(display_label("app", "App"), "App");
    }

    #[test]
    fn supervisor_keys_and_shared_hosts_are_never_inherited() {
        let mut keys = AppKeys::default();
        assert_eq!(keys.claim("path:c:\\other\\supervisor.exe", "Supervisor", true), "supervisor#2");
        assert_eq!(keys.claim("path:c:\\x\\untracked.exe", "untracked", true), "untracked#2");
        assert_eq!(keys.claim("exe:javaw|oracle|java|jar:a", "javaw", true), "javaw#2");
        assert_eq!(keys.claim("exe:javaw|oracle|java|jar:b", "javaw", true), "javaw#3");
    }

    #[test]
    fn a_stem_whose_history_is_another_app_is_not_taken() {
        let mut keys = AppKeys::default();
        assert_eq!(keys.claim("exe:tool|new|new", "tool", false), "tool#2");
        // The app that did use it shows up later and takes it.
        assert_eq!(keys.claim("exe:tool|old|old", "tool", true), "tool");
    }

    #[test]
    fn saved_assignments_survive_a_restart() {
        let mut first = AppKeys::default();
        let a = first.claim("aumid:a!app", "app", true);
        let b = first.claim("aumid:b!app", "app", true);
        let mut reopened = AppKeys::default();
        // Loaded in any order, every identity gets its key back and nothing is reassigned.
        assert!(reopened.insert("aumid:b!app", &b));
        assert!(reopened.insert("aumid:a!app", &a));
        assert!(!reopened.insert("aumid:c!app", &a));
        assert_eq!((reopened.get("aumid:a!app"), reopened.get("aumid:b!app")), (Some("app"), Some("app#2")));
        assert_eq!(reopened.claim("aumid:c!app", "app", true), "app#3");
    }
}
