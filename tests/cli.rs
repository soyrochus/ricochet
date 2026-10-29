use ricochet::app::{Cli, parse};
use std::process::Command;
#[test]
fn help_version_and_bad_arguments_work_without_terminal() {
    for args in [vec!["--help"], vec!["--version"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_ricochet"))
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(!output.stdout.contains(&27));
    }
    for args in [vec!["--fps"], vec!["--fps", "120"], vec!["--unknown"]] {
        assert!(parse(args.into_iter().map(str::to_owned)).is_err());
    }
    assert_eq!(
        parse(["--fps".to_owned(), "30".to_owned()]).unwrap(),
        Cli::Run {
            fps: 30,
            fx: ricochet::render::fx::Level::Full
        }
    );
    let output = Command::new(env!("CARGO_BIN_EXE_ricochet"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("interactive stdin and stdout"));
    assert!(!output.stdout.contains(&27));
}
#[cfg(feature = "dev-harness")]
#[test]
fn development_fixtures_reach_end_states_via_simulation() {
    use ricochet::{app::fixture, game::State};
    assert_eq!(fixture("won").unwrap().state, State::Won);
    assert_eq!(fixture("game-over").unwrap().state, State::GameOver);
    assert!(fixture("invalid").is_none());
}
