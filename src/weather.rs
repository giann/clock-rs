use std::{
    env,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::{Duration, Instant},
};

use serde::Deserialize;

use crate::config::{TemperatureUnit, WeatherConfig};

pub struct Weather {
    latitude: Option<f64>,
    longitude: Option<f64>,
    refresh_interval: Duration,
    temperature_unit: TemperatureUnit,
    line: Option<String>,
    temperature_celsius: Option<f64>,
    last_fetch: Option<Instant>,
    in_flight: Option<Receiver<Result<WeatherData, String>>>,
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

struct WeatherData {
    line: String,
    temperature_celsius: f64,
}

impl Weather {
    const REQUEST_TIMEOUT: Duration = Duration::from_secs(6);
    const FALLBACK_LINE: &'static str = "Weather unavailable";

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
            temperature_celsius: None,
            last_fetch: None,
            in_flight: None,
        })
    }

    pub fn line(&self) -> Option<&str> {
        self.line.as_deref()
    }

    pub fn temperature_celsius(&self) -> Option<f64> {
        self.temperature_celsius
    }

    pub fn update_if_due(&mut self) {
        let polled = self.in_flight.as_ref().map(Receiver::try_recv);

        match polled {
            Some(Ok(Ok(weather_data))) => {
                self.line = Some(weather_data.line);
                self.temperature_celsius = Some(weather_data.temperature_celsius);
                self.in_flight = None;
            }
            Some(Ok(Err(_))) | Some(Err(TryRecvError::Disconnected)) => {
                self.line = Some(Self::FALLBACK_LINE.to_string());
                self.temperature_celsius = None;
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
        let latitude = self.latitude;
        let longitude = self.longitude;
        let temperature_unit = self.temperature_unit;

        thread::spawn(move || {
            let _ = tx.send(Self::fetch_line_for(latitude, longitude, temperature_unit));
        });
        self.in_flight = Some(rx);
    }

    pub fn is_loading(&self) -> bool {
        self.in_flight.is_some()
    }

    fn fetch_line_for(
        latitude: Option<f64>,
        longitude: Option<f64>,
        temperature_unit: TemperatureUnit,
    ) -> Result<WeatherData, String> {
        let (latitude, longitude) = Self::coordinates(latitude, longitude)?;
        let url = format!(
            "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&current=temperature_2m,weather_code&temperature_unit={}&forecast_days=1",
            latitude,
            longitude,
            temperature_unit.as_api_value()
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
        let temp_value = weather.current.temperature_2m;
        let temperature_celsius = match temperature_unit {
            TemperatureUnit::Celsius => temp_value,
            TemperatureUnit::Fahrenheit => (temp_value - 32.0) * 5.0 / 9.0,
        };

        Ok(WeatherData {
            line: format!(
                "{temp_value:.1}{} | {icon}  {wording}",
                temperature_unit.symbol()
            ),
            temperature_celsius,
        })
    }

    fn coordinates(latitude: Option<f64>, longitude: Option<f64>) -> Result<(f64, f64), String> {
        match (latitude, longitude) {
            (Some(latitude), Some(longitude)) => Ok((latitude, longitude)),
            _ => Self::resolve_coordinates_from_ip(),
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
