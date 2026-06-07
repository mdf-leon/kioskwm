//! Thread evdev/tty: fallback P0/P1 quando o event loop do compositor nao responde.
//! O caminho principal e o filtro de teclado do compositor (`emergency.rs`).

use std::{
    fs::{File, OpenOptions},
    os::fd::AsRawFd,
    sync::Arc,
    thread,
    time::Duration,
};

use crate::{
    emergency::{self, EmergencyAction, EmergencyContext},
    env_detect,
};

const EV_KEY: u16 = 1;
const KEY_RESERVED: u16 = 0;

#[repr(C)]
struct InputEvent {
    time: libc::timeval,
    type_: u16,
    code: u16,
    value: i32,
}

struct EvdevMods {
    ctrl: bool,
    alt: bool,
    shift: bool,
    meta: bool,
}

struct ScancodeMods {
    ctrl: bool,
    alt: bool,
    shift: bool,
    meta: bool,
}

const BTN_RIGHT: u16 = 0x111;

pub fn spawn(
    ctx: Arc<EmergencyContext>,
    mod_tracker: Arc<crate::modifiers::ModifierTracker>,
    hardware: Arc<crate::hardware_bridge::HardwareBridge>,
) {
    let nested = env_detect::parent_steals_global_shortcuts();
    thread::Builder::new()
        .name("kioskwm-kill".into())
        .spawn(move || kill_switch_thread(ctx, mod_tracker, hardware, nested))
        .ok();
    if nested {
        tracing::info!(
            "Fallback evdev (sessão aninhada): P0=Shift+Del sai, F1-F12/0-9 VT; \
             P1=Del/Ctrl+Shift+Esc/Super+Esc painel"
        );
    } else {
        tracing::info!(
            "Fallback hardware: P0=Shift+Del sai, F1-F12/0-9 VT; \
             P1=Del/Ctrl+Shift+Esc/Super+Esc painel"
        );
    }
}

fn kill_switch_thread(
    ctx: Arc<EmergencyContext>,
    mod_tracker: Arc<crate::modifiers::ModifierTracker>,
    hardware: Arc<crate::hardware_bridge::HardwareBridge>,
    nested: bool,
) {
    let mut evdev_mods = EvdevMods {
        ctrl: false,
        alt: false,
        shift: false,
        meta: false,
    };
    let mut scan_mods = ScancodeMods {
        ctrl: false,
        alt: false,
        shift: false,
        meta: false,
    };
    let mut extended = false;

    let input_devices = open_input_devices();
    let use_tty_fallback = input_devices.is_empty();

    let tty = if use_tty_fallback {
        open_controlling_tty()
    } else {
        None
    };
    if let Some(ref t) = &tty {
        set_medium_raw(t.as_raw_fd());
    }

    loop {
        let mut fds: Vec<libc::pollfd> = Vec::new();

        for dev in &input_devices {
            fds.push(libc::pollfd {
                fd: dev.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            });
        }
        let tty_fd = tty.as_ref().map(|t| t.as_raw_fd());
        if use_tty_fallback {
            if let Some(fd) = tty_fd {
                fds.push(libc::pollfd {
                    fd,
                    events: libc::POLLIN,
                    revents: 0,
                });
            }
        }

        if fds.is_empty() {
            if nested {
                tracing::warn!(
                    "Fallback evdev sem /dev/input — atalhos P0/P1 podem não funcionar no desktop. \
                     Adicione seu usuário ao grupo 'input' ou rode com acesso ao teclado."
                );
            }
            thread::sleep(Duration::from_secs(2));
            continue;
        }

        let poll_ret = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as _, 500) };
        if poll_ret <= 0 {
            continue;
        }

        let evdev_count = fds.len().saturating_sub(if use_tty_fallback && tty_fd.is_some() {
            1
        } else {
            0
        });
        for (i, device) in input_devices.iter().enumerate() {
            if fds[i].revents & libc::POLLIN == 0 {
                continue;
            }
            while let Some(ev) = read_input_event(device) {
                if ev.type_ != EV_KEY || ev.code == KEY_RESERVED {
                    continue;
                }
                update_evdev_mods(&mut evdev_mods, ev.code, ev.value != 0);
                mod_tracker.set_evdev_super(evdev_mods.meta);

                if ev.code == BTN_RIGHT && ev.value == 1 && evdev_mods.meta {
                    let (x, y) = hardware.pointer();
                    ctx.menu.request_open(x, y);
                    continue;
                }

                if let Some(action) = emergency::match_evdev(
                    evdev_mods.ctrl,
                    evdev_mods.alt,
                    evdev_mods.shift,
                    evdev_mods.meta,
                    ev.value != 0,
                    ev.code,
                ) {
                    dispatch_hardware(action, &ctx);
                }
            }
        }

        if let (Some(t), Some(fd)) = (&tty, tty_fd) {
            let tty_idx = evdev_count;
            if fds.get(tty_idx).is_some_and(|f| f.revents & libc::POLLIN != 0) {
                let mut buf = [0u8; 64];
                let n = unsafe {
                    libc::read(
                        fd,
                        buf.as_mut_ptr() as *mut libc::c_void,
                        buf.len(),
                    )
                };
                if n > 0 {
                    for &b in &buf[..n as usize] {
                        if b == 0xE0 {
                            extended = true;
                            continue;
                        }
                        if let Some(action) =
                            match_scancode(b, extended, &mut scan_mods, &mod_tracker)
                        {
                            dispatch_hardware(action, &ctx);
                        }
                        extended = false;
                    }
                }
                let _ = t;
            }
        }
    }
}

