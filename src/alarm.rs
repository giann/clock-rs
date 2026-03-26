use chrono::{Datelike, Timelike};

use crate::config::{AlarmConfig, AlarmDay};

#[derive(Clone, Copy, Eq, PartialEq)]
struct TriggerStamp {
    alarm_index: usize,
    year: i32,
    ordinal: u32,
    hour: u32,
    minute: u32,
    second: u32,
}

pub struct AlarmScheduler {
    alarms: Vec<AlarmConfig>,
    use_utc: bool,
    active: bool,
    last_trigger: Option<TriggerStamp>,
}

impl AlarmScheduler {
    pub fn new(alarms: Vec<AlarmConfig>, use_utc: bool) -> Self {
        Self {
            alarms,
            use_utc,
            active: false,
            last_trigger: None,
        }
    }

    pub fn refresh(&mut self) {
        if self.active {
            return;
        }

        if self.alarms.is_empty() {
            return;
        }

        let now = if self.use_utc {
            chrono::Utc::now().naive_utc()
        } else {
            chrono::Local::now().naive_local()
        };

        let weekday = now.weekday();
        let hour = now.hour();
        let minute = now.minute();
        let second = now.second();

        for (alarm_index, alarm) in self.alarms.iter().enumerate() {
            if !Self::matches_day(&alarm.days, weekday) {
                continue;
            }

            if alarm.time.hour != hour || alarm.time.minute != minute || alarm.time.second != second
            {
                continue;
            }

            let stamp = TriggerStamp {
                alarm_index,
                year: now.year(),
                ordinal: now.ordinal(),
                hour,
                minute,
                second,
            };

            if self.last_trigger == Some(stamp) {
                continue;
            }

            self.last_trigger = Some(stamp);
            self.active = true;
            return;
        }
    }

    pub fn dismiss(&mut self) {
        self.active = false;
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn reconfigure(&mut self, alarms: Vec<AlarmConfig>, use_utc: bool) {
        self.alarms = alarms;
        self.use_utc = use_utc;
    }

    fn matches_day(days: &[AlarmDay], weekday: chrono::Weekday) -> bool {
        days.iter().any(|day| day.matches(weekday))
    }
}
