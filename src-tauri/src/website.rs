//! Website identity: which site a browser address belongs to, and what to call it.
//! Pure and platform-free; capture code only supplies the address text.

use std::net::IpAddr;
use url::{Host, Url};

/// One row for localhost, private IPs, and intranet names.
pub const LOCAL_NETWORK: &str = "local-network";

/// Windows exe stems and macOS app names, lowercased. Thorium is a book app, not a browser.
pub const BROWSERS: &[&str] = &[
    "chrome",
    "chromium",
    "msedge",
    "firefox",
    "brave",
    "opera",
    "vivaldi",
    "arc",
    "google chrome",
    "microsoft edge",
    "brave browser",
    "safari",
    // The other builds macOS names by their own app names (on Windows they share an exe).
    "google chrome beta",
    "google chrome dev",
    "google chrome canary",
    "microsoft edge beta",
    "microsoft edge dev",
    "microsoft edge canary",
    "brave browser beta",
    "brave browser nightly",
    "firefox developer edition",
    "firefox nightly",
    "safari technology preview",
    "opera gx",
    "zen browser",
    "orion",
    "duckduckgo",
];

/// Browser New Tab titles, after the browser-name suffix is stripped. Safari calls it Start Page.
const NEW_TAB_TITLES: &[&str] = &["new tab", "start page"];

/// Schemes a browser shows for its own pages.
const BROWSER_PAGE_SCHEMES: &[&str] = &[
    "about",
    "chrome",
    "chrome-search",
    "chrome-untrusted",
    "edge",
    "brave",
    "opera",
    "vivaldi",
    "arc",
];

/// Non-web addresses: shown under the browser, never as a website.
const LOCAL_SCHEMES: &[&str] = &[
    "file",
    "data",
    "blob",
    "view-source",
    "chrome-extension",
    "extension",
    "moz-extension",
    "devtools",
];

/// Subdomains that are distinct products. Every other subdomain joins its registrable domain.
const SERVICE_HOSTS: &[&str] = &["mail.google.com", "docs.google.com", "drive.google.com"];

/// Other domains that are the same site.
const ALIASES: &[(&str, &str)] = &[
    ("youtu.be", "youtube.com"),
    ("twitter.com", "x.com"),
    ("redd.it", "reddit.com"),
    ("fb.com", "facebook.com"),
];

/// Hand-verified labels. A title can name one of these sites only by the whole label.
/// Names people mostly use as desktop apps (Slack, Discord, Spotify) stay out, so a bare
/// "Spotify" rule means the app.
const SITES: &[(&str, &str)] = &[
    ("github.com", "GitHub"),
    ("stackoverflow.com", "Stack Overflow"),
    ("docs.google.com", "Google Docs"),
    ("drive.google.com", "Google Drive"),
    ("chatgpt.com", "ChatGPT"),
    ("wikipedia.org", "Wikipedia"),
    ("mail.google.com", "Gmail"),
    ("linkedin.com", "LinkedIn"),
    ("youtube.com", "YouTube"),
    ("netflix.com", "Netflix"),
    ("twitch.tv", "Twitch"),
    ("tiktok.com", "TikTok"),
    ("instagram.com", "Instagram"),
    ("reddit.com", "Reddit"),
    ("facebook.com", "Facebook"),
    ("x.com", "X"),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Address {
    Website(String),
    BrowserPage,
    Local,
    /// Search text, a partial entry, or anything else that is not a committed address.
    NotAddress,
}

/// `chrome#2` (a second Chrome install, such as Canary) is a browser too.
pub fn is_browser(app_id: &str) -> bool {
    BROWSERS.contains(&crate::app_identity::stem(app_id))
}

pub fn is_new_tab_title(title: &str) -> bool {
    NEW_TAB_TITLES.iter().any(|known| title.trim().eq_ignore_ascii_case(known))
}

pub fn classify_address(text: &str) -> Address {
    let text = text.trim();
    if text.is_empty() || text.chars().any(char::is_whitespace) {
        return Address::NotAddress;
    }
    // Address bars often hide the scheme ("github.com/openai"). `localhost:3000` must not be
    // read as a "localhost" scheme, so only known schemes are taken as written.
    let parsed = Url::parse(text).ok();
    // A browser page with a scheme not listed ("favorites://", a new browser's own) is still
    // the browser's own page, not a site.
    if let Some(url) = parsed.as_ref().filter(|url| !is_known_scheme(url.scheme()) && text.contains("://")) {
        if !matches!(url.scheme(), "http" | "https") {
            return Address::BrowserPage;
        }
    }
    let parsed = parsed.filter(|url| is_known_scheme(url.scheme()));
    let (url, typed_without_scheme) = match parsed {
        Some(url) => (url, false),
        None => match Url::parse(&format!("https://{text}")) {
            Ok(url) => (url, true),
            Err(_) => return Address::NotAddress,
        },
    };

    match url.scheme() {
        "http" | "https" => {}
        scheme if BROWSER_PAGE_SCHEMES.contains(&scheme) => return Address::BrowserPage,
        _ => return Address::Local,
    }
    let Some(host) = url.host() else {
        return Address::NotAddress;
    };
    if typed_without_scheme && !looks_like_host(&host) {
        return Address::NotAddress;
    }
    Address::Website(site_key(host))
}

/// Rule input: an address, a host, or a verified site name. `None` means it is not a website.
pub fn website_from_input(text: &str) -> Option<String> {
    match classify_address(text) {
        Address::Website(key) => Some(key),
        _ => site_for_label(text).map(str::to_string),
    }
}

pub fn site_label(key: &str) -> String {
    if key == LOCAL_NETWORK {
        return "Local network".to_string();
    }
    SITES
        .iter()
        .find(|(site, _)| *site == key)
        .map(|(_, label)| label.to_string())
        .unwrap_or_else(|| key.to_string())
}

/// The built-in names, for sites the user has not named.
pub fn preset_names() -> impl Iterator<Item = (&'static str, &'static str)> {
    SITES.iter().copied()
}

/// Fallback when the address cannot be read: one outer title segment must equal a verified
/// site label. Two brand-like ends, or none, mean the site is unknown.
pub fn site_from_title(title: &str) -> Option<&'static str> {
    let title = title.trim();
    let Some((first, last)) = outer_segments(title) else {
        return site_for_label(title);
    };
    match (site_for_label(first), site_for_label(last)) {
        (Some(site), None) | (None, Some(site)) => Some(site),
        _ => None,
    }
}

