//! Accessibility element lookup.
//!
//! When the target is a native control this is strictly better than any image
//! detector: the system reports the exact element frame. Games draw their own UI
//! and have an empty accessibility tree, so this is an opportunistic first
//! candidate, never the only path. Every failure mode returns `None`.

use std::ffi::c_void;

use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
use core_foundation::string::{CFString, CFStringRef};
use core_graphics::geometry::{CGPoint, CGSize};
use liverguard_ir::Rect;

type AXUIElementRef = *const c_void;
type AXError = i32;
type AXValueRef = *const c_void;

const AX_SUCCESS: AXError = 0;
const AX_VALUE_CGPOINT: i32 = 1;
const AX_VALUE_CGSIZE: i32 = 2;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCopyElementAtPosition(application: AXUIElementRef, x: f32, y: f32, element: *mut AXUIElementRef) -> AXError;
    fn AXUIElementCopyAttributeValue(element: AXUIElementRef, attribute: CFStringRef, value: *mut CFTypeRef) -> AXError;
    fn AXValueGetValue(value: AXValueRef, kind: i32, out: *mut c_void) -> bool;
}

/// Owns a retained CF object and releases it on drop.
struct Owned(CFTypeRef);

impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) };
        }
    }
}

fn attribute(element: AXUIElementRef, name: &str) -> Option<Owned> {
    let key = CFString::new(name);
    let mut value: CFTypeRef = std::ptr::null();
    let status = unsafe { AXUIElementCopyAttributeValue(element, key.as_concrete_TypeRef(), &mut value) };
    (status == AX_SUCCESS && !value.is_null()).then(|| Owned(value))
}

/// Frame of the UI element under a global point, if the system exposes one.
pub fn element_at(x: f64, y: f64) -> Option<Rect> {
    unsafe {
        let system = Owned(AXUIElementCreateSystemWide());
        if system.0.is_null() {
            return None;
        }
        let mut element: AXUIElementRef = std::ptr::null();
        if AXUIElementCopyElementAtPosition(system.0, x as f32, y as f32, &mut element) != AX_SUCCESS || element.is_null() {
            return None;
        }
        let element = Owned(element);

        let position = attribute(element.0, "AXPosition")?;
        let size = attribute(element.0, "AXSize")?;
        let mut point = CGPoint::new(0.0, 0.0);
        let mut extent = CGSize::new(0.0, 0.0);
        if !AXValueGetValue(position.0, AX_VALUE_CGPOINT, &mut point as *mut _ as *mut c_void)
            || !AXValueGetValue(size.0, AX_VALUE_CGSIZE, &mut extent as *mut _ as *mut c_void)
        {
            return None;
        }
        // Zero-sized elements (containers, separators) are not clickable targets.
        (extent.width >= 2.0 && extent.height >= 2.0).then(|| Rect::new(point.x, point.y, extent.width, extent.height))
    }
}
