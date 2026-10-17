@echo off
setlocal
pushd "%~dp0.."
cargo build --locked --release -p fsx-app --features viewer --bin openfsx-viewer
if errorlevel 1 goto failed
cargo build --locked --release -p fsx-check --bin openfsx
if errorlevel 1 goto failed
echo Built target\release\openfsx-viewer.exe and openfsx.exe. Game content stays in your own installation.
popd
exit /b 0
:failed
popd
exit /b 1
