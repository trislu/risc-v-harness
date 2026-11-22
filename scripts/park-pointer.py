#!/usr/bin/env python3
"""Move the X11 pointer out of the window a recording captures.

A screenshot of the page contains the composited cursor, and the pointer starts at
the top-left corner of the X display, where it lands on the title bar of every
frame. The capture script calls this once per shot; without an X display it does
nothing.
"""

import ctypes
import os
import sys

display_name = os.environ.get("DISPLAY") or ":126"
x11 = ctypes.cdll.LoadLibrary("libX11.so.6")
x11.XOpenDisplay.restype = ctypes.c_void_p
handle = x11.XOpenDisplay(display_name.encode())
if not handle:
    sys.exit(0)
display = ctypes.c_void_p(handle)
root = x11.XDefaultRootWindow(display)
x11.XWarpPointer(display, 0, ctypes.c_void_p(root), 0, 0, 0, 0, 3000, 2000)
x11.XFlush(display)
