@echo off
setlocal

if "%CODEX_BIN%"=="" set "CODEX_BIN=codex.exe"

echo CODEX_BIN=%CODEX_BIN%
echo.
echo == host-context ==
whoami
whoami /groups
powershell -NoProfile -Command "Write-Output ('SESSION=' + [System.Diagnostics.Process]::GetCurrentProcess().SessionId)"
quser 2>nul
echo.

echo == direct-powershell ==
"%CODEX_BIN%" sandbox windows powershell.exe -NoProfile -Command "Write-Output ok"
echo EXIT=%ERRORLEVEL%
echo.

echo == direct-dotnet ==
"%CODEX_BIN%" sandbox windows dotnet.exe --info
echo EXIT=%ERRORLEVEL%
echo.

echo == direct-cmd ==
"%CODEX_BIN%" sandbox windows cmd.exe /c echo ok
echo EXIT=%ERRORLEVEL%
echo.

echo == direct-reg ==
"%CODEX_BIN%" sandbox windows reg.exe query HKCU\Environment
echo EXIT=%ERRORLEVEL%
echo.

echo == child-powershell-via-cmd ==
"%CODEX_BIN%" sandbox windows cmd.exe /c powershell.exe -NoProfile -Command "Write-Output ok"
echo EXIT=%ERRORLEVEL%
echo.

echo == child-dotnet-via-cmd ==
"%CODEX_BIN%" sandbox windows cmd.exe /c dotnet.exe --info
echo EXIT=%ERRORLEVEL%
echo.

echo == child-whoami-groups-via-cmd ==
"%CODEX_BIN%" sandbox windows cmd.exe /c whoami /groups
echo EXIT=%ERRORLEVEL%
