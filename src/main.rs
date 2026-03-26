mod character;
mod cli;
mod clock;
#[macro_use]
mod color;
mod config;
mod error;
mod now_playing;
mod position;
mod segment;
mod state;
mod weather;

use std::process;

use crate::{color::Color, error::Error, state::State};

fn run() -> Result<(), Error> {
    State::new()?.run()
}

fn main() {
    if let Err(err) = run() {
        println!("{}error:{} {err}", esc!("1;31"), Color::RESET);
        process::exit(1);
    }
}
