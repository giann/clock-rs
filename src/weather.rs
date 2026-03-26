use std::{env, time::Duration, time::Instant};

use serde::Deserialize;

use crate::config::{TemperatureUnit, WeatherConfig};

pub struct Weather {
    latitude: Option<f64>,
    longitude: Option<f64>,
    refresh_interval: Duration,
    temperature_unit: TemperatureUnit,
    line: Option<String>,
    last_fetch: Option<Instant>,
}

#[derive(Deserialize)]
struct OpenMeteoResponse {
    current: OpenMeteoCurrent,
}

#[derive(Deserialize)]
struct OpenMeteoCurrent {
    temperature_2m: f64,
    weather_code: u16,
}

#[derive(Deserialize)]
struct IpApiResponse {
    latitude: Option<f64>,
    longitude: Option<f64>,
}

impl Weather {
    const REQUEST_TIMEOUT: Duration = Duration::from_secs(6);
    const DISPLAY_ERROR_MAX_LEN: usize = 80;

    pub fn from_config(config: WeatherConfig) -> Option<Self> {
        let has_coordinates = config.latitude.is_some() && config.longitude.is_some();

        if !has_coordinates && !config.auto_location {
            return None;
        }

        let refresh_interval =
            Duration::from_secs(config.refresh_interval_minutes.max(1).saturating_mul(60));

        Some(Self {
            latitude: config.latitude,
            longitude: config.longitude,
            refresh_interval,
            temperature_unit: config.temperature_unit,
            line: None,
            last_fetch: None,
        })
    }

    pub fn line(&self) -> Option<&str> {
        self.line.as_deref()
    }

    pub fn update_if_due(&mut self) {
        if self
            .last_fetch
            .is_some_and(|last_fetch| last_fetch.elapsed() < self.refresh_interval)
        {
            return;
        }

        self.last_fetch = Some(Instant::now());

        match self.fetch_line() {
            Ok(line) => self.line = Some(line),
            Err(err) => {
                eprintln!("weather fetch error: {err}");
                self.line = Some(Self::format_display_error(&err));
            }
        }
    }

    fn fetch_line(&mut self) -> Result<String, String> {
        let (latitude, longitude) = self.coordinates()?;
        let url = format!(
            "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&current=temperature_2m,weather_code&temperature_unit={}&forecast_days=1",
            latitude,
            longitude,
            self.temperature_unit.as_api_value()
        );

        let response = Self::agent_for_host("api.open-meteo.com", 443, true)?
            .get(&url)
            .timeout(Self::REQUEST_TIMEOUT)
            .call()
            .map_err(|err| err.to_string())?;

        let weather = response
            .into_json::<OpenMeteoResponse>()
            .map_err(|err| err.to_string())?;
        let (icon, wording) = Self::weather_display(weather.current.weather_code);

        Ok(format!(
            "{:.1}{} | {icon}  {wording}",
            weather.current.temperature_2m,
            self.temperature_unit.symbol()
        ))
    }

    fn coordinates(&mut self) -> Result<(f64, f64), String> {
        match (self.latitude, self.longitude) {
            (Some(latitude), Some(longitude)) => Ok((latitude, longitude)),
            _ => {
                let (latitude, longitude) = Self::resolve_coordinates_from_ip()?;
                self.latitude = Some(latitude);
                self.longitude = Some(longitude);
                Ok((latitude, longitude))
            }
        }
    }

    fn resolve_coordinates_from_ip() -> Result<(f64, f64), String> {
        let response = Self::agent_for_host("ipapi.co", 443, true)?
            .get("https://ipapi.co/json/")
            .timeout(Self::REQUEST_TIMEOUT)
            .call()
            .map_err(|err| err.to_string())?;

        let ip_location = response
            .into_json::<IpApiResponse>()
            .map_err(|err| err.to_string())?;
        let (Some(latitude), Some(longitude)) = (ip_location.latitude, ip_location.longitude)
        else {
            return Err("could not resolve coordinates from IP".to_string());
        };

        Ok((latitude, longitude))
    }

