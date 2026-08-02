use litecrazy::{browser, config, lock::acquire_instance_lock};
use log::{error, info};

const HELP: &str = "\
litecrazy — battery tray icon for the Pulsar X2 CrazyLight

USAGE:
    litecrazy            Run the tray (default)
    litecrazy --open     Open the web configurator and exit
    litecrazy --help     Show this message
    litecrazy --version  Show the version

Device settings are configured at the web configurator, not here.

ENVIRONMENT:
    LITECRAZY_URL              Configurator URL
    LITECRAZY_BROWSER          Browser binary; skips auto-detection
    LITECRAZY_BROWSER_ARGS     Extra flags passed to the browser
    LITECRAZY_WINDOW_MODE      app (default) or tab
    LITECRAZY_INTERVAL         Battery poll interval in seconds (10-3600)
    LITECRAZY_LOW_THRESHOLD    Low-battery notification percent (0 = off)
    LITECRAZY_PAUSE_MINUTES    Polling pause after opening the configurator
";

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{HELP}");
        return Ok(());
    }

    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("litecrazy {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    init_logger();

    if args.iter().any(|a| a == "--open" || a == "-o") {
        info!("Opening configurator at {}", config::configurator_url());
        browser::open_configurator();
        std::thread::sleep(std::time::Duration::from_millis(300));
        return Ok(());
    }

    if let Some(unknown) = args
        .iter()
        .find(|a| a.starts_with('-') && a.as_str() != "--")
    {
        eprintln!("litecrazy: unrecognised option '{unknown}'\n");
        eprint!("{HELP}");
        std::process::exit(2);
    }

    let _lock = acquire_instance_lock().map_err(|_| {
        anyhow::anyhow!(
            "litecrazy is already running.\n\
             Use --open to open the configurator in a browser."
        )
    })?;

    if let Err(e) = litecrazy::tray::run() {
        error!("Tray service error: {e}");
        std::process::exit(1);
    }

    Ok(())
}

fn init_logger() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .format_timestamp(Some(env_logger::TimestampPrecision::Seconds))
        .parse_default_env()
        .init();
}
