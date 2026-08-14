use crate::activity::ActivitySample;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const DEFAULT_PORT: u16 = 43177;

const MAX_HEADER_BYTES: usize = 8 * 1024;
const MAX_BODY_BYTES: usize = 16 * 1024;
const FRESH_FOR: Duration = Duration::from_secs(5);
const PORT_FALLBACKS: u16 = 10;
const BRIDGE_HEADER: &str = "X-Supervisor-Browser-Bridge";
const BRIDGE_HEADER_VALUE: &str = "1";

#[derive(Clone, Debug, Default)]
pub struct BrowserBridge {
    latest: Arc<Mutex<Option<StoredBrowserTabSample>>>,
    port: Arc<Mutex<Option<u16>>>,
}

impl BrowserBridge {
    pub fn start(&self) -> Result<u16, String> {
        let listener = bind_listener()?;
        let port = listener
            .local_addr()
            .map_err(|error| error.to_string())?
            .port();
        if let Ok(mut stored_port) = self.port.lock() {
            *stored_port = Some(port);
        }

        let latest = Arc::clone(&self.latest);
        std::thread::Builder::new()
            .name("supervisor-browser-bridge".to_string())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    handle_stream(stream, &latest);
                }
            })
            .map_err(|error| error.to_string())?;

        Ok(port)
    }

    pub fn enrich_activity_sample(&self, sample: ActivitySample) -> ActivitySample {
        let Ok(mut stored) = self.latest.lock() else {
            return sample;
        };
        let Some(stored) = stored.as_mut() else {
            return sample;
        };
        if !stored.is_usable() || !browser_matches_app(&stored.sample, &sample) {
            return sample;
        }

        stored.last_used_at = Instant::now();
        enrich_activity_sample(sample, Some(&stored.sample))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct BrowserTabSample {
    pub browser: String,
    pub tab_id: i64,
    pub window_id: i64,
    pub url: String,
    pub title: Option<String>,
    pub domain: Option<String>,
    pub captured_at: String,
}

#[derive(Clone, Debug)]
struct StoredBrowserTabSample {
    sample: BrowserTabSample,
    received_at: Instant,
    last_used_at: Instant,
}

impl StoredBrowserTabSample {
    fn new(sample: BrowserTabSample) -> Self {
        let now = Instant::now();
        Self {
            sample,
            received_at: now,
            last_used_at: now,
        }
    }

    fn is_usable(&self) -> bool {
        self.received_at.elapsed() <= FRESH_FOR || self.last_used_at.elapsed() <= FRESH_FOR
    }
}

#[derive(Debug, PartialEq)]
struct HttpRequest {
    method: String,
    path: String,
    bridge_header_present: bool,
    body: Vec<u8>,
}

#[derive(Debug, PartialEq)]
struct HttpError {
    status: &'static str,
    message: &'static str,
}

impl HttpError {
    fn new(status: &'static str, message: &'static str) -> Self {
        Self { status, message }
    }
}

fn bind_listener() -> Result<TcpListener, String> {
    for port in DEFAULT_PORT..=DEFAULT_PORT + PORT_FALLBACKS {
        if let Ok(listener) = TcpListener::bind(("127.0.0.1", port)) {
            return Ok(listener);
        }
    }

    Err(format!(
        "could not bind browser bridge on 127.0.0.1:{DEFAULT_PORT}-{}",
        DEFAULT_PORT + PORT_FALLBACKS
    ))
}

fn handle_stream(mut stream: TcpStream, latest: &Arc<Mutex<Option<StoredBrowserTabSample>>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let response =
        match read_http_request(&mut stream).and_then(|request| handle_request(&request, latest)) {
            Ok(()) => http_response("204 No Content", ""),
            Err(error) => http_response(error.status, error.message),
        };

    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn handle_request(
    request: &HttpRequest,
    latest: &Mutex<Option<StoredBrowserTabSample>>,
) -> Result<(), HttpError> {
    if request.path != "/tab" {
        return Err(HttpError::new("404 Not Found", "not found"));
    }
    if request.method == "OPTIONS" {
        return Ok(());
    }
    if request.method != "POST" {
        return Err(HttpError::new(
            "405 Method Not Allowed",
            "method not allowed",
        ));
    }
    if !request.bridge_header_present {
        return Err(HttpError::new("403 Forbidden", "missing bridge header"));
    }

    let sample = serde_json::from_slice::<BrowserTabSample>(&request.body)
        .map_err(|_| HttpError::new("400 Bad Request", "malformed tab sample"))?;
    let sample = clean_tab_sample(sample)?;
    *latest
        .lock()
        .map_err(|_| HttpError::new("500 Internal Server Error", "bridge unavailable"))? =
        Some(StoredBrowserTabSample::new(sample));
    Ok(())
}

fn read_http_request(reader: &mut impl Read) -> Result<HttpRequest, HttpError> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 512];
    let header_end = loop {
        let read = reader
            .read(&mut chunk)
            .map_err(|_| HttpError::new("400 Bad Request", "could not read request"))?;
        if read == 0 {
            return Err(HttpError::new("400 Bad Request", "incomplete request"));
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.len() > MAX_HEADER_BYTES {
            return Err(HttpError::new(
                "431 Request Header Fields Too Large",
                "headers too large",
            ));
        }
        if let Some(index) = find_headers_end(&buffer) {
            break index;
        }
    };

    let header_text = std::str::from_utf8(&buffer[..header_end])
        .map_err(|_| HttpError::new("400 Bad Request", "invalid request headers"))?;
    let mut lines = header_text.lines();
    let request_line = lines
        .next()
        .ok_or_else(|| HttpError::new("400 Bad Request", "missing request line"))?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| HttpError::new("400 Bad Request", "missing method"))?
        .to_string();
    let path = parts
        .next()
        .ok_or_else(|| HttpError::new("400 Bad Request", "missing path"))?
        .to_string();
    let content_length = content_length(header_text)?;
    let bridge_header_present = bridge_header_present(header_text);
    if content_length > MAX_BODY_BYTES {
        return Err(HttpError::new(
            "413 Payload Too Large",
            "tab sample too large",
        ));
    }

    let body_start = header_end + 4;
    let mut body = buffer[body_start..].to_vec();
    while body.len() < content_length {
        let read = reader
            .read(&mut chunk)
            .map_err(|_| HttpError::new("400 Bad Request", "could not read request body"))?;
        if read == 0 {
            return Err(HttpError::new("400 Bad Request", "incomplete request body"));
        }
        body.extend_from_slice(&chunk[..read]);
    }
    body.truncate(content_length);

    Ok(HttpRequest {
        method,
        path,
        bridge_header_present,
        body,
    })
}

