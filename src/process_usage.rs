use std::{
    process::{Command, Output, Stdio},
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::{Duration, Instant},
};

pub struct ProcessUsage {
    line: Option<ProcessUsageLine>,
    last_fetch: Option<Instant>,
    in_flight: Option<Receiver<Result<Option<ProcessUsageLine>, String>>>,
}

pub struct ProcessUsageLine {
    usage_prefix: String,
    usage_text: String,
    usage_percent: f64,
}

#[derive(Clone, Copy)]
enum Metric {
    Cpu,
    Ram,
}

struct ProcessSample {
    process_name: String,
    metric: Metric,
    usage_percent: f64,
}

impl ProcessUsage {
    const SYMBOL: &'static str = "⚠";
    const THRESHOLD_PERCENT: f64 = 50.0;
    const REFRESH_INTERVAL: Duration = Duration::from_secs(2);
    const COMMAND_TIMEOUT: Duration = Duration::from_millis(800);

    pub fn new() -> Self {
        Self {
            line: None,
            last_fetch: None,
            in_flight: None,
        }
    }

    pub fn line_data(&self) -> Option<&ProcessUsageLine> {
        self.line.as_ref()
    }

    pub fn update_if_due(&mut self) {
        let polled = self.in_flight.as_ref().map(Receiver::try_recv);

        match polled {
            Some(Ok(Ok(sample))) => {
                self.line = sample;
                self.in_flight = None;
            }
            Some(Ok(Err(_))) | Some(Err(TryRecvError::Disconnected)) => {
                self.line = None;
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
            .is_some_and(|last_fetch| last_fetch.elapsed() < Self::REFRESH_INTERVAL)
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

    #[cfg(unix)]
    fn fetch_line() -> Result<Option<ProcessUsageLine>, String> {
        let output = Self::run_ps_with_timeout()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let reason = if !stderr.is_empty() {
                stderr
            } else if !stdout.is_empty() {
                stdout
            } else {
                format!("ps exited with status {}", output.status)
            };

            return Err(reason);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut best: Option<ProcessSample> = None;

        for raw_line in stdout.lines() {
            let Some((process_name, cpu_percent, ram_percent)) = Self::parse_ps_line(raw_line)
            else {
                continue;
            };

            for (metric, usage_percent) in [(Metric::Cpu, cpu_percent), (Metric::Ram, ram_percent)]
            {
                if usage_percent <= Self::THRESHOLD_PERCENT {
                    continue;
                }

                let should_replace = match best.as_ref() {
                    Some(sample) => usage_percent > sample.usage_percent,
                    None => true,
                };

                if should_replace {
                    best = Some(ProcessSample {
                        process_name: process_name.clone(),
                        metric,
                        usage_percent,
                    });
                }
            }
        }

        Ok(best.map(ProcessUsageLine::from_sample))
    }

    #[cfg(not(unix))]
    fn fetch_line() -> Result<Option<ProcessUsageLine>, String> {
        Ok(None)
    }

    #[cfg(unix)]
    fn parse_ps_line(line: &str) -> Option<(String, f64, f64)> {
        let tokens: Vec<_> = line.split_whitespace().collect();
        if tokens.len() < 3 {
            return None;
        }

        let ram_percent = Self::parse_percent_token(tokens[tokens.len() - 1])?;
        let cpu_percent = Self::parse_percent_token(tokens[tokens.len() - 2])?;
        let process_name = Self::display_process_name(&tokens[..tokens.len() - 2].join(" "));

        if process_name.is_empty() {
            return None;
        }

        Some((process_name, cpu_percent, ram_percent))
    }

    #[cfg(unix)]
    fn parse_percent_token(token: &str) -> Option<f64> {
        let normalized = token.trim().replace(',', ".");
        normalized.parse::<f64>().ok()
    }

    #[cfg(unix)]
    fn display_process_name(raw: &str) -> String {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return String::new();
        }

        let executable = trimmed.split_whitespace().next().unwrap_or(trimmed);
        if executable.contains('/') {
            return executable
                .rsplit('/')
                .find(|part| !part.is_empty())
                .unwrap_or(executable)
                .to_string();
        }

        executable.to_string()
    }

    #[cfg(unix)]
    fn run_ps_with_timeout() -> Result<Output, String> {
        let mut child = Command::new("ps")
            .arg("-axo")
            .arg("args=,pcpu=,pmem=")
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
                            "ps timed out after {}ms",
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

impl ProcessUsageLine {
    fn from_sample(sample: ProcessSample) -> Self {
        let metric = sample.metric.label();
        let usage_text = format!("{:.1}%", sample.usage_percent);
        let usage_prefix = format!(
            "{} {} {} ",
            ProcessUsage::SYMBOL,
            sample.process_name,
            metric
        );

        Self {
            usage_prefix,
            usage_text,
            usage_percent: sample.usage_percent,
        }
    }

    pub fn usage_prefix(&self) -> &str {
        &self.usage_prefix
    }

    pub fn usage_text(&self) -> &str {
        &self.usage_text
    }

    pub fn usage_percent(&self) -> f64 {
        self.usage_percent
    }
}

impl Metric {
    fn label(self) -> &'static str {
        match self {
            Self::Cpu => "CPU",
            Self::Ram => "RAM",
        }
    }
}
