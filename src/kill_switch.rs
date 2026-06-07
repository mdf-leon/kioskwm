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
}

struct ScancodeMods {
    ctrl: bool,
    alt: bool,
    shift: bool,
}

pub fn spawn(ctx: Arc<EmergencyContext>) {
    thread::Builder::new()
        .name("kioskwm-kill".into())
        .spawn(move || kill_switch_thread(ctx))
        .ok();
    tracing::info!(
        "Fallback hardware: P0=Shift+Del sai, F1-F12/0-9 troca VT; P1=Del painel"
    );
}

fn kill_switch_thread(ctx: Arc<EmergencyContext>) {
    let mut evdev_mods = EvdevMods {
        ctrl: false,
        alt: false,
        shift: false,
    };
    let mut scan_mods = ScancodeMods {
        ctrl: false,
        alt: false,
        shift: false,
    };
    let mut extended = false;

    let tty = open_controlling_tty();
    if let Some(ref t) = tty {
        set_medium_raw(t.as_raw_fd());
    }

    loop {
        let mut fds: Vec<libc::pollfd> = Vec::new();

        for dev in open_keyboard_devices() {
            fds.push(libc::pollfd {
                fd: dev.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            });
        }
        let tty_fd = tty.as_ref().map(|t| t.as_raw_fd());
        if let Some(fd) = tty_fd {
            fds.push(libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            });
        }

        if fds.is_empty() {
            thread::sleep(Duration::from_secs(2));
            continue;
        }

        let poll_ret = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as _, 500) };
        if poll_ret <= 0 {
            continue;
        }

        let evdev_count = fds.len().saturating_sub(if tty_fd.is_some() { 1 } else { 0 });
        let devices = open_keyboard_devices();
        for (i, device) in devices.iter().enumerate() {
            if fds[i].revents & libc::POLLIN == 0 {
                continue;
            }
            while let Some(ev) = read_input_event(device) {
                if ev.type_ != EV_KEY || ev.code == KEY_RESERVED {
                    continue;
                }
                update_evdev_mods(&mut evdev_mods, ev.code, ev.value != 0);
                if let Some(action) =
                    emergency::match_evdev(evdev_mods.ctrl, evdev_mods.alt, evdev_mods.shift, ev.value != 0, ev.code)
                {
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
                        if let Some(action) = match_scancode(b, extended, &mut scan_mods) {
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
                ctx.request_vt_switch(vt);
            }
        }
    }
}

fn update_evdev_mods(mods: &mut EvdevMods, code: u16, pressed: bool) {
    match code {
        29 | 97 => mods.ctrl = pressed,
        56 | 100 => mods.alt = pressed,
        42 | 54 => mods.shift = pressed,
        _ => {}
    }
}

fn match_scancode(byte: u8, extended: bool, mods: &mut ScancodeMods) -> Option<EmergencyAction> {
    if byte & 0x80 != 0 {
        match byte & 0x7F {
            0x1D | 0x9D => mods.ctrl = false,
            0x38 | 0xB8 => mods.alt = false,
            0x2A | 0x36 => mods.shift = false,
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
        _ => None,
    }
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

fn open_keyboard_devices() -> Vec<File> {
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir("/dev/input") else {
        return out;
    };
    for entry in dir.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("event") {
            continue;
        }
        let sysfs_ev = format!("/sys/class/input/{name}/device/capabilities/ev");
        if let Ok(cap) = std::fs::read_to_string(&sysfs_ev) {
            let cap = cap.trim();
            if cap.len() < 2 || !cap.chars().nth(1).is_some_and(|c| c == '1' || c > '1') {
                continue;
            }
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