fn find_headers_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

fn content_length(header_text: &str) -> Result<usize, HttpError> {
    header_text
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim())
        })
        .unwrap_or("0")
        .parse::<usize>()
        .map_err(|_| HttpError::new("400 Bad Request", "invalid content length"))
}

fn bridge_header_present(header_text: &str) -> bool {
    header_text.lines().any(|line| {
        let Some((name, value)) = line.split_once(':') else {
            return false;
        };
        name.eq_ignore_ascii_case(BRIDGE_HEADER) && value.trim() == BRIDGE_HEADER_VALUE
    })
}

fn http_response(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{BRIDGE_HEADER}: {BRIDGE_HEADER_VALUE}\r\n\r\n{body}",
        body.len()
    )
}

fn clean_tab_sample(mut sample: BrowserTabSample) -> Result<BrowserTabSample, HttpError> {
    sample.browser = clean_required(sample.browser, 64)?;
    sample.url = clean_required(sample.url, 4096)?;
    sample.title = clean_optional(sample.title, 512);
    sample.domain = clean_optional(sample.domain.or_else(|| domain_from_url(&sample.url)), 253);
    sample.captured_at = clean_required(sample.captured_at, 96)?;
    Ok(sample)
}

fn clean_required(value: String, max_chars: usize) -> Result<String, HttpError> {
    let cleaned = value.trim().chars().take(max_chars).collect::<String>();
    if cleaned.is_empty() {
        return Err(HttpError::new("400 Bad Request", "empty tab sample field"));
    }
    Ok(cleaned)
}

