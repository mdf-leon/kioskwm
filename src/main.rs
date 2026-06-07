mod alt_tab;
mod apps;
mod context_menu;
mod cursor;
mod emergency;
mod hardware_bridge;
mod hardware_poll;
mod i18n;
mod modifiers;
mod env_detect;
mod font8x8;
mod input;
mod kill_switch;
mod overlay;
mod parent_shortcuts;
mod render;
mod settings;
mod spawn;
mod state;
mod tty;
mod winit_backend;
mod x11;

use clap::Parser;
use tracing_subscriber::fmt::writer::MakeWriterExt;

#[derive(Parser, Debug)]
#[command(author, version, about = "Kiosk WM — Wayland compositor for kiosk or nested desktop testing")]
pub struct Args {
    /// Terminal to launch: name, "auto", or via KIOSKWM_TERMINAL / TERMINAL
    #[arg(short, long, default_value = "auto")]
    terminal: String,

    /// UI language code (en, pt). Overrides KIOSKWM_LANG and LANG.
    #[arg(long)]
    lang: Option<String>,

    /// Do not spawn a terminal automatically
    #[arg(long)]
    no_spawn: bool,

    /// Delay in ms before spawning the terminal (wait for the socket)
    #[arg(long, default_value_t = 300)]
    spawn_delay_ms: u64,

    /// Force nested desktop mode (winit inside Plasma/Konsole)
    #[arg(long)]
    desktop: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "kioskwm=info");
    }

    let log_path = std::env::var_os("KIOSKWM_LOG").unwrap_or_else(|| "/tmp/kioskwm.log".into());
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    eprintln!("kioskwm: log em {}", log_path.to_string_lossy());
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr.and(log_file))
        .init();

    let args = Args::parse();
    let i18n = i18n::I18n::resolve(args.lang.as_deref());
    tracing::info!("UI language: {:?}", i18n.lang());

    tracing::info!("{}", env_detect::detection_debug());

    if args.desktop {
        tracing::info!("Modo desktop (winit) — forçado via --desktop");
        winit_backend::run(args, i18n)
    } else if env_detect::on_hardware_tty() {
        let tty = env_detect::controlling_tty().unwrap_or_default();
        tracing::info!("VT {tty} — modo TTY (DRM + libseat)");
        tty::run(args, i18n)
    } else {
        tracing::info!("Terminal gráfico — modo desktop (winit)");
        winit_backend::run(args, i18n)
    }
}
