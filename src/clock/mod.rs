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
    config::{Config, LayoutMode, NowPlayingConfig, WeatherConfig},
    error::Error,
    now_playing::NowPlaying,
    position::Position,
    process_usage::ProcessUsage,
    weather::Weather,
};

#[derive(Default)]
pub struct Padding {
    pub top: u16,
    clock: String,
    terminal_width: u16,
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
    layout_mode: LayoutMode,
    alarm_active: bool,
    weather: Option<Weather>,
    now_playing: Option<NowPlaying>,
    process_usage: Option<ProcessUsage>,
}

enum InfoLine<'a> {
    Plain(&'a str),
    Weather {
        line: &'a str,
        temperature_celsius: f64,
    },
    ProcessUsage {
        usage_prefix: &'a str,
        usage_text: &'a str,
        usage_percent: f64,
    },
}

impl InfoLine<'_> {
    fn len_chars(&self) -> usize {
        match self {
            Self::Plain(line) | Self::Weather { line, .. } => line.chars().count(),
            Self::ProcessUsage {
                usage_prefix,
                usage_text,
                ..
            } => usage_prefix.chars().count() + usage_text.chars().count(),
        }
    }
}

impl Clock {
    const WIDTH: u16 = 51;
    const WIDTH_NO_SECONDS: u16 = 32;
    const HEIGHT: u16 = 7;
    const AM_SUFFIX: &'static str = " [AM]";
    const PM_SUFFIX: &'static str = " [PM]";
    const TEMP_COLOR_STOPS: [(f64, (u8, u8, u8)); 5] = [
        (-10.0, (8, 25, 80)),
        (5.0, (12, 40, 120)),
        (15.0, (25, 80, 45)),
        (25.0, (100, 55, 10)),
        (35.0, (95, 20, 10)),
    ];
    const USAGE_MIN_COLOR: (u8, u8, u8) = (30, 170, 65);
    const USAGE_MAX_COLOR: (u8, u8, u8) = (200, 35, 35);
    const DIGIT_ROWS: usize = 5;

    pub fn new(config: Config, mode: ClockMode) -> Self {
        let Config {
            general,
            position,
            date,
            layout,
            weather,
            now_playing,
            ..
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
            layout_mode: layout.mode,
            alarm_active: false,
            weather: Weather::from_config(weather),
            now_playing: NowPlaying::from_config(now_playing),
            process_usage: Some(ProcessUsage::new()),
        }
    }

