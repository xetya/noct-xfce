//! X11 helpers for XFCE and other light desktops (xfwm4, Openbox, ...).
//!
//! - `reserve_top`: tells the window manager the bar owns the top strip, so
//!   maximised windows and the XFCE panel do not slide underneath it.
//! - `compositing`: without a compositor (common on XFCE) transparent windows
//!   draw as black boxes, so `blit` flattens the picture onto a dark colour.
//! - `window_types`: `NOCK_WINDOW_TYPE=normal|utility` is an escape hatch for
//!   window managers that mishandle dock windows.
//! - `doctor`: `nock --doctor` prints what this machine offers, for bug reports.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use winit::platform::x11::WindowType;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, PropMode};

/// True when there is no compositor and alpha must be flattened.
pub static OPAQUE: AtomicBool = AtomicBool::new(false);

pub fn debug() -> bool {
    static D: OnceLock<bool> = OnceLock::new();
    *D.get_or_init(|| std::env::var_os("NOCK_DEBUG").is_some())
}

pub fn window_types() -> Vec<WindowType> {
    match std::env::var("NOCK_WINDOW_TYPE").as_deref() {
        Ok("normal") => vec![WindowType::Normal],
        Ok("utility") => vec![WindowType::Utility, WindowType::Normal],
        _ => vec![WindowType::Dock],
    }
}

fn xid(w: &Window) -> Option<u32> {
    match w.window_handle().ok()?.as_raw() {
        RawWindowHandle::Xlib(h) => Some(h.window as u32),
        RawWindowHandle::Xcb(h) => Some(h.window.get()),
        _ => None,
    }
}

/// Is a compositing manager running on the default screen?
pub fn compositing() -> bool {
    let Ok((conn, screen)) = x11rb::connect(None) else { return true };
    let name = format!("_NET_WM_CM_S{screen}");
    let Some(atom) = conn.intern_atom(false, name.as_bytes()).ok().and_then(|c| c.reply().ok()).map(|r| r.atom) else {
        return true;
    };
    conn.get_selection_owner(atom).ok().and_then(|c| c.reply().ok()).map_or(true, |r| r.owner != 0)
}

/// Set up once at start: decide whether alpha can be used.
pub fn init() {
    let solid = std::env::var_os("NOCK_FORCE_ALPHA").is_none() && !compositing();
    OPAQUE.store(solid, Ordering::Relaxed);
    if debug() {
        eprintln!("nock: compositor {}", if solid { "NOT running, drawing opaque" } else { "running, drawing with alpha" });
    }
}

/// Reserve `height` physical pixels along the top edge, from `x` for `width` pixels.
pub fn reserve_top(w: &Window, height: u32, x: u32, width: u32) {
    if std::env::var_os("NOCK_NO_STRUT").is_some() {
        return;
    }
    let Some(id) = xid(w) else { return };
    let Ok((conn, _)) = x11rb::connect(None) else { return };
    let set = |name: &str, vals: &[u32]| {
        let Some(atom) = conn.intern_atom(false, name.as_bytes()).ok().and_then(|c| c.reply().ok()).map(|r| r.atom) else {
            return;
        };
        let _ = conn.change_property32(PropMode::REPLACE, id, atom, AtomEnum::CARDINAL, vals);
    };
    set("_NET_WM_STRUT", &[0, 0, height, 0]);
    set("_NET_WM_STRUT_PARTIAL", &[0, 0, height, 0, 0, 0, 0, 0, x, x + width.saturating_sub(1), 0, 0]);
    let _ = conn.flush();
    if debug() {
        eprintln!("nock: reserved {height}px at the top (window {id:#x})");
    }
}

fn have(cmd: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(cmd).is_file()))
}

/// `nock --doctor`: what the bar can use on this machine.
pub fn doctor() {
    let env = |k: &str| std::env::var(k).unwrap_or_else(|_| "-".into());
    println!("Nock doctor");
    println!("  session       {} (DISPLAY={}, WAYLAND_DISPLAY={})", env("XDG_SESSION_TYPE"), env("DISPLAY"), env("WAYLAND_DISPLAY"));
    println!("  desktop       {}", env("XDG_CURRENT_DESKTOP"));
    println!("  compositor    {}", if compositing() { "running" } else { "NOT running (Nock draws opaque)" });
    println!("  tools:");
    for t in ["xdotool", "nmcli", "bluetoothctl", "wpctl", "pactl", "brightnessctl", "curl", "xdg-open", "loginctl"] {
        println!("    {:<14} {}", t, if have(t) { "found" } else { "missing" });
    }
    println!("  live readings (what the bar will show):");
    println!("    focused app  {:?}", crate::sys::active_app());
    println!("    wifi         present={} name={:?}", crate::sys::wifi_present(), crate::sys::wifi_name());
    println!("    bluetooth    {:?}", crate::sys::bt_powered());
    println!("    volume       {:?}", crate::sys::volume());
    println!("    brightness   {:?}", crate::sys::brightness());
    println!("    battery      {:?}", crate::sys::battery_info());
    println!("Run `NOCK_DEBUG=1 nock` to log window and mouse events.");
}
