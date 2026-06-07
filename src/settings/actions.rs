use std::process::Command;

use crate::state::{request_exit, State};

use super::layout::ConfirmAction;

pub fn execute_confirm(state: &State, action: ConfirmAction) {
    match action {
        ConfirmAction::QuitWm => {
            tracing::info!("Confirmado: fechar kioskwm");
            request_exit(&state.exit_requested);
        }
        ConfirmAction::Shutdown => {
            tracing::info!("Confirmado: desligar PC");
            if Command::new("systemctl").arg("poweroff").spawn().is_err() {
                let _ = Command::new("shutdown").args(["-h", "now"]).spawn();
            }
        }
        ConfirmAction::Reboot => {
            tracing::info!("Confirmado: reiniciar PC");
            if Command::new("systemctl").arg("reboot").spawn().is_err() {
                let _ = Command::new("shutdown").args(["-r", "now"]).spawn();
            }
        }
    }
}
