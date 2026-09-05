// window.rs — window lifecycle helpers
//
// Encapsulates the show/center/focus sequence derived from the validated spike.
// Keeping this in its own module satisfies SRP: main.rs handles app setup;
// this module handles window positioning.
//
// Slice B: position is now config-driven (Center / BottomCenter / NearMouse).

use tauri::{AppHandle, Manager, PhysicalPosition};
use ui_lib::config::Position;

/// Show the overlay and position it according to `position`, then claim focus.
///
/// Sequence (validated in `spikes/focus-spike`):
///   1. `set_position()` — move BEFORE making visible (no flash at old spot).
///   2. `show()`         — make the window visible.
///   3. `set_always_on_top` — bring above all other windows.
///   4. `set_focus()`    — transfer OS keyboard focus.
///
/// Position variants:
///   - `Center`       — center of the monitor under the cursor (Slice A behaviour).
///   - `BottomCenter` — horizontally centered, near bottom of that monitor.
///   - `NearMouse`    — near the cursor position itself.
pub fn show_and_focus(app: &AppHandle, position: &Position) {
    let Some(win) = app.get_webview_window("main") else {
        eprintln!("[ui] show_and_focus: window 'main' not found");
        return;
    };

    // Step 1: position BEFORE showing.
    if let Err(e) = apply_position(&win, position) {
        eprintln!("[ui] position warning (non-fatal): {e}");
    }

    // Step 2: make visible.
    if let Err(e) = win.show() {
        eprintln!("[ui] show() error: {e}");
        return;
    }

    // Step 3: bring to the top of the z-order.
    if let Err(e) = win.set_always_on_top(true) {
        eprintln!("[ui] set_always_on_top() error: {e}");
    }

    // Step 4: request OS keyboard focus.
    if let Err(e) = win.set_focus() {
        eprintln!("[ui] set_focus() error: {e}");
    }
}

/// Compute and apply the desired position for the overlay window.
fn apply_position(
    win: &tauri::WebviewWindow,
    position: &Position,
) -> Result<(), Box<dyn std::error::Error>> {
    match position {
        Position::Center => center_on_active_monitor(win),
        Position::BottomCenter => bottom_center_on_active_monitor(win),
        Position::NearMouse => near_mouse(win),
    }
}

/// Center the window on the monitor that currently holds the cursor.
///
/// Fallback chain:
///   1. Monitor whose bounds contain the cursor position.
///   2. `win.primary_monitor()` — the OS-designated primary display.
///   3. Skip positioning (return Ok).
fn center_on_active_monitor(win: &tauri::WebviewWindow) -> Result<(), Box<dyn std::error::Error>> {
    let target = active_monitor(win)?;
    let Some(monitor) = target else {
        return Ok(());
    };

    let mpos = monitor.position();
    let msize = monitor.size();
    let wsize = win.outer_size()?;

    let cx = mpos.x + (msize.width as i32 - wsize.width as i32) / 2;
    let cy = mpos.y + (msize.height as i32 - wsize.height as i32) / 2;

    win.set_position(PhysicalPosition::new(cx, cy))?;
    Ok(())
}

/// Horizontally center the window; vertically place it near the bottom of the
/// monitor that currently holds the cursor (with a small margin).
fn bottom_center_on_active_monitor(
    win: &tauri::WebviewWindow,
) -> Result<(), Box<dyn std::error::Error>> {
    const BOTTOM_MARGIN: i32 = 60; // pixels above the taskbar / screen edge

    let target = active_monitor(win)?;
    let Some(monitor) = target else {
        return Ok(());
    };

    let mpos = monitor.position();
    let msize = monitor.size();
    let wsize = win.outer_size()?;

    let cx = mpos.x + (msize.width as i32 - wsize.width as i32) / 2;
    let cy = mpos.y + msize.height as i32 - wsize.height as i32 - BOTTOM_MARGIN;

    win.set_position(PhysicalPosition::new(cx, cy))?;
    Ok(())
}

/// Place the overlay near the current cursor position (offset so the cursor
/// is not hidden under the window).
fn near_mouse(win: &tauri::WebviewWindow) -> Result<(), Box<dyn std::error::Error>> {
    const OFFSET_X: i32 = 16; // px right of cursor
    const OFFSET_Y: i32 = 8; // px below cursor

    let cursor = match win.cursor_position() {
        Ok(pos) => pos,
        Err(_) => {
            // Cursor position unavailable — fall back to centering.
            return center_on_active_monitor(win);
        }
    };

    // Clamp to keep the window fully on screen.
    let monitors = win.available_monitors()?;
    let wsize = win.outer_size()?;

    // Find the monitor under the cursor.
    let monitor = monitors.into_iter().find(|m| {
        let mpos = m.position();
        let msize = m.size();
        cursor.x >= mpos.x as f64
            && cursor.x < (mpos.x + msize.width as i32) as f64
            && cursor.y >= mpos.y as f64
            && cursor.y < (mpos.y + msize.height as i32) as f64
    });

    let (x, y) = if let Some(m) = monitor {
        let mpos = m.position();
        let msize = m.size();
        let raw_x = cursor.x as i32 + OFFSET_X;
        let raw_y = cursor.y as i32 + OFFSET_Y;

        // Clamp so window stays within monitor bounds.
        let x = raw_x.min(mpos.x + msize.width as i32 - wsize.width as i32);
        let y = raw_y.min(mpos.y + msize.height as i32 - wsize.height as i32);
        (x, y)
    } else {
        // No monitor found — place at cursor without clamping.
        (cursor.x as i32 + OFFSET_X, cursor.y as i32 + OFFSET_Y)
    };

    win.set_position(PhysicalPosition::new(x, y))?;
    Ok(())
}

/// Return the monitor that currently holds the cursor, falling back to primary.
fn active_monitor(
    win: &tauri::WebviewWindow,
) -> Result<Option<tauri::Monitor>, Box<dyn std::error::Error>> {
    let monitors = win.available_monitors()?;
    let cursor_pos = win.cursor_position().ok();

    let target = cursor_pos
        .and_then(|pos| {
            monitors.into_iter().find(|m| {
                let mpos = m.position();
                let msize = m.size();
                let x = mpos.x as f64;
                let y = mpos.y as f64;
                let w = msize.width as f64;
                let h = msize.height as f64;
                pos.x >= x && pos.x < x + w && pos.y >= y && pos.y < y + h
            })
        })
        .or_else(|| win.primary_monitor().ok().flatten());

    Ok(target)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
// The GUI is tested interactively (Slice A exception to TDD per spec).
// Pure logic (no Tauri mock needed) is extracted to config.rs and tested there.