    pub fn update_padding(&mut self, width: u16, height: u16) -> Result<(), Error> {
        let clock_width = self.width();
        self.mode.text(clock_width)?;

        let column = if self.layout_mode == LayoutMode::Split {
            Position::Start.calculate(width, clock_width / 2)
        } else {
            self.x_pos.calculate(width, clock_width / 2)
        };
        self.padding.top = self.y_pos.calculate(height, self.height() / 2);

        self.padding.clock = " ".repeat(column as usize);
        self.padding.terminal_width = width;

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

    pub fn refresh_process_usage(&mut self) {
        if !matches!(self.mode, ClockMode::Time { .. }) {
            return;
        }

        let Some(process_usage) = &mut self.process_usage else {
            return;
        };

        process_usage.update_if_due();
    }

    pub fn set_alarm_active(&mut self, active: bool) {
        self.alarm_active = active;
    }

    pub fn set_layout_mode(&mut self, layout_mode: LayoutMode) {
        self.layout_mode = layout_mode;
    }

    pub fn is_info_loading(&self) -> bool {
        self.weather.as_ref().is_some_and(Weather::is_loading)
            || self
                .now_playing
                .as_ref()
                .is_some_and(NowPlaying::is_loading)
            || self
                .process_usage
                .as_ref()
                .is_some_and(ProcessUsage::is_loading)
    }

    fn width(&self) -> u16 {
        if self.hide_seconds {
            return Self::WIDTH_NO_SECONDS;
        }

        Self::WIDTH
    }

    fn height(&self) -> u16 {
        if self.layout_mode == LayoutMode::Split {
            return Self::DIGIT_ROWS as u16;
        }

        Self::HEIGHT
            + if self.show_weather() { 1 } else { 0 }
            + if self.show_now_playing() { 1 } else { 0 }
            + if self.show_process_usage() { 1 } else { 0 }
    }

    fn show_weather(&self) -> bool {
        matches!(self.mode, ClockMode::Time { .. }) && self.weather.is_some()
    }

    fn show_now_playing(&self) -> bool {
        matches!(self.mode, ClockMode::Time { .. }) && self.now_playing.is_some()
    }

    fn show_process_usage(&self) -> bool {
        matches!(self.mode, ClockMode::Time { .. }) && self.process_usage.is_some()
    }

    fn line_padding(&self, line_len: u16) -> String {
        let half_width = self.width() / 2;

        format!(
            "{}{}",
            self.padding.clock,
            " ".repeat(half_width.saturating_sub(line_len / 2) as usize)
        )
    }

    fn info_lines<'a>(&'a self, date_line: &'a str) -> Vec<InfoLine<'a>> {
        let mut lines = vec![InfoLine::Plain(date_line)];

        if self.show_weather() {
            if let Some(weather_line) = self.weather.as_ref().and_then(Weather::line) {
                if let Some(temperature_celsius) =
                    self.weather.as_ref().and_then(Weather::temperature_celsius)
                {
                    lines.push(InfoLine::Weather {
                        line: weather_line,
                        temperature_celsius,
                    });
                } else {
                    lines.push(InfoLine::Plain(weather_line));
                }
            }
        }

        if self.show_now_playing() {
            if let Some(now_playing_line) = self.now_playing.as_ref().and_then(NowPlaying::line) {
                lines.push(InfoLine::Plain(now_playing_line));
            }
        }

        if self.show_process_usage() {
            if let Some(process_usage_line) = self
                .process_usage
                .as_ref()
                .and_then(ProcessUsage::line_data)
            {
                lines.push(InfoLine::ProcessUsage {
                    usage_prefix: process_usage_line.usage_prefix(),
                    usage_text: process_usage_line.usage_text(),
                    usage_percent: process_usage_line.usage_percent(),
                });
            } else {
                lines.push(InfoLine::Plain(""));
            }
        }

        lines
    }

    fn split_column(&self) -> (u16, u16) {
        let clock_start = self.padding.clock.chars().count() as u16;
        let clock_end = clock_start + self.width();
        let column_start = clock_end + 4;
        let column_width = self
            .padding
            .terminal_width
            .saturating_sub(column_start.saturating_add(1));

        (column_start, column_width)
    }

    fn rgb_fg((r, g, b): (u8, u8, u8)) -> String {
        format!("\x1B[38;2;{r};{g};{b}m")
    }

    fn lerp_u8(a: u8, b: u8, t: f64) -> u8 {
        let value = a as f64 + (b as f64 - a as f64) * t;
        value.round().clamp(0.0, 255.0) as u8
    }

    fn temperature_colors(celsius: f64) -> (u8, u8, u8) {
        let stops = &Self::TEMP_COLOR_STOPS;

        if celsius <= stops[0].0 {
            return stops[0].1;
        }

        if celsius >= stops[stops.len() - 1].0 {
            let stop = stops[stops.len() - 1];
            return stop.1;
        }

        for i in 0..(stops.len() - 1) {
            let (t0, fg0) = stops[i];
            let (t1, fg1) = stops[i + 1];

            if (t0..=t1).contains(&celsius) {
                let ratio = if (t1 - t0).abs() < f64::EPSILON {
                    0.0
                } else {
                    (celsius - t0) / (t1 - t0)
                };
                let fg = (
                    Self::lerp_u8(fg0.0, fg1.0, ratio),
                    Self::lerp_u8(fg0.1, fg1.1, ratio),
                    Self::lerp_u8(fg0.2, fg1.2, ratio),
                );

                return fg;
            }
        }

        stops[0].1
    }

    fn usage_colors(usage_percent: f64) -> (u8, u8, u8) {
        let ratio = ((usage_percent - 50.0) / 50.0).clamp(0.0, 1.0);
        (
            Self::lerp_u8(Self::USAGE_MIN_COLOR.0, Self::USAGE_MAX_COLOR.0, ratio),
            Self::lerp_u8(Self::USAGE_MIN_COLOR.1, Self::USAGE_MAX_COLOR.1, ratio),
            Self::lerp_u8(Self::USAGE_MIN_COLOR.2, Self::USAGE_MAX_COLOR.2, ratio),
        )
    }

    fn write_info_line(
        &self,
        w: &mut BufWriter<StdoutLock<'_>>,
        info_line: &InfoLine<'_>,
        bold_escape_str: &str,
    ) -> Result<(), Error> {
        match info_line {
            InfoLine::Plain(line) => {
                write!(w, "{line}")?;
            }
            InfoLine::Weather {
                line,
                temperature_celsius,
            } => {
                if let Some((temperature_text, rest)) = line.split_once(" | ") {
                    let fg = Self::temperature_colors(*temperature_celsius);

                    write!(
                        w,
                        "{}{}{}{}{}{}{}",
                        Self::rgb_fg(fg),
                        temperature_text,
                        Color::RESET,
                        bold_escape_str,
                        self.color.foreground(),
                        " | ",
                        rest
                    )?;
                } else {
                    write!(w, "{line}")?;
                }
            }
            InfoLine::ProcessUsage {
                usage_prefix,
                usage_text,
                usage_percent,
            } => {
                let fg = Self::usage_colors(*usage_percent);
                write!(
                    w,
                    "{usage_prefix}{}{}{}{}{}",
                    Self::rgb_fg(fg),
                    usage_text,
                    Color::RESET,
                    bold_escape_str,
                    self.color.foreground()
                )?;
            }
        }

        Ok(())
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

        let alarm_color = Color::BrightRed;
        let time_color = if self.alarm_active && (second & 1 == 0) {
            &alarm_color
        } else {
            &self.color
        };
        let info_lines = self.info_lines(&text);
        let (split_column_start, split_column_width) = self.split_column();
        let clock_end = self.padding.clock.chars().count() as u16 + self.width();

        let split_info_start_row = Self::DIGIT_ROWS
            .saturating_sub(info_lines.len())
            .saturating_add(1)
            / 2;

        for row in 0..Self::DIGIT_ROWS {
            let colon_character = if self.blink && (second & 1 == 1) {
                Character::Empty
            } else {
                Character::Colon
            };

            let colon = colon_character.fmt(time_color, row);
            let h0 = Character::Num(hour / 10).fmt(time_color, row);
            let h1 = Character::Num(hour % 10).fmt(time_color, row);
            let m0 = Character::Num(minute / 10).fmt(time_color, row);
            let m1 = Character::Num(minute % 10).fmt(time_color, row);

            write!(
                w,
                "\r\x1B[2K{}{}{}{}{}",
                self.padding.clock, h0, h1, colon, m0
            )?;
            write!(w, "{m1}")?;

            if !self.hide_seconds {
                let s0 = Character::Num(second / 10).fmt(time_color, row);
                let s1 = Character::Num(second % 10).fmt(time_color, row);

                write!(w, "{colon}{s0}{s1}")?;
            }

            if self.layout_mode == LayoutMode::Split {
                let info_row_index = row.saturating_sub(split_info_start_row);
                if row >= split_info_start_row {
                    if let Some(info_line) = info_lines.get(info_row_index) {
                        let line_len = info_line.len_chars() as u16;
                        let centered_offset = if line_len >= split_column_width {
                            0
                        } else {
                            (split_column_width - line_len) / 2
                        };
                        let target_col = split_column_start + centered_offset;
                        let gap = target_col.saturating_sub(clock_end);

                        write!(w, "{}{}", " ".repeat(gap as usize), self.color.foreground())?;
                        self.write_info_line(w, info_line, "")?;
                    }
                }
            }

            writeln!(w, "\r")?;
        }

        if self.layout_mode == LayoutMode::Split {
            return Ok(());
        }

        let bold_escape_str = if self.bold { Color::BOLD } else { "" };
        for (index, info_line) in info_lines.iter().enumerate() {
            let line_padding = self.line_padding(info_line.len_chars() as u16);
            let top_spacing = if index == 0 { "\n" } else { "" };

            write!(
                w,
                "{top_spacing}\r\x1B[2K{bold_escape_str}{line_padding}{}",
                self.color.foreground()
            )?;
            self.write_info_line(w, info_line, bold_escape_str)?;
            writeln!(w)?;
        }

        Ok(())
    }
}
