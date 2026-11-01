"""macOS device driver: capture, injection, window targeting, AX lookup.

Everything OS specific lives here; the matching and the interpreter stay in
``skycore`` so the Windows/Android/iOS drivers can be written against the same
protocol without touching the engine.
"""
