@echo off
rem PhotoCraft (EVREN eklentileriyle) - kurulu resmi PhotoCraft'tan ayri pencere olarak acilir.
cd /d "%~dp0"
if not exist "target\release\photocraft.exe" (
  echo Program henuz derlenmedi. Once: cargo build --release -p photocraft
  pause
  exit /b 1
)
start "" "target\release\photocraft.exe" --new-instance %*