fn clean_optional(value: Option<String>, max_chars: usize) -> Option<String> {
    value
        .map(|value| value.trim().chars().take(max_chars).collect::<String>())
        .filter(|value| !value.is_empty())
}

fn enrich_activity_sample(
    mut activity: ActivitySample,
    tab: Option<&BrowserTabSample>,
) -> ActivitySample {
    let Some(tab) = tab else {
        return activity;
    };
    if !browser_matches_app(tab, &activity) {
        return activity;
    }
    if let Some(title) = tab_title(tab) {
        activity.window_title = Some(title);
    }
    activity
}

fn browser_matches_app(tab: &BrowserTabSample, activity: &ActivitySample) -> bool {
    if !tab.browser.trim().eq_ignore_ascii_case("chrome") {
        return false;
    }

    let app = format!("{} {}", activity.app_name, activity.process_path).to_lowercase();

    app.contains("google chrome")
        || app.contains("google-chrome")
        || app.contains("chrome for testing")
        || app.contains("chrome.exe")
}

fn tab_title(tab: &BrowserTabSample) -> Option<String> {
    let domain = tab.domain.as_deref().and_then(clean_display_part);
    let title = tab.title.as_deref().and_then(clean_display_part);

    match (domain, title) {
        (Some(domain), Some(title)) if !domain.eq_ignore_ascii_case(title) => {
            Some(format!("{domain} / {title}"))
        }
        (Some(domain), _) => Some(domain.to_string()),
        (_, Some(title)) => Some(title.to_string()),
        _ => domain_from_url(&tab.url),
    }
}

