# Launcher interface

`index.html` is the independently written Free Flight frontend. The Windows
host in `crates/fsx-ui` embeds it at compile time. Installed UI skins and
aircraft resources are read from the user's FSX folder at runtime. The main preview is a native child window running the shared MDLX/DDS renderer, with slow rotation, drag orbit and wheel zoom. Aircraft-picker tiles use installed thumbnails. Unsupported visuals show an error instead of retaining the previous aircraft.

This directory follows openOMSI's separation of the launcher frontend from
Rust engine crates. openFSX currently uses a WebView2 host; no openOMSI frontend
or launcher implementation is included. It is not a website publishing root.