fn outer_segments(title: &str) -> Option<(&str, &str)> {
    const SEPARATORS: &[&str] = &[" - ", " — ", " – ", " | "];
    let first_end = SEPARATORS.iter().filter_map(|sep| title.find(sep)).min()?;
    let last_start = SEPARATORS
        .iter()
        .filter_map(|sep| title.rfind(sep).map(|at| at + sep.len()))
        .max()?;
    Some((&title[..first_end], &title[last_start..]))
}

fn site_for_label(text: &str) -> Option<&'static str> {
    let text = text.trim();
    // One- and two-letter labels ("X") are too easily ordinary words.
    if text.chars().count() < 3 {
        return None;
    }
    SITES
        .iter()
        .find(|(_, label)| label.eq_ignore_ascii_case(text))
        .map(|(site, _)| *site)
}

fn is_known_scheme(scheme: &str) -> bool {
    matches!(scheme, "http" | "https")
        || BROWSER_PAGE_SCHEMES.contains(&scheme)
        || LOCAL_SCHEMES.contains(&scheme)
}

fn looks_like_host(host: &Host<&str>) -> bool {
    match host {
        Host::Domain(domain) => domain.contains('.') || *domain == "localhost",
        Host::Ipv4(_) | Host::Ipv6(_) => true,
    }
}

fn site_key(host: Host<&str>) -> String {
    match host {
        Host::Ipv4(ip) => ip_key(IpAddr::V4(ip)),
        Host::Ipv6(ip) => ip_key(IpAddr::V6(ip)),
        Host::Domain(domain) => {
            let domain = domain.trim_end_matches('.');
            if is_local_name(domain) {
                return LOCAL_NETWORK.to_string();
            }
            if SERVICE_HOSTS.contains(&domain) {
                return domain.to_string();
            }
            let site = psl::domain_str(domain).unwrap_or(domain);
            ALIASES
                .iter()
                .find(|(alias, _)| *alias == site)
                .map_or(site, |(_, canonical)| canonical)
                .to_string()
        }
    }
}

fn is_local_name(domain: &str) -> bool {
    const LOCAL_SUFFIXES: &[&str] = &[".localhost", ".local", ".lan", ".internal", ".home.arpa"];
    domain == "localhost"
        || !domain.contains('.')
        || LOCAL_SUFFIXES.iter().any(|suffix| domain.ends_with(suffix))
}

