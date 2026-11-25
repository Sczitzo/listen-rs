@echo off
rem Package Listen (Rust build) with its speech runtime and model into dist\Listen,
rem ready to run locally or zip for GitHub Releases.
setlocal
cd /d "%~dp0"

set "OUT=dist\Listen"
set "RUNTIME=%LOCALAPPDATA%\Listen\runtimes\transcribe-0.4.2-cpu-vulkan"
if not exist "%RUNTIME%\transcribe.dll" (
  if exist "%OUT%\runtime\transcribe-0.4.2-cpu-vulkan\transcribe.dll" (
    set "RUNTIME=%CD%\%OUT%\runtime\transcribe-0.4.2-cpu-vulkan"
  )
)

set "MODEL=%USERPROFILE%\.cache\huggingface\hub\models--handy-computer--parakeet-unified-en-0.6b-gguf\snapshots\7e948f21b7bdbac698d3318db9d350f1096f3b6c\parakeet-unified-en-0.6b-Q8_0.gguf"
if not exist "%MODEL%" (
  if exist "%OUT%\models\parakeet-unified-en-0.6b-Q8_0.gguf" (
    set "MODEL=%CD%\%OUT%\models\parakeet-unified-en-0.6b-Q8_0.gguf"
  )
)

if not exist "%RUNTIME%\transcribe.dll" (
  echo Speech runtime not found at "%RUNTIME%".
  echo Place transcribe-0.4.2-cpu-vulkan in %%LOCALAPPDATA%%\Listen\runtimes\ or dist\Listen\runtime\.
  exit /b 1
)
if not exist "%MODEL%" (
  echo Speech model not found at "%MODEL%".
  echo Download parakeet-unified-en-0.6b-Q8_0.gguf into dist\Listen\models\ or HuggingFace cache.
  exit /b 1
)

cargo build --release --locked || exit /b 1

set "STAGE=dist\_stage_Listen"
if exist "%STAGE%" rmdir /s /q "%STAGE%"
mkdir "%STAGE%\runtime\transcribe-0.4.2-cpu-vulkan" "%STAGE%\models" || exit /b 1
copy /y "target\release\listen-rs.exe" "%STAGE%\Listen.exe" >nul || exit /b 1
xcopy /e /i /q /y "%RUNTIME%" "%STAGE%\runtime\transcribe-0.4.2-cpu-vulkan" >nul || exit /b 1
copy /y "%MODEL%" "%STAGE%\models\" >nul || exit /b 1
copy /y NOTICE.md "%STAGE%\" >nul
copy /y LICENSE "%STAGE%\" >nul || exit /b 1

if exist "%OUT%" rmdir /s /q "%OUT%"
move /y "%STAGE%" "%OUT%" >nul || exit /b 1

echo Packaged into %CD%\%OUT%
dir /s "%OUT%" | findstr /r /c:"File(s)"
exit /b 0
