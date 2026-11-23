@echo off
rem Builds the C and C++ examples with MSVC (cl.exe from Visual Studio / Build Tools 2019 or later).
rem Output: build\examples\hello_c.dll, build\examples\hello_cpp.dll
rem Usage: tools\build-examples.cmd        (from any folder; finds Visual Studio through vswhere)
setlocal
set "ROOT=%~dp0.."
set "OUT=%ROOT%\build\examples"
if not exist "%OUT%" mkdir "%OUT%"

where cl >nul 2>nul
if not errorlevel 1 goto :build
set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
if not exist "%VSWHERE%" goto :novs
for /f "usebackq delims=" %%i in (`"%VSWHERE%" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do set "VSDIR=%%i"
if not defined VSDIR goto :novs
call "%VSDIR%\VC\Auxiliary\Build\vcvars64.bat" >nul 2>&1

:build
set FLAGS=/nologo /W4 /O2 /MT /LD /I "%ROOT%\sdk\include"
pushd "%OUT%"
cl %FLAGS% /TC "%ROOT%\examples\hello-c\hello.c" "%ROOT%\sdk\c\scripthook_srt2.c" /Fe:hello_c.dll /link /NOLOGO || goto :fail
cl %FLAGS% /std:c++17 /EHsc "%ROOT%\examples\hello-cpp\hello.cpp" /Tc "%ROOT%\sdk\c\scripthook_srt2.c" /Fe:hello_cpp.dll /link /NOLOGO || goto :fail
del /q *.obj *.exp *.lib 2>nul
popd
echo build-examples: %OUT%\hello_c.dll, %OUT%\hello_cpp.dll
exit /b 0

:novs
echo build-examples: Visual Studio not found. Install "Desktop development with C++" or run from a Developer Command Prompt.
exit /b 1

:fail
popd
echo build-examples: failed
exit /b 1
