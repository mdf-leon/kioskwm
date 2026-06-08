//! UI strings — default English; override via `--lang`, `KIOSKWM_LANG` or `LANG`.

use crate::settings::ConfirmAction;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Lang {
    #[default]
    En,
    Pt,
}

#[derive(Debug, Clone, Copy)]
pub struct I18n {
    lang: Lang,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Msg {
    Back,
    Settings,
    MostUsedPages,
    Mouse,
    FooterQuitWm,
    FooterShutDown,
    FooterRestart,
    PointerSpeed,
    MouseFooterHint,
    Cancel,
    Confirm,
    OpenSettings,
    CloseApp,
    Main,
}

impl I18n {
    pub fn new(lang: Lang) -> Self {
        Self { lang }
    }

    pub fn lang(&self) -> Lang {
        self.lang
    }

    pub fn cache_tag(&self) -> u8 {
        self.lang as u8
    }

    /// CLI `--lang` wins, then `KIOSKWM_LANG`. Does not follow LANG (stay English on localized OS).
    pub fn resolve(cli_lang: Option<&str>) -> Self {
        if let Some(s) = cli_lang.filter(|s| !s.is_empty()) {
            return Self::new(parse_lang(s));
        }
        if let Ok(v) = std::env::var("KIOSKWM_LANG") {
            return Self::new(parse_lang(&v));
        }
        Self::new(Lang::En)
    }

    pub fn t(self, msg: Msg) -> &'static str {
        match (self.lang, msg) {
            (Lang::En, Msg::Back) => "Back",
            (Lang::En, Msg::Settings) => "Settings",
            (Lang::En, Msg::MostUsedPages) => "Most used",
            (Lang::En, Msg::Mouse) => "Mouse",
            (Lang::En, Msg::FooterQuitWm) => "Quit WM",
            (Lang::En, Msg::FooterShutDown) => "Shut down",
            (Lang::En, Msg::FooterRestart) => "Restart",
            (Lang::En, Msg::PointerSpeed) => "Pointer speed",
            (Lang::En, Msg::MouseFooterHint) => "Drag the slider. Esc or Back: main menu.",
            (Lang::En, Msg::Cancel) => "Cancel",
            (Lang::En, Msg::Confirm) => "Confirm",
            (Lang::En, Msg::OpenSettings) => "Open settings",
            (Lang::En, Msg::CloseApp) => "Close window",
            (Lang::En, Msg::Main) => "Main",

            (Lang::Pt, Msg::Back) => "Voltar",
            (Lang::Pt, Msg::Settings) => "Ajustes",
            (Lang::Pt, Msg::MostUsedPages) => "Paginas mais usadas",
            (Lang::Pt, Msg::Mouse) => "Mouse",
            (Lang::Pt, Msg::FooterQuitWm) => "Fechar WM",
            (Lang::Pt, Msg::FooterShutDown) => "Desligar",
            (Lang::Pt, Msg::FooterRestart) => "Reiniciar",
            (Lang::Pt, Msg::PointerSpeed) => "Velocidade do ponteiro",
            (Lang::Pt, Msg::MouseFooterHint) => "Arraste o controle. Esc ou Voltar: menu principal.",
            (Lang::Pt, Msg::Cancel) => "Cancelar",
            (Lang::Pt, Msg::Confirm) => "Confirmar",
            (Lang::Pt, Msg::OpenSettings) => "Abrir ajustes",
            (Lang::Pt, Msg::CloseApp) => "Fechar janela",
            (Lang::Pt, Msg::Main) => "Principal",
        }
    }

    pub fn confirm_dialog(self, action: ConfirmAction) -> (&'static str, &'static str) {
        match (self.lang, action) {
            (Lang::En, ConfirmAction::QuitWm) => (
                "Quit kioskwm?",
                "The Wayland compositor will exit.",
            ),
            (Lang::En, ConfirmAction::Shutdown) => (
                "Shut down the computer?",
                "All programs will be closed.",
            ),
            (Lang::En, ConfirmAction::Reboot) => (
                "Restart the computer?",
                "All programs will be closed.",
            ),
            (Lang::Pt, ConfirmAction::QuitWm) => (
                "Fechar kioskwm?",
                "O compositor Wayland sera encerrado.",
            ),
            (Lang::Pt, ConfirmAction::Shutdown) => (
                "Desligar o computador?",
                "Todos os programas serao fechados.",
            ),
            (Lang::Pt, ConfirmAction::Reboot) => (
                "Reiniciar o computador?",
                "Todos os programas serao fechados.",
            ),
        }
    }
}

fn parse_lang(s: &str) -> Lang {
    let code = s.to_lowercase();
    let code = code.split(['_', '-']).next().unwrap_or(&code);
    match code {
        "pt" | "br" => Lang::Pt,
        _ => Lang::En,
    }
}
