mod cursor;
mod env_detect;
mod input;
mod render;
mod spawn;
mod state;
mod tty;
mod winit_backend;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about = "Kiosk WM — compositor Wayland para kiosk ou teste no desktop")]
pub struct Args {
    /// Terminal a abrir: nome, "auto", ou via env KIOSKWM_TERMINAL / TERMINAL
    #[arg(short, long, default_value = "auto")]
    terminal: String,

    /// Não abre terminal automaticamente (útil para testar manualmente)
    #[arg(long)]
    no_spawn: bool,

    /// Atraso em ms antes de abrir o terminal (espera o socket ficar pronto)
    #[arg(long, default_value_t = 300)]
    spawn_delay_ms: u64,

    /// Força modo desktop (winit aninhado no Plasma/Konsole)
    #[arg(long)]
    desktop: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "kioskwm=info");
    }
    tracing_subscriber::fmt().init();

    let args = Args::parse();

    tracing::info!("{}", env_detect::detection_debug());

    if args.desktop {
        tracing::info!("Modo desktop (winit) — forçado via --desktop");
        winit_backend::run(args)
    } else if env_detect::on_hardware_tty() {
        let tty = env_detect::controlling_tty().unwrap_or_default();
        tracing::info!("VT {tty} — modo TTY (DRM + libseat)");
        tty::run(args)
    } else {
        tracing::info!("Terminal gráfico — modo desktop (winit)");
        winit_backend::run(args)
    }
}
