"""Windows desktop driver. Importing this package is safe on other platforms."""
from .driver import SurfaceUnavailable, WindowsDriver

__all__ = ['SurfaceUnavailable', 'WindowsDriver']
