//! Explicit development fixture; absent from the normal gameplay CLI.
use std::{io, process::ExitCode};
fn execute() -> io::Result<()> {
    let name = std::env::args().nth(1).unwrap_or_default();
    if matches!(name.as_str(), "error" | "panic") {
        use ricochet::{
            game::Game,
            render::Renderer,
            terminal::{self, Session, Signals},
        };
        let signals = Signals::install()?;
        let mut session = Session::start()?;
        let capabilities = terminal::negotiate(&mut session.out, &signals.shutdown)?;
        Renderer::default().draw(
            &mut session,
            capabilities.geometry,
            &Game::default(),
            capabilities.synchronized,
        )?;
        if name == "panic" {
            panic!("development injected panic");
        }
        return Err(io::Error::other("development injected runtime error"));
    }
    let game = ricochet::app::fixture(&name)
        .ok_or_else(|| io::Error::other("usage: acceptance won|game-over|damaged|error|panic"))?;
    ricochet::app::run_game(60, game)
}
fn main() -> ExitCode {
    match ricochet::app::restored_errors(|| execute().map_err(|e| e.to_string())) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("ricochet acceptance: {error}");
            ExitCode::FAILURE
        }
    }
}
