mod app;
mod config;
mod playback;
mod provider;
mod tui;

use provider::{
    deezer::{DeezerError, DeezerProvider},
    invidious::AutomaticProvider,
    router::Providers,
};
use std::{
    io::{self, IsTerminal},
    process::ExitCode,
};

#[tokio::main(worker_threads = 2)]
async fn main() -> ExitCode {
    match start().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("Mélimo: {message}");
            ExitCode::FAILURE
        }
    }
}

async fn start() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        println!(
            "Mélimo — unofficial streaming-only terminal music client\n\nUsage: melimo [--mock | --invidious | --check-auth | --login | --forget | --version]\n\nDefault: Deezer search using DEEZER_ARL or the saved login.\n--invidious: anonymous Invidious audio with automatic instance selection.\n--mock: offline demo with fictional tracks.\n--check-auth: validate credentials without opening the TUI.\n--login: open browser sign-in; saves your ARL cookie (0600) for next launch.\n--forget: delete the saved ARL cookie.\n--version: print the version and exit.\n\nSet MELIMO_NO_STORE=1 to never write the saved login.\nSet MELIMO_CONFIG to a TOML settings file; see README for [invidious].\nEnter plays a selected track. P switches search provider. Space pauses/resumes; s stops."
        );
        return Ok(());
    }
    if args.len() == 1 && (args[0] == "--version" || args[0] == "-V") {
        println!("Mélimo {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.len() > 1
        || args.first().is_some_and(|arg| {
            arg != "--mock"
                && arg != "--invidious"
                && arg != "--check-auth"
                && arg != "--version"
                && arg != "--login"
                && arg != "--forget"
        })
    {
        // Do not echo arguments; a user may accidentally pass credentials here.
        return Err("Unknown argument. Use melimo --help.".into());
    }
    if args.first().is_some_and(|arg| arg == "--forget") {
        config::store::clear()?;
        println!("Saved login removed.");
        return Ok(());
    }
    if args.first().is_some_and(|arg| arg == "--check-auth") {
        let arl = config::resolve_arl()?;
        let provider = DeezerProvider::new(arl).map_err(|error| error.to_string())?;
        provider
            .check_auth()
            .await
            .map_err(|error| error.to_string())?;
        println!("Deezer authentication succeeded. No account details or credentials displayed.");
        return Ok(());
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err("An interactive terminal is required.".into());
    }
    if args.first().is_some_and(|arg| arg == "--mock") {
        run(Providers {
            mock: true,
            ..Default::default()
        })
    } else {
        let settings = config::invidious::Settings::load()?;
        let invidious = if settings.invidious.enabled {
            Some(AutomaticProvider::new(settings.invidious.instance()?)?)
        } else { None };
        let invidious_only = args.first().is_some_and(|arg| arg == "--invidious");
        if invidious_only && invidious.is_none() {
            return Err("Invidious is disabled. Set [invidious] enabled = true in your Mélimo configuration.".into());
        }
        let deezer = if invidious_only {
            None
        } else {
            Some(connect(&args).await?)
        };
        run(Providers {
            deezer,
            invidious,
            mock: false,
        })
    }
}

/// Build an authenticated Deezer provider for the TUI.
///
/// Precedence: explicit `--login`, then `DEEZER_ARL`, then a saved login. A
/// saved credential Deezer clearly rejects (HTTP 401 or an unauthenticated
/// payload) is deleted and the user is taken through login again. Ambiguous
/// failures (network, timeout, HTTP 403, gateway) keep the saved login.
async fn connect(args: &[std::ffi::OsString]) -> Result<DeezerProvider, String> {
    if args.first().is_some_and(|arg| arg == "--login") {
        return login_provider().await;
    }
    if std::env::var_os("DEEZER_ARL").is_some() {
        let arl = config::Arl::from_env().map_err(str::to_owned)?;
        return check_arl(arl).await.map_err(|error| error.to_string());
    }
    match config::store::load()? {
        Some(arl) => match check_arl(arl).await {
            Ok(provider) => Ok(provider),
            Err(DeezerError::Authentication) => {
                let _ = config::store::clear();
                login_provider().await
            }
            Err(error) => Err(error.to_string()),
        },
        None => {
            Err("No credentials found. Set DEEZER_ARL, or run melimo --login to sign in.".into())
        }
    }
}

/// Construct a provider and validate the credential once.
async fn check_arl(arl: config::Arl) -> Result<DeezerProvider, DeezerError> {
    let provider = DeezerProvider::new(arl)?;
    provider.check_auth().await?;
    Ok(provider)
}

/// Save only after authentication succeeds; never replace a working saved login on failure.
async fn login_provider() -> Result<DeezerProvider, String> {
    let arl = config::login_arl()?;
    let provider = check_arl(arl.clone())
        .await
        .map_err(|error| error.to_string())?;
    if config::store::save(&arl).is_err() {
        eprintln!("Login succeeded, but could not be saved; this session is memory-only.");
    }
    Ok(provider)
}

fn run(provider: Providers) -> Result<(), String> {
    let mut terminal = ratatui::init();
    let result = app::run(&mut terminal, provider);
    ratatui::restore();
    result.map_err(|_| "Terminal I/O failed.".into())
}
