//! UI Automation element lookup.
//!
//! Native controls report an exact frame; games draw their own UI and expose
//! nothing, so this is an opportunistic first candidate. Every failure is `None`.

use liverguard_ir::Rect;
use windows::Win32::Foundation::POINT;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation};

pub fn element_at(x: f64, y: f64) -> Option<Rect> {
    unsafe {
        // Already-initialised (S_FALSE) and mode-mismatch results are fine: COM is usable either way.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let automation: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
        let element = automation.ElementFromPoint(POINT { x: x.round() as i32, y: y.round() as i32 }).ok()?;
        let rect = element.CurrentBoundingRectangle().ok()?;
        let (w, h) = ((rect.right - rect.left) as f64, (rect.bottom - rect.top) as f64);
        (w >= 2.0 && h >= 2.0).then(|| Rect::new(rect.left as f64, rect.top as f64, w, h))
    }
}