fn dispatch_hardware(action: EmergencyAction, ctx: &EmergencyContext) {
    match action {
        EmergencyAction::ForceQuit => emergency::force_quit(&ctx.exit_flag),
        EmergencyAction::ToggleOverlay => {
            if emergency::try_p1_debounce() {
                ctx.overlay.request_toggle();
            }
        }
        EmergencyAction::SwitchVt(vt) => {
            if emergency::try_p0_vt_debounce() {
                tracing::info!("P0 — evdev troca imediata para tty{vt}");
                emergency::do_vt_switch(vt);
            }
        }
    }
}

fn update_evdev_mods(mods: &mut EvdevMods, code: u16, pressed: bool) {
    match code {
        29 | 97 => mods.ctrl = pressed,
        56 | 100 => mods.alt = pressed,
        42 | 54 => mods.shift = pressed,
        125 | 126 => mods.meta = pressed,
        _ => {}
    }
}

fn match_scancode(
    byte: u8,
    extended: bool,
    mods: &mut ScancodeMods,
    mod_tracker: &crate::modifiers::ModifierTracker,
) -> Option<EmergencyAction> {
    if byte & 0x80 != 0 {
        match byte & 0x7F {
            0x1D | 0x9D => mods.ctrl = false,
            0x38 | 0xB8 => mods.alt = false,
            0x2A | 0x36 => mods.shift = false,
            0x5B | 0x5C if extended => {
                mods.meta = false;
                mod_tracker.set_evdev_super(false);
            }
            _ => {}
        }
        return None;
    }
    match byte {
        0x1D | 0x9D => {
            mods.ctrl = true;
            None
        }
        0x38 | 0xB8 => {
            mods.alt = true;
            None
        }
        0x2A | 0x36 => {
            mods.shift = true;
            None
        }
        0x5B | 0x5C if extended => {
            mods.meta = true;
            mod_tracker.set_evdev_super(true);
            None
        }
        0x01 if mods.ctrl && mods.shift && !mods.alt => Some(EmergencyAction::ToggleOverlay),
        0x01 if mods.meta => Some(EmergencyAction::ToggleOverlay),
        0x53 => {
            let _ = extended;
            if mods.ctrl && mods.alt && mods.shift {
                Some(EmergencyAction::ForceQuit)
            } else if mods.ctrl && mods.alt {
                Some(EmergencyAction::ToggleOverlay)
            } else {
                None
            }
        }
        0x3B..=0x44 if mods.ctrl && mods.alt && !mods.shift => {
            Some(EmergencyAction::SwitchVt((byte - 0x3B + 1) as u8))
        }
        0x02..=0x0A if mods.ctrl && mods.alt && !mods.shift => {
            Some(EmergencyAction::SwitchVt((byte - 0x02 + 1) as u8))
        }
        0x0B if mods.ctrl && mods.alt && !mods.shift => Some(EmergencyAction::SwitchVt(10)),
        _ => None,
    }
}

fn is_input_device(event_name: &str) -> bool {
    let sysfs_ev = format!("/sys/class/input/{event_name}/device/capabilities/ev");
    let sysfs_key = format!("/sys/class/input/{event_name}/device/capabilities/key");
    let has_ev_key = std::fs::read_to_string(&sysfs_ev)
        .ok()
        .is_some_and(|cap| {
            let cap = cap.trim();
            cap.len() >= 2 && cap.chars().nth(1).is_some_and(|c| c == '1' || c > '1')
        });
    let has_keys = std::fs::read_to_string(&sysfs_key)
        .ok()
        .is_some_and(|cap| cap.trim().chars().any(|c| c != '0'));
    has_ev_key && has_keys
}

fn open_controlling_tty() -> Option<File> {
    let path = env_detect::controlling_tty().unwrap_or_else(|| "/dev/tty".to_string());
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .ok()
}

fn set_medium_raw(fd: i32) -> bool {
    const KDSKBMODE: libc::c_ulong = 0x4B4B;
    const K_MEDIUMRAW: libc::c_ulong = 1;
    unsafe { libc::ioctl(fd, KDSKBMODE, K_MEDIUMRAW) == 0 }
}

fn open_input_devices() -> Vec<File> {
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir("/dev/input") else {
        return out;
    };
    for entry in dir.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("event") || !is_input_device(&name) {
            continue;
        }
        if let Ok(f) = OpenOptions::new().read(true).open(entry.path()) {
            out.push(f);
        }
    }
    out
}

fn read_input_event(file: &File) -> Option<InputEvent> {
    let mut ev = InputEvent {
        time: unsafe { std::mem::zeroed() },
        type_: 0,
        code: 0,
        value: 0,
    };
    let size = std::mem::size_of::<InputEvent>();
    let ptr = &mut ev as *mut InputEvent as *mut libc::c_void;
    let n = unsafe { libc::read(file.as_raw_fd(), ptr, size) };
    if n as usize != size {
        return None;
    }
    Some(ev)
}
