use std::{
    env::{self, VarError},
    fs,
    path::Path,
};

use serde::Deserialize;

use crate::{color::Color, error::Error, position::Position};

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub general: GeneralConfig,
    pub position: PositionConfig,
    pub date: DateConfig,
    pub weather: WeatherConfig,
    pub now_playing: NowPlayingConfig,
}

#[derive(Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    pub color: Color,
    pub interval: u64,
    pub blink: bool,
    pub bold: bool,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            interval: 200,
            color: Color::default(),
            blink: false,
            bold: false,
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct PositionConfig {
    #[serde(rename = "horizontal")]
    pub x: Position,
    #[serde(rename = "vertical")]
    pub y: Position,
}

#[derive(Deserialize)]
#[serde(default)]
pub struct DateConfig {
    pub fmt: String,
    pub use_12h: bool,
    pub utc: bool,
    pub hide_seconds: bool,
}

impl Default for DateConfig {
    fn default() -> Self {
        Self {
            fmt: "%d-%m-%Y".to_string(),
            use_12h: false,
            utc: false,
            hide_seconds: false,
        }
    }
}

#[derive(Clone, Deserialize)]
#[serde(default)]
pub struct WeatherConfig {
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub auto_location: bool,
    pub refresh_interval_minutes: u64,
    pub temperature_unit: TemperatureUnit,
}

impl Default for WeatherConfig {
    fn default() -> Self {
        Self {
            latitude: None,
            longitude: None,
            auto_location: false,
            refresh_interval_minutes: 10,
            temperature_unit: TemperatureUnit::default(),
        }
    }
}

#[derive(Clone, Deserialize)]
#[serde(default)]
pub struct NowPlayingConfig {
    pub enabled: bool,
    pub refresh_interval_seconds: u64,
}

impl Default for NowPlayingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            refresh_interval_seconds: 5,
        }
    }
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TemperatureUnit {
    #[default]
    Celsius,
    Fahrenheit,
}

impl TemperatureUnit {
    pub fn as_api_value(self) -> &'static str {
        match self {
            Self::Celsius => "celsius",
            Self::Fahrenheit => "fahrenheit",
        }
    }

    pub fn symbol(self) -> &'static str {
        match self {
            Self::Celsius => "°C",
            Self::Fahrenheit => "°F",
        }
    }
}

impl Config {
    pub fn parse() -> Result<Self, Error> {
        let path = match env::var("CONF_PATH") {
            Ok(path) => match path.as_str() {
                "None" => None,
                _ => Some(path),
            },
            Err(VarError::NotUnicode(path)) => {
                return Err(Error::NonUnicodePath(path.display().to_string()));
            }
            Err(VarError::NotPresent) => Self::default_config_path()?,
        };

        let Some(file_path) = path else {
            return Ok(Config::default());
        };

        let config_str = fs::read_to_string(&file_path).map_err(|err| Error::ReadFile {
            path: file_path.clone(),
            err: err.to_string(),
        })?;

        toml::from_str(&config_str).map_err(|err| Error::ParseToml {
            path: file_path,
            err: err.to_string(),
        })
    }

    fn default_config_path() -> Result<Option<String>, Error> {
        #[cfg(target_os = "macos")]
        let config_path = dirs::home_dir()
            .map(|home_dir| home_dir.join(".config").join("clock-rs").join("conf.toml"));

        #[cfg(not(target_os = "macos"))]
        let config_path = dirs::config_local_dir()
            .map(|config_local_dir| config_local_dir.join("clock-rs").join("conf.toml"));

        let Some(config_path) = config_path else {
            return Ok(None);
        };

        let Some(path) = config_path.to_str() else {
            return Err(Error::NonUnicodePath(config_path.display().to_string()));
        };

        if Path::new(path).exists() {
            return Ok(Some(path.to_string()));
        }

        Ok(None)
    }
}
