@echo off
setlocal
cd /d "%~dp0"

REM Debug launcher: keeps a console so log lines are visible live. The server
REM opens a tray icon either way; this window's output is the only difference,
REM and closing it stops the server.
REM
REM To run normally, double-click kroma-server.exe (no console, tray icon).
REM
REM Optional overrides, set before starting:
REM   set "KROMA_PORT=8080"
REM   set "KROMA_MEDIA_DIRS=D:\Movies,D:\Shows"

"%~dp0kroma-server.exe"

endlocal