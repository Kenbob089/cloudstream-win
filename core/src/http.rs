use std::collections::HashMap;
use std::fmt;
use std::io::Read;
use std::sync::Mutex;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetError {
    InvalidUrl(String),
    Insecure(String),
    BadStatus(u16),
    TooLarge,
    Offline,
    Timeout,
    Transport(String),
}

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl(s) => write!(f, "\u{201c}{s}\u{201d} is not a valid URL."),
            Self::Insecure(s) => write!(f, "Only HTTPS addresses are allowed ({s})."),
            Self::BadStatus(c) => write!(f, "The server responded with status {c}."),
            Self::TooLarge => write!(f, "The response was larger than the allowed size."),
            Self::Offline => write!(f, "You appear to be offline, or the host is unreachable."),
            Self::Timeout => write!(f, "The request timed out."),
            Self::Transport(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for NetError {}

/// Abstraction over the HTTP stack so parsers and providers are testable.
pub trait HttpClient: Send + Sync {
    fn get(&self, url: &str) -> Result<Vec<u8>, NetError>;
}

/// Returns a normalized https URL, or None. Rejects other schemes, hostless URLs and credentials.
/// Use for every URL that comes from untrusted repository metadata.
pub fn secure_url(s: &str) -> Option<String> {
    let u = url::Url::parse(s.trim()).ok()?;
    if u.scheme() != "https" || u.host_str().is_none() || !u.username().is_empty() || u.password().is_some() {
        return None;
    }
    Some(u.to_string())
}

pub fn require_secure(s: &str) -> Result<String, NetError> {
    let trimmed = s.trim();
    let u = url::Url::parse(trimmed).map_err(|_| NetError::InvalidUrl(trimmed.to_string()))?;
    if u.host_str().is_none() {
        return Err(NetError::InvalidUrl(trimmed.to_string()));
    }
    if u.scheme() != "https" {
        return Err(NetError::Insecure(trimmed.to_string()));
    }
    if !u.username().is_empty() || u.password().is_some() {
        return Err(NetError::InvalidUrl(trimmed.to_string()));
    }
    Ok(u.to_string())
}

/// Production client: timeouts, status validation, size cap, https-only manual redirects.
pub struct UreqClient {
    agent: ureq::Agent,
    max_bytes: u64,
}

impl UreqClient {
    pub fn new() -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .redirects(0) // followed manually so every hop can be checked for https
            .user_agent("CloudStreamWindows/1.0")
            .build();
        Self { agent, max_bytes: 16 * 1024 * 1024 }
    }
}

impl Default for UreqClient {
    fn default() -> Self {
        Self::new()
    }
}

fn map_transport(t: &ureq::Transport) -> NetError {
    let msg = t.to_string();
    if msg.to_lowercase().contains("timed out") {
        return NetError::Timeout;
    }
    match t.kind() {
        ureq::ErrorKind::Dns | ureq::ErrorKind::ConnectionFailed => NetError::Offline,
        _ => NetError::Transport(msg),
    }
}

impl HttpClient for UreqClient {
    fn get(&self, url: &str) -> Result<Vec<u8>, NetError> {
        let mut current = require_secure(url)?;
        for _ in 0..=5 {
            match self.agent.get(&current).call() {
                Ok(resp) => {
                    let status = resp.status();
                    if (300..400).contains(&status) {
                        let next = {
                            let loc = resp.header("location").ok_or(NetError::BadStatus(status))?;
                            let base = url::Url::parse(&current).map_err(|_| NetError::InvalidUrl(current.clone()))?;
                            base.join(loc).map_err(|_| NetError::InvalidUrl(loc.to_string()))?
                        };
                        current = require_secure(next.as_str())?;
                        continue;
                    }
                    let mut buf = Vec::new();
                    resp.into_reader()
                        .take(self.max_bytes + 1)
                        .read_to_end(&mut buf)
                        .map_err(|e| NetError::Transport(e.to_string()))?;
                    if buf.len() as u64 > self.max_bytes {
                        return Err(NetError::TooLarge);
                    }
                    return Ok(buf);
                }
                Err(ureq::Error::Status(code, _)) => return Err(NetError::BadStatus(code)),
                Err(ureq::Error::Transport(t)) => return Err(map_transport(&t)),
            }
        }
        Err(NetError::Transport("Too many redirects.".into()))
    }
}

/// Deterministic in-memory client used by tests (kept in the library so integration tests can use it).
#[derive(Default)]
pub struct MockClient {
    responses: Mutex<HashMap<String, Result<Vec<u8>, NetError>>>,
    requested: Mutex<Vec<String>>,
}

impl MockClient {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn set(&self, url: &str, body: &str) {
        self.responses.lock().unwrap().insert(url.to_string(), Ok(body.as_bytes().to_vec()));
    }
    pub fn set_err(&self, url: &str, err: NetError) {
        self.responses.lock().unwrap().insert(url.to_string(), Err(err));
    }
    pub fn requested(&self) -> Vec<String> {
        self.requested.lock().unwrap().clone()
    }
}

impl HttpClient for MockClient {
    fn get(&self, url: &str) -> Result<Vec<u8>, NetError> {
        self.requested.lock().unwrap().push(url.to_string());
        match self.responses.lock().unwrap().get(url) {
            Some(r) => r.clone(),
            None => Err(NetError::BadStatus(404)),
        }
    }
}
