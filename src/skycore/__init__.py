"""skycore: platform independent logic shared by every device client.

Nothing in this package may import a platform specific module (pyobjc, mss,
Windows API). That boundary is what lets the Windows, Android and iOS clients
reuse the same behaviour instead of re-deriving it.
"""
