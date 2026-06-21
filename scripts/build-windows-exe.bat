@echo off
setlocal

set "SCRIPT_DIR=%~dp0"
pushd "%SCRIPT_DIR%\.." >nul || exit /b 1

where cargo >nul 2>nul
if errorlevel 1 (
    echo [error] cargo was not found in PATH.
    echo Install Rust from https://rustup.rs/ and reopen this terminal.
    popd >nul
    exit /b 1
)

if not "%PROTOC%"=="" (
    if exist "%PROTOC%" goto protoc_ok
)

where protoc >nul 2>nul
if errorlevel 1 (
    echo [error] protoc was not found in PATH and PROTOC does not point to protoc.exe.
    echo Install Google.Protobuf with winget, add protoc.exe to PATH, or set PROTOC.
    popd >nul
    exit /b 1
)
:protoc_ok

set "PROFILE_ARG=--release"
set "PROFILE_DIR=release"
if /I "%~1"=="--debug" (
    set "PROFILE_ARG="
    set "PROFILE_DIR=debug"
)

set "TARGET_ARG="
set "TARGET_DIR="
if not "%NEXSHELL_WINDOWS_TARGET%"=="" (
    set "TARGET_ARG=--target %NEXSHELL_WINDOWS_TARGET%"
    set "TARGET_DIR=%NEXSHELL_WINDOWS_TARGET%\"
)

echo Building NexShell native shell exe...
cargo build %PROFILE_ARG% --features warpui-app --bin nexshell %TARGET_ARG%
if errorlevel 1 (
    popd >nul
    exit /b 1
)

set "EXE_PATH=target\%TARGET_DIR%%PROFILE_DIR%\nexshell.exe"
echo.
echo Built: %CD%\%EXE_PATH%

popd >nul
endlocal
