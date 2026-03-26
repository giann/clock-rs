use std::{
    env::{self, VarError},
    fs,
    path::Path,
    str::FromStr,
};

use serde::{de, Deserialize, Deserializer};

use crate::{color::Color, error::Error, position::Position};

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub general: GeneralConfig,
    pub position: PositionConfig,
    pub date: DateConfig,
    pub layout: LayoutConfig,
    pub weather: WeatherConfig,
    pub now_playing: NowPlayingConfig,
    pub alarms: Vec<AlarmConfig>,
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
pub struct LayoutConfig {
    pub mode: LayoutMode,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            mode: LayoutMode::default(),
        }
    }
}

#[derive(Clone, Copy, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum LayoutMode {
    #[default]
    Stacked,
    Split,
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

#[derive(Clone, Deserialize)]
pub struct AlarmConfig {
    pub days: Vec<AlarmDay>,
    pub time: AlarmTime,
}

#[derive(Clone, Copy)]
pub enum AlarmDay {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl AlarmDay {
    pub fn matches(self, weekday: chrono::Weekday) -> bool {
        use chrono::Weekday;

        matches!(
            (self, weekday),
            (Self::Monday, Weekday::Mon)
                | (Self::Tuesday, Weekday::Tue)
                | (Self::Wednesday, Weekday::Wed)
                | (Self::Thursday, Weekday::Thu)
                | (Self::Friday, Weekday::Fri)
                | (Self::Saturday, Weekday::Sat)
                | (Self::Sunday, Weekday::Sun)
        )
    }
}

impl FromStr for AlarmDay {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "mon" | "monday" => Ok(Self::Monday),
            "tue" | "tues" | "tuesday" => Ok(Self::Tuesday),
            "wed" | "wednesday" => Ok(Self::Wednesday),
            "thu" | "thur" | "thurs" | "thursday" => Ok(Self::Thursday),
            "fri" | "friday" => Ok(Self::Friday),
            "sat" | "saturday" => Ok(Self::Saturday),
            "sun" | "sunday" => Ok(Self::Sunday),
            _ => Err(format!("invalid weekday `{value}`")),
        }
    }
}

impl<'de> Deserialize<'de> for AlarmDay {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let string = String::deserialize(deserializer)?;
        Self::from_str(&string).map_err(de::Error::custom)
    }
}

#[derive(Clone, Copy)]
pub struct AlarmTime {
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl FromStr for AlarmTime {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let parts: Vec<_> = value.split(':').collect();
        if parts.len() != 2 && parts.len() != 3 {
            return Err(format!(
                "invalid alarm time `{value}`: expected `HH:MM` or `HH:MM:SS`"
            ));
        }

        let hour = parts[0]
            .parse::<u32>()
            .map_err(|_| format!("invalid alarm hour in `{value}`"))?;
        let minute = parts[1]
            .parse::<u32>()
            .map_err(|_| format!("invalid alarm minute in `{value}`"))?;
        let second = if parts.len() == 3 {
            parts[2]
                .parse::<u32>()
                .map_err(|_| format!("invalid alarm second in `{value}`"))?
        } else {
            0
        };

        if hour > 23 || minute > 59 || second > 59 {
            return Err(format!("invalid alarm time `{value}`: out-of-range values"));
        }

        Ok(Self {
            hour,
            minute,
            second,
        })
    }
}

impl<'de> Deserialize<'de> for AlarmTime {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let string = String::deserialize(deserializer)?;
        Self::from_str(&string).map_err(de::Error::custom)
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
