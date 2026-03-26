pub mod counter;
pub mod mode;
pub mod time_zone;

use std::{
    io::{BufWriter, StdoutLock, Write},
    time::Duration,
};

use crate::{
    character::Character,
    clock::mode::ClockMode,
    color::Color,
    config::{Config, NowPlayingConfig, WeatherConfig},
    error::Error,
    now_playing::NowPlaying,
    position::Position,
    weather::Weather,
};

#[derive(Default)]
pub struct Padding {
    pub top: u16,
    clock: String,
}

pub struct Clock {
    pub mode: ClockMode,
    pub padding: Padding,
    pub interval: Duration,
    pub x_pos: Position,
    pub y_pos: Position,
    pub color: Color,
    pub use_12h: bool,
    pub hide_seconds: bool,
    pub blink: bool,
    pub bold: bool,
    weather: Option<Weather>,
    now_playing: Option<NowPlaying>,
}

impl Clock {
    const WIDTH: u16 = 51;
    const WIDTH_NO_SECONDS: u16 = 32;
    const HEIGHT: u16 = 7;
    const AM_SUFFIX: &'static str = " [AM]";
    const PM_SUFFIX: &'static str = " [PM]";

    pub fn new(config: Config, mode: ClockMode) -> Self {
        let Config {
            general,
            position,
            date,
            weather,
            now_playing,
        } = config;

        Self {
            mode,
            padding: Padding::default(),
            interval: Duration::from_millis(general.interval),
            x_pos: position.x,
            y_pos: position.y,
            color: general.color,
            use_12h: date.use_12h,
            hide_seconds: date.hide_seconds,
            blink: general.blink,
            bold: general.bold,
            weather: Weather::from_config(weather),
            now_playing: NowPlaying::from_config(now_playing),
        }
    }

    pub fn update_padding(&mut self, width: u16, height: u16) -> Result<(), Error> {
        let clock_width = self.width();
        self.mode.text(clock_width)?;

        let column = self.x_pos.calculate(width, clock_width / 2);
        self.padding.top = self.y_pos.calculate(height, self.height() / 2);

        self.padding.clock = " ".repeat(column as usize);

        Ok(())
    }

    pub fn is_too_large(&self, width: u16, height: u16) -> bool {
        self.width() + 1 >= width || self.height() + 1 >= height
    }

    pub fn refresh_weather(&mut self) {
        if !matches!(self.mode, ClockMode::Time { .. }) {
            return;
        }

        let Some(weather) = &mut self.weather else {
            return;
        };

        weather.update_if_due();
    }

    pub fn set_weather_config(&mut self, weather_config: WeatherConfig) {
        self.weather = Weather::from_config(weather_config);
    }

    pub fn refresh_now_playing(&mut self) {
        if !matches!(self.mode, ClockMode::Time { .. }) {
            return;
        }

        let Some(now_playing) = &mut self.now_playing else {
            return;
        };

        now_playing.update_if_due();
    }

    pub fn set_now_playing_config(&mut self, now_playing_config: NowPlayingConfig) {
        self.now_playing = NowPlaying::from_config(now_playing_config);
    }

    fn width(&self) -> u16 {
        if self.hide_seconds {
            return Self::WIDTH_NO_SECONDS;
        }

        Self::WIDTH
    }

    fn height(&self) -> u16 {
        Self::HEIGHT
            + if self.show_weather() { 1 } else { 0 }
            + if self.show_now_playing() { 1 } else { 0 }
    }

    fn show_weather(&self) -> bool {
        matches!(self.mode, ClockMode::Time { .. }) && self.weather.is_some()
    }

    fn show_now_playing(&self) -> bool {
        matches!(self.mode, ClockMode::Time { .. }) && self.now_playing.is_some()
    }

    fn line_padding(&self, line_len: u16) -> String {
        let half_width = self.width() / 2;

        format!(
            "{}{}",
            self.padding.clock,
            " ".repeat(half_width.saturating_sub(line_len / 2) as usize)
        )
    }

    pub fn fmt(&self, w: &mut BufWriter<StdoutLock<'_>>) -> Result<(), Error> {
        let mut text = self.mode.text(self.width())?;
        let (mut hour, minute, second) = self.mode.get_time();

        if matches!(self.mode, ClockMode::Time { .. }) && self.use_12h {
            let suffix = if hour < 12 {
                Self::AM_SUFFIX
            } else {
                Self::PM_SUFFIX
            };

            text.push_str(suffix);

            if hour > 12 {
                hour -= 12;
            } else if hour == 0 {
                hour = 12;
            }
        }

        let color = &self.color;

        for row in 0..5 {
            let colon_character = if self.blink && (second & 1 == 1) {
                Character::Empty
            } else {
                Character::Colon
            };

            let colon = colon_character.fmt(color, row);
            let h0 = Character::Num(hour / 10).fmt(color, row);
            let h1 = Character::Num(hour % 10).fmt(color, row);
            let m0 = Character::Num(minute / 10).fmt(color, row);
            let m1 = Character::Num(minute % 10).fmt(color, row);

            write!(w, "{}{h0}{h1}{colon}{m0}{m1}", self.padding.clock)?;

            if !self.hide_seconds {
                let s0 = Character::Num(second / 10).fmt(color, row);
                let s1 = Character::Num(second % 10).fmt(color, row);

                write!(w, "{colon}{s0}{s1}")?;
            }

            writeln!(w, "\r")?;
        }

        let bold_escape_str = if self.bold { Color::BOLD } else { "" };
        let text_padding = self.line_padding(text.chars().count() as u16);

        writeln!(
            w,
            "\n\r\x1B[2K{bold_escape_str}{}{}{text}",
            text_padding,
            self.color.foreground()
        )?;

        if self.show_weather() {
            if let Some(weather_line) = self.weather.as_ref().and_then(Weather::line) {
                let weather_padding = self.line_padding(weather_line.chars().count() as u16);

                writeln!(
                    w,
                    "\r\x1B[2K{bold_escape_str}{weather_padding}{}{}",
                    self.color.foreground(),
                    weather_line
                )?;
            }
        }

        if self.show_now_playing() {
            if let Some(now_playing_line) = self.now_playing.as_ref().and_then(NowPlaying::line) {
                let now_playing_padding =
                    self.line_padding(now_playing_line.chars().count() as u16);

                writeln!(
                    w,
                    "\r\x1B[2K{bold_escape_str}{now_playing_padding}{}{}",
                    self.color.foreground(),
                    now_playing_line
                )?;
            }
        }

        Ok(())
    }
}