fn ip_key(ip: IpAddr) -> String {
    let local = match ip {
        IpAddr::V4(ip) => ip.is_private() || ip.is_loopback() || ip.is_link_local(),
        IpAddr::V6(ip) => {
            let first = ip.segments()[0];
            ip.is_loopback() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
        }
    };
    if local {
        LOCAL_NETWORK.to_string()
    } else {
        ip.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(text: &str) -> Address {
        Address::Website(text.to_string())
    }

    #[test]
    fn a_browser_scheme_not_listed_is_the_browser_itself() {
        assert_eq!(classify_address("favorites://"), Address::BrowserPage);
        assert_eq!(classify_address("orion://settings"), Address::BrowserPage);
        // Not a scheme: a host with a port stays a site.
        assert_eq!(classify_address("localhost:3000"), site(LOCAL_NETWORK));
        assert!(is_browser("google chrome canary"));
        assert!(is_browser("safari technology preview"));
        assert!(!is_browser("zen"));
    }

    #[test]
    fn all_pages_on_a_site_share_its_key() {
        assert_eq!(classify_address("github.com/openai/codex/issues/42"), site("github.com"));
        assert_eq!(classify_address("https://github.com/openai/codex/pulls"), site("github.com"));
        assert_eq!(classify_address("https://www.youtube.com/watch?v=1"), site("youtube.com"));
        assert_eq!(classify_address("m.youtube.com"), site("youtube.com"));
        assert_eq!(classify_address("HTTPS://YouTube.com./feed"), site("youtube.com"));
    }

    #[test]
    fn registrable_domain_comes_from_the_suffix_list() {
        assert_eq!(classify_address("news.bbc.co.uk/sport"), site("bbc.co.uk"));
        assert_eq!(classify_address("alice.github.io/blog"), site("alice.github.io"));
    }

    #[test]
    fn alternate_domains_join_their_site() {
        assert_eq!(classify_address("https://youtu.be/abc"), site("youtube.com"));
        assert_eq!(classify_address("twitter.com/someone"), site("x.com"));
    }

    #[test]
    fn distinct_google_products_stay_separate() {
        assert_eq!(classify_address("mail.google.com/mail/u/0"), site("mail.google.com"));
        assert_eq!(classify_address("docs.google.com/document/d/1"), site("docs.google.com"));
        assert_eq!(classify_address("drive.google.com"), site("drive.google.com"));
        assert_eq!(classify_address("www.google.com/search?q=x"), site("google.com"));
        assert_eq!(site_label("mail.google.com"), "Gmail");
    }

    #[test]
    fn local_and_ip_hosts_are_explicit() {
        assert_eq!(classify_address("localhost:1420"), site(LOCAL_NETWORK));
        assert_eq!(classify_address("http://192.168.1.10/admin"), site(LOCAL_NETWORK));
        assert_eq!(classify_address("http://intranet/wiki"), site(LOCAL_NETWORK));
        assert_eq!(classify_address("http://[::1]:8080"), site(LOCAL_NETWORK));
        assert_eq!(classify_address("http://8.8.8.8"), site("8.8.8.8"));
    }

    #[test]
    fn internationalized_hosts_use_one_form() {
        assert_eq!(
            classify_address("https://bücher.de/x"),
            classify_address("https://xn--bcher-kva.de/y")
        );
    }

    #[test]
    fn browser_pages_and_non_web_addresses_are_not_websites() {
        assert_eq!(classify_address("chrome://history"), Address::BrowserPage);
        assert_eq!(classify_address("edge://settings/profiles"), Address::BrowserPage);
        assert_eq!(classify_address("about:preferences"), Address::BrowserPage);
        assert_eq!(classify_address("file:///C:/Users/me/notes.html"), Address::Local);
        assert_eq!(classify_address("data:text/html,hi"), Address::Local);
        assert_eq!(classify_address("chrome-extension://abc/popup.html"), Address::Local);
    }

    #[test]
    fn typed_search_text_is_not_an_address() {
        assert_eq!(classify_address("how to center a div"), Address::NotAddress);
        assert_eq!(classify_address("github"), Address::NotAddress);
        assert_eq!(classify_address(""), Address::NotAddress);
    }

    #[test]
    fn title_fallback_needs_one_whole_verified_outer_segment() {
        assert_eq!(site_from_title("Issue #42 - GitHub"), Some("github.com"));
        assert_eq!(site_from_title("GitHub"), Some("github.com"));
        assert_eq!(site_from_title("Article - Something"), None);
        assert_eq!(site_from_title("GitHub Actions - Docs"), None);
        assert_eq!(site_from_title("YouTube - GitHub"), None);
        assert_eq!(site_from_title("Dashboard"), None);
        assert_eq!(site_from_title("Post / X"), None);
    }

    #[test]
    fn rule_input_accepts_addresses_and_verified_names() {
        assert_eq!(website_from_input("YouTube").as_deref(), Some("youtube.com"));
        assert_eq!(website_from_input("https://www.reddit.com/r/rust").as_deref(), Some("reddit.com"));
        assert_eq!(website_from_input("Spotify"), None);
        assert_eq!(website_from_input("spotify.com").as_deref(), Some("spotify.com"));
    }
}