    fn weather_display(code: u16) -> (&'static str, &'static str) {
        match code {
            0 => ("☀", "Clear sky"),
            1 => ("☀", "Mainly clear"),
            2 => ("☁", "Partly cloudy"),
            3 => ("☁", "Overcast"),
            45 | 48 => ("≋", "Fog"),
            51 | 53 | 55 => ("☂", "Drizzle"),
            56 | 57 => ("☂", "Freezing drizzle"),
            61 | 63 | 65 => ("☂", "Rain"),
            66 | 67 => ("☂", "Freezing rain"),
            71 | 73 | 75 => ("*", "Snow"),
            77 => ("*", "Snow grains"),
            80 | 81 | 82 => ("☂", "Rain showers"),
            85 | 86 => ("*", "Snow showers"),
            95 => ("⚡", "Thunderstorm"),
            96 | 99 => ("⚡*", "Thunderstorm + hail"),
            _ => ("?", "Unknown"),
        }
    }

    fn format_display_error(err: &str) -> String {
        let single_line = err.split_whitespace().collect::<Vec<_>>().join(" ");

        if single_line.chars().count() > Self::DISPLAY_ERROR_MAX_LEN {
            let truncated = single_line
                .chars()
                .take(Self::DISPLAY_ERROR_MAX_LEN.saturating_sub(1))
                .collect::<String>();
            return format!("Weather error: {truncated}…");
        }

        format!("Weather error: {single_line}")
    }

    fn agent_for_host(host: &str, port: u16, is_https: bool) -> Result<ureq::Agent, String> {
        let mut builder = ureq::AgentBuilder::new()
            .timeout_connect(Self::REQUEST_TIMEOUT)
            .timeout_read(Self::REQUEST_TIMEOUT)
            .timeout_write(Self::REQUEST_TIMEOUT)
            .timeout(Self::REQUEST_TIMEOUT);

        if !Self::should_bypass_proxy(host, port) {
            if let Some(proxy_url) = Self::proxy_url_from_env(is_https) {
                let proxy = ureq::Proxy::new(&proxy_url).map_err(|err| err.to_string())?;
                builder = builder.proxy(proxy);
            }
        }

        Ok(builder.build())
    }

    fn proxy_url_from_env(is_https: bool) -> Option<String> {
        let vars: &[&str] = if is_https {
            &[
                "HTTPS_PROXY",
                "https_proxy",
                "ALL_PROXY",
                "all_proxy",
                "HTTP_PROXY",
                "http_proxy",
            ]
        } else {
            &["HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"]
        };

        vars.iter()
            .find_map(|name| env::var(name).ok())
            .and_then(|value| {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            })
    }

    fn should_bypass_proxy(host: &str, port: u16) -> bool {
        let no_proxy = env::var("NO_PROXY")
            .ok()
            .or_else(|| env::var("no_proxy").ok());
        let Some(no_proxy) = no_proxy else {
            return false;
        };

        no_proxy
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .any(|entry| Self::no_proxy_entry_matches(entry, host, port))
    }

    fn no_proxy_entry_matches(entry: &str, host: &str, port: u16) -> bool {
        let host = host.to_ascii_lowercase();

        if entry == "*" {
            return true;
        }

        let entry = entry
            .strip_prefix("http://")
            .or_else(|| entry.strip_prefix("https://"))
            .unwrap_or(entry);
        let entry = entry.trim_end_matches('/');

        // NO_PROXY entries can include an optional port.
        let (entry_host, entry_port) = match entry.rsplit_once(':') {
            Some((h, p)) if p.chars().all(|ch| ch.is_ascii_digit()) => (h, p.parse::<u16>().ok()),
            _ => (entry, None),
        };

        if let Some(entry_port) = entry_port {
            if entry_port != port {
                return false;
            }
        }

        let entry_host = entry_host.trim().to_ascii_lowercase();
        if entry_host.is_empty() {
            return false;
        }

        if let Some(stripped) = entry_host.strip_prefix('.') {
            return host == stripped || host.ends_with(&entry_host);
        }

        host == entry_host || host.ends_with(&format!(".{entry_host}"))
    }
}