fn clean_display_part(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

fn domain_from_url(url: &str) -> Option<String> {
    let without_scheme = url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(url)
        .trim_start_matches('/');
    let host = without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .rsplit('@')
        .next()
        .unwrap_or_default()
        .split(':')
        .next()
        .unwrap_or_default()
        .trim();

    (!host.is_empty()).then(|| host.chars().take(253).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::{Bounds, CapturePermissions, PermissionState};
    use std::io::Cursor;

    #[test]
    fn chrome_foreground_sample_gets_tab_title() {
        let sample = enrich_activity_sample(
            activity("Google Chrome", "Old title"),
            Some(&tab(
                "chrome",
                "https://developer.chrome.com/docs",
                "developer.chrome.com",
            )),
        );

        assert_eq!(
            sample.window_title.as_deref(),
            Some("developer.chrome.com / Example docs")
        );
    }

    #[test]
    fn non_chrome_foreground_sample_is_unchanged() {
        let sample = enrich_activity_sample(
            activity("Code", "main.ts"),
            Some(&tab("chrome", "https://youtube.com/watch", "youtube.com")),
        );

        assert_eq!(sample.window_title.as_deref(), Some("main.ts"));
    }

    #[test]
    fn non_chrome_relay_label_is_ignored() {
        let sample = enrich_activity_sample(
            activity("Google Chrome", "Old title"),
            Some(&tab("edge", "https://youtube.com/watch", "youtube.com")),
        );

        assert_eq!(sample.window_title.as_deref(), Some("Old title"));
    }

    #[test]
    fn stale_tab_data_is_ignored() {
        let bridge = BrowserBridge::default();
        *bridge.latest.lock().unwrap() = Some(StoredBrowserTabSample {
            sample: tab("chrome", "https://youtube.com/watch", "youtube.com"),
            received_at: Instant::now() - FRESH_FOR - Duration::from_secs(1),
            last_used_at: Instant::now() - FRESH_FOR - Duration::from_secs(1),
        });

        let sample = bridge.enrich_activity_sample(activity("Google Chrome", "Old title"));

        assert_eq!(sample.window_title.as_deref(), Some("Old title"));
    }

    #[test]
    fn active_chrome_keeps_using_current_tab_sample() {
        let bridge = BrowserBridge::default();
        *bridge.latest.lock().unwrap() = Some(StoredBrowserTabSample {
            sample: tab("chrome", "https://youtube.com/watch", "youtube.com"),
            received_at: Instant::now() - FRESH_FOR - Duration::from_secs(30),
            last_used_at: Instant::now() - Duration::from_secs(1),
        });

        let sample = bridge.enrich_activity_sample(activity("Google Chrome", "Old title"));

        assert_eq!(
            sample.window_title.as_deref(),
            Some("youtube.com / Example docs")
        );
    }

    #[test]
    fn malformed_payload_is_rejected() {
        let latest = Mutex::new(None);
        let error = handle_request(&request("POST", "/tab", true, b"{"), &latest).unwrap_err();

        assert_eq!(error.status, "400 Bad Request");
        assert!(latest.lock().unwrap().is_none());
    }

    #[test]
    fn post_without_bridge_header_is_rejected() {
        let latest = Mutex::new(None);
        let error = handle_request(
            &request("POST", "/tab", false, br#"{"browser":"chrome"}"#),
            &latest,
        )
        .unwrap_err();

        assert_eq!(error.status, "403 Forbidden");
        assert!(latest.lock().unwrap().is_none());
    }

    #[test]
    fn preflight_does_not_store_payload() {
        let latest = Mutex::new(None);

        handle_request(&request("OPTIONS", "/tab", false, b""), &latest).unwrap();

        assert!(latest.lock().unwrap().is_none());
    }

    #[test]
    fn oversized_payload_is_rejected() {
        let request = format!(
            "POST /tab HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            MAX_BODY_BYTES + 1
        );
        let error = read_http_request(&mut Cursor::new(request.into_bytes())).unwrap_err();

        assert_eq!(error.status, "413 Payload Too Large");
    }

    #[test]
    fn bridge_response_has_fingerprint_header() {
        let response = http_response("204 No Content", "");

        assert!(response.contains("X-Supervisor-Browser-Bridge: 1\r\n"));
        assert!(!response.contains("Access-Control-Allow-Origin"));
    }

    #[test]
    fn request_header_is_parsed_case_insensitively() {
        let request =
            "POST /tab HTTP/1.1\r\nx-supervisor-browser-bridge: 1\r\nContent-Length: 0\r\n\r\n";
        let parsed = read_http_request(&mut Cursor::new(request.as_bytes())).unwrap();

        assert!(parsed.bridge_header_present);
    }

    fn activity(app_name: &str, title: &str) -> ActivitySample {
        ActivitySample {
            captured_at: "2026-08-13T00:00:00Z".to_string(),
            platform: "macos",
            app_name: app_name.to_string(),
            process_path: format!("/Applications/{app_name}.app"),
            process_id: 42,
            window_title: Some(title.to_string()),
            window_id: "window-1".to_string(),
            bounds: Bounds {
                x: 0.0,
                y: 0.0,
                width: 1440.0,
                height: 900.0,
            },
            idle_seconds: Some(0.0),
            permissions: CapturePermissions {
                screen_recording: PermissionState::Granted,
            },
        }
    }

    fn tab(browser: &str, url: &str, domain: &str) -> BrowserTabSample {
        BrowserTabSample {
            browser: browser.to_string(),
            tab_id: 1,
            window_id: 1,
            url: url.to_string(),
            title: Some("Example docs".to_string()),
            domain: Some(domain.to_string()),
            captured_at: "2026-08-13T00:00:00Z".to_string(),
        }
    }

    fn request(method: &str, path: &str, bridge_header_present: bool, body: &[u8]) -> HttpRequest {
        HttpRequest {
            method: method.to_string(),
            path: path.to_string(),
            bridge_header_present,
            body: body.to_vec(),
        }
    }
}
