"""Accessibility element lookup.

When the target is a native control this is strictly better than any image
detector: the system reports the exact element frame, down to a point. Games
draw their own UI and have an empty accessibility tree, so this is an
opportunistic first candidate, never the only path.

Requires the Accessibility permission; every failure mode returns ``None``
instead of raising.
"""

from __future__ import annotations

from skycore.geometry import Rect


def _unwrap(result):
    """pyobjc returns out-parameters as ``(error, value)`` tuples for these APIs."""
    if isinstance(result, tuple):
        if len(result) >= 2:
            return result[0], result[1]
        if len(result) == 1:
            return 0, result[0]
    return 0, result


def element_at(x: float, y: float) -> Rect | None:
    try:
        from ApplicationServices import (
            AXUIElementCopyAttributeValue,
            AXUIElementCopyElementAtPosition,
            AXUIElementCreateSystemWide,
            AXValueGetValue,
            kAXPositionAttribute,
            kAXSizeAttribute,
            kAXValueCGPointType,
            kAXValueCGSizeType,
        )
    except Exception:
        return None

    try:
        system = AXUIElementCreateSystemWide()
        try:
            _, element = _unwrap(AXUIElementCopyElementAtPosition(system, float(x), float(y), None))
        except TypeError:
            _, element = _unwrap(AXUIElementCopyElementAtPosition(system, float(x), float(y)))
        if element is None:
            return None

        _, position_value = _unwrap(AXUIElementCopyAttributeValue(element, kAXPositionAttribute, None))
        _, size_value = _unwrap(AXUIElementCopyAttributeValue(element, kAXSizeAttribute, None))
        if position_value is None or size_value is None:
            return None

        ok_point, point = _unwrap(AXValueGetValue(position_value, kAXValueCGPointType, None))
        ok_size, size = _unwrap(AXValueGetValue(size_value, kAXValueCGSizeType, None))
        if not ok_point or not ok_size:
            return None
        if size.width < 2 or size.height < 2:
            return None

        return Rect(float(point.x), float(point.y), float(size.width), float(size.height))
    except Exception:
        return None


def describe_element_at(x: float, y: float) -> dict:
    """Diagnostic helper for ``doctor``: what does the system see under a point?"""
    rect = element_at(x, y)
    if rect is None:
        return {"found": False}
    return {"found": True, "x": rect.x, "y": rect.y, "w": rect.w, "h": rect.h}
