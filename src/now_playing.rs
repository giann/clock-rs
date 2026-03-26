use std::{
    process::{Command, Output, Stdio},
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::{Duration, Instant},
};

use crate::config::NowPlayingConfig;

pub struct NowPlaying {
    refresh_interval: Duration,
    line: Option<String>,
    last_fetch: Option<Instant>,
    in_flight: Option<Receiver<Result<Option<String>, String>>>,
}

impl NowPlaying {
    const DISPLAY_ERROR_MAX_LEN: usize = 80;
    const COMMAND_TIMEOUT: Duration = Duration::from_millis(900);

    pub fn from_config(config: NowPlayingConfig) -> Option<Self> {
        if !config.enabled {
            return None;
        }

        Some(Self {
            refresh_interval: Duration::from_secs(config.refresh_interval_seconds.max(1)),
            line: None,
            last_fetch: None,
            in_flight: None,
        })
    }

    pub fn line(&self) -> Option<&str> {
        self.line.as_deref()
    }

    pub fn update_if_due(&mut self) {
        let polled = self.in_flight.as_ref().map(Receiver::try_recv);

        match polled {
            Some(Ok(Ok(Some(track)))) => {
                self.line = Some(format!("♪ {track}"));
                self.in_flight = None;
            }
            Some(Ok(Ok(None))) => {
                self.line = Some("♪ No song playing".to_string());
                self.in_flight = None;
            }
            Some(Ok(Err(err))) => {
                self.line = Some(Self::format_display_error(&err));
                self.in_flight = None;
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.line = Some(Self::format_display_error(
                    "background updater disconnected",
                ));
                self.in_flight = None;
            }
            Some(Err(TryRecvError::Empty)) => return,
            None => (),
        }

        if self.in_flight.is_some() {
            return;
        }

        if self
            .last_fetch
            .is_some_and(|last_fetch| last_fetch.elapsed() < self.refresh_interval)
        {
            return;
        }

        self.last_fetch = Some(Instant::now());
        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            let _ = tx.send(Self::fetch_line());
        });
        self.in_flight = Some(rx);
    }

    pub fn is_loading(&self) -> bool {
        self.in_flight.is_some()
    }

    #[cfg(target_os = "macos")]
    fn fetch_line() -> Result<Option<String>, String> {
        const SCRIPT: &str = r#"
set outputText to ""
try
    if application "Music" is running then
        tell application "Music"
            if player state is playing then
                set outputText to name of current track & " - " & artist of current track
            end if
        end tell
    end if
end try

if outputText is "" then
    try
        if application "Spotify" is running then
            tell application "Spotify"
                if player state is playing then
                    set outputText to name of current track & " - " & artist of current track
                end if
            end tell
        end if
    end try
end if

return outputText
"#;

        let output = Self::run_osascript_with_timeout(SCRIPT)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let reason = if !stderr.is_empty() {
                stderr
            } else if !stdout.is_empty() {
                stdout
            } else {
                format!("osascript exited with status {}", output.status)
            };

            return Err(reason);
        }

        let line = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if line.is_empty() {
            return Ok(None);
        }

        Ok(Some(line))
    }

    #[cfg(not(target_os = "macos"))]
    fn fetch_line() -> Result<Option<String>, String> {
        Ok(None)
    }

    fn format_display_error(err: &str) -> String {
        let single_line = err.split_whitespace().collect::<Vec<_>>().join(" ");

        if single_line.chars().count() > Self::DISPLAY_ERROR_MAX_LEN {
            let truncated = single_line
                .chars()
                .take(Self::DISPLAY_ERROR_MAX_LEN.saturating_sub(1))
                .collect::<String>();
            return format!("Now playing error: {truncated}…");
        }

        format!("Now playing error: {single_line}")
    }

    #[cfg(target_os = "macos")]
    fn run_osascript_with_timeout(script: &str) -> Result<Output, String> {
        let mut child = Command::new("osascript")
            .arg("-e")
            .arg(script)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| err.to_string())?;
        let start = Instant::now();

        loop {
            match child.try_wait() {
                Ok(Some(_)) => return child.wait_with_output().map_err(|err| err.to_string()),
                Ok(None) => {
                    if start.elapsed() >= Self::COMMAND_TIMEOUT {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(format!(
                            "osascript timed out after {}ms (check macOS Automation permissions)",
                            Self::COMMAND_TIMEOUT.as_millis()
                        ));
                    }
                    thread::sleep(Duration::from_millis(20));
                }
                Err(err) => return Err(err.to_string()),
            }
        }
    }
}
