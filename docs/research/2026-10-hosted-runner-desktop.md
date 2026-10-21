# Hosted Windows runners and UI Automation: what works and what does not

Date: 2026-10-21. The probe lives on the branch `probe/hosted-desktop` and ran six times on GitHub's hosted Windows runners; this note is on `docs/hosted-runner-desktop`. It settles the risk BUILD.md section 7 carried for WP5 and WP9, "Hosted Windows runners may not give UIA an interactive desktop; if not, desktop scenarios run only on the owner's and fleet PCs", and the question WP5 acceptance 9 left to the first nightly. On the owner's PC only `git`, `gh`, a PowerShell parse check of the probe and an `Add-Type` compile of its C# block ran: no Rust, no cargo, nothing executed from the compiled types, and no setting changed.

## The answer

Hosted Windows runners do give UI Automation an interactive desktop. The x64 images (`windows-latest`, `windows-2025`, `windows-2022`) can carry desktop tests on owned fixtures today, under four conditions the design has to respect. The `windows-11-arm` image cannot, as GitHub ships it.

1. **A real interactive desktop.** The job's steps run in session 2, which is the active console session (`console runneradmin 2 Active`) and not a remote session, on `WinSta0\Default`, which is also the input desktop. The window station is visible, Explorer and the taskbar are running, and DWM composition is on. A window the step creates is visible and found by UI Automation, and a screenshot of the screen is not black (99.72 to 99.96 percent of pixels non-black).
2. **UI Automation works from the runner's own process with the COM client.** In the step's own process, inside the runner's Job Object, `IUIAutomation` read the classic Notepad's text box as `Text Editor [Edit] 50004`, set and read back its value through ValuePattern, and invoked a WinForms button through InvokePattern, and the click landed. Controls that bring their own provider (Edge's web content and address bar, the Windows 11 Notepad) work as well.
3. **No test process may load the .NET UI Automation client.** Once a process has made one call through `System.Windows.Automation`, both that client and the COM client in the same process see Win32 and WinForms controls as bare windows: the Notepad text box becomes a Pane (`50033`) named only by its window text, with no ValuePattern and no LegacyIAccessible pattern, and the button becomes a Pane with no InvokePattern. Controls with their own provider are unaffected. This, and not the runner's Job Object, is what hid the controls in the first three runs: run 6 shows the step's own process, in the runner's job, reading them before its first .NET call and losing them after it. Why the .NET client does this is not established (see "The .NET client, and a corrected reading").
4. **The session is elevated.** The runner, Explorer and every app they started ran at high integrity, while Eileen and the apps it drives run at medium integrity on a user's PC. Why is unverified. The probe read `EnableLUA=1 ConsentPromptBehaviorAdmin=0` with `FilterAdministratorToken` unset. Microsoft gives `ConsentPromptBehaviorAdmin` 0 as "Elevate without prompting" for an operation that asks for elevation, which does not by itself run Explorer elevated, and gives `FilterAdministratorToken` 0, the default, as the built-in Administrator account running every application with full administrative privilege ([UAC settings and configuration](https://learn.microsoft.com/en-us/windows/security/application-security/application-control/user-account-control/settings-and-configuration)); that would explain it if `runneradmin` is the built-in Administrator account, which the probe did not record. Because Explorer is elevated, an app started through the shell is too: the Notepad launched by AUMID through `explorer.exe shell:AppsFolder\...` ran at high integrity, so on this image anything `user_launch` starts through the shell runs elevated, and whether a medium caller can use the elevated shell at all was not measured (unverified). A client made by lowering the step's token to medium integrity set and read back a value in, and typed into, a Notepad also started at medium; no medium-to-medium Invoke was tried, because the only Invoke target, the probe window, was elevated. Against an elevated Notepad and the elevated probe window, Windows reported success and did nothing: SetValue returned `S_OK` and the value did not change, Invoke on the probe window's button returned `S_OK` and the click did not land, and SendInput reported every event inserted while no text arrived. Microsoft documents the last of these for SendInput under UIPI ("neither GetLastError nor the return value will indicate the failure was caused by UIPI blocking", [SendInput](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput)); the probe shows the same silent success for UI Automation's SetValue and Invoke. A medium client walking the elevated classic Notepad's tree saw only its title bar; it reached the Edit only by window handle, so a client that finds elements by walking the tree gets "not found" there, not the silent success.

Job Objects make no difference. The runner's own job (limit flags `0x0`), a new kill-on-close job of the shape `eileen_supervisor` gives its sidecars (`0x2000`), Task Scheduler's job and no job at all gave the same result in every check. The runner's job does not allow breakaway, so `CREATE_BREAKAWAY_FROM_JOB` fails with error 5, but Notepad launched through the shell (`shell:AppsFolder`) ran in no job, and, like Explorer, at high integrity.

What a hosted runner cannot show: any audio device (no render and no capture endpoint, in any state, on any image), more than one monitor or any scale but 100 percent (one 1024x768 Hyper-V display at 96 DPI), physical input (only injected input, which Raw Input reports with no device handle), the Windows 11 Notepad on the x64 images (they ship the classic one), a medium-integrity session that Windows itself created, and a shell at medium integrity (apps started through Explorer run elevated). On `windows-11-arm`, a full-screen "Microsoft account" window holds the foreground and every way of taking it failed.

The recommended CI design is in the section "Recommended CI design for desktop scenarios", and the changes to BUILD.md follow it.

## Setup

| Item | Value |
|---|---|
| Runners | GitHub-hosted, runner 2.338.0. One workflow, a matrix of `windows-latest`, `windows-2025`, `windows-2022` and `windows-11-arm`, so four jobs per run; all 24 jobs succeeded |
| Images | `windows-latest` and `windows-2025`: `win25-vs2026` 20261007.264.1 or 20261018.275.1, both seen (Windows Server 2025 Datacenter, build 26100, x64). `windows-2022`: `win22` 20261012.333.1 (Windows Server 2022 Datacenter, build 20348, x64). `windows-11-arm`: `win11-vs2026-arm64` 20261013.183.4 or 20261018.189.1 (Windows 11 Enterprise, build 26200, ARM64). Results did not differ between the two versions of an image |
| Session | The runner's user `runneradmin`, session 2, elevated, Windows PowerShell 5.1 (5.1.26100.33438 or .33648 on Server 2025, 5.1.20348.5622 on Server 2022, 5.1.26100.9457 or .9711 on ARM) |
| Probe | `tools/probe/hosted-desktop.ps1`, PowerShell only: user32, kernel32, advapi32, oleacc and shcore through P/Invoke; the COM client `IUIAutomation` declared by hand (the client Eileen's Rust code uses through the `windows` crate); the .NET client `System.Windows.Automation`; `IMMDeviceEnumerator`; WinForms for the probe window and screen capture. The final script and the workflow are at the end of this note |
| Fixtures | A WinForms window with one button whose title counts clicks; Notepad launched by AUMID from `shell:AppsFolder`; on the x64 images, a second Notepad started by the step and a third started with the step's token lowered to medium integrity; an isolated Edge profile on a local page loaded as `file:///<temp>/page.html?q=1#frag` with one labelled input |
| Output | Each result is a `RESULT key = value` line in the step's log and an entry in `report.json`. Every line quoted here is copied from `gh run view <id> --log` |

The runs, all on 2026-10-21, in this repository's Actions ([run 1](https://github.com/Sixteen1-6/eileen/actions/runs/39624024208), [2](https://github.com/Sixteen1-6/eileen/actions/runs/39625568326), [3](https://github.com/Sixteen1-6/eileen/actions/runs/39626751028), [4](https://github.com/Sixteen1-6/eileen/actions/runs/39632073196), [5](https://github.com/Sixteen1-6/eileen/actions/runs/39633283235), [6](https://github.com/Sixteen1-6/eileen/actions/runs/39634202692)):

| Run | Id | Probe commit | What it added |
|---|---|---|---|
| 1 | 39624024208 | `178cb9b` | Session, desktop, display, the probe window, Notepad by AUMID through the .NET client, SendInput, screenshots, audio, Edge |
| 2 | 39625568326 | `e50eced` | The COM client, foreground strategies, integrity levels, a child under `runas /trustlevel:0x20000` (it ran at high integrity, so it added nothing) |
| 3 | 39626751028 | `a387f4c` | MSAA, job membership, a child at medium integrity (it never reported) and a child started by Explorer |
| 4 | 39632073196 | `a72f9a0` | Children by breakaway, in a new kill-on-close job, with the runner's environment and from Task Scheduler. The step's log stopped after its first child exited, although the step ran two more minutes and succeeded, so nothing from this run is used |
| 5 | 39633283235 | `f7aeb18` | Run 4 again, with the report printed a second time by a separate step as a guard. The log was complete. The medium-integrity children failed at `Add-Type` |
| 6 | 39634202692 | `e94c535` | Every client makes its COM calls, then one .NET call, then the same COM calls again; the step does the same before its own first .NET call; the children load a prebuilt DLL, so the medium ones run |

## Results

### The desktop and the session

The same on all four images in every run. From run 6 on `windows-latest`:

```text
RESULT environment.elevated = true
RESULT environment.integrity = "high"
RESULT environment.job = "in a job"
RESULT environment.jobUiRestrictions = "0x0"
RESULT environment.uac = "EnableLUA=1 ConsentPromptBehaviorAdmin=0 FilterAdministratorToken="
RESULT session.processSessionId = 2
RESULT session.activeConsoleSessionId = 2
RESULT session.userInteractive = true
RESULT session.remoteSession = false
RESULT session.windowStation = "WinSta0"
RESULT session.windowStationVisible = true
RESULT session.threadDesktop = "Default"
RESULT session.inputDesktop = "Default"
RESULT session.runnerProcesses = ["Runner.Listener session 2","Runner.Worker session 2"]
RESULT session.explorerRunning = true
RESULT session.taskbarFound = true
RESULT session.screenSaverActive = 0
RESULT outside.ownJobLimits = "0x0 breakawayOk=False silentBreakawayOk=False killOnClose=False"
```

`query session` lists `services 0 Disc`, `>console runneradmin 2 Active` and two listeners. Nothing had touched the input since the image started: the idle time at the start of the step was 83 to 988 seconds. The job's UI restrictions are none (`0x0`), and so are its limit flags.

### Display

```text
RESULT display.awarenessAfter = 2
RESULT display.setPerMonitorV2 = "ok"
RESULT display.systemDpi = 96
RESULT display.primaryMonitorDpi = "hr=0x00000000 x=96 y=96"
RESULT display.monitorCount = 1
RESULT display.screens = "\\\\.\\DISPLAY1 1024x768 at 0,0 primary=True"
RESULT display.dwmComposition = "hr=0x00000000 enabled=True"
RESULT display.videoControllers = "Microsoft Hyper-V Video 1024x768 1Hz"
```

The same on every image. Per-monitor-v2 awareness is accepted, but with one monitor at 100 percent the DPI mapping can only be tested as the identity.

### UI Automation: the client matters, the job does not

The step's own process, in the runner's job and at high integrity, against a Notepad it started and a second WinForms probe window, first with the COM client alone, then after one `AutomationElement.FromHandle` call, then with the same COM calls again (run 6, `windows-latest`; `windows-2025` and `windows-2022` printed the same values):

```text
RESULT comfirst.notepad = "notepad, high, in a job"
RESULT comfirst.com.children = ["Text Editor [Edit] 50004","Status Bar [msctls_statusbar32] 50017"," [] 50037","Application [] 50010"]
RESULT comfirst.com.legacyPattern = "hr=0x00000000 present=True"
RESULT comfirst.com.setAndRead = ["set hr=0x00000000","get hr=0x00000000","set over COM, com"]
RESULT comfirst.com.button = "Probe button [WindowsForms10.BUTTON.app.0.2e3ecc8_r8_ad1] 50000"
RESULT comfirst.com.invoke = "invoke hr=0x00000000, clicks=1"
RESULT comfirst.managedEdit = "set over COM, com [Edit] ControlType.Pane: "
RESULT comfirst.managedButton = "Probe button [WindowsForms10.BUTTON.app.0.2e3ecc8_r8_ad1] ControlType.Pane: "
RESULT comfirst.comAfterManaged.children = ["set over COM, com [Edit] 50033"," [msctls_statusbar32] 50033"]
RESULT comfirst.comAfterManaged.legacyPattern = "hr=0x00000000 present=False"
RESULT comfirst.comAfterManaged.setAndRead = ["GetCurrentPattern hr=0x00000000","",""]
RESULT comfirst.comAfterManaged.button = "Probe button [WindowsForms10.BUTTON.app.0.2e3ecc8_r8_ad1] 50033"
RESULT comfirst.comAfterManaged.invoke = "no InvokePattern (hr=0x00000000), clicks=1"
```

Each element prints as `name [class] control type`. The control type ids are UI Automation's ([control type identifiers](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-controltype-ids)): 50000 Button, 50004 Edit, 50010 MenuBar, 50017 StatusBar, 50030 Document, 50033 Pane, 50037 TitleBar. After the .NET call, each name is only the window's text, which is what Microsoft says UI Automation can still supply for a control with no proxy: "only basic information available from the window handle" ([UI Automation providers overview](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-providersoverview)).

Then every other way of starting a client, each against three Notepads and the main probe window's button. On the x64 images, t1 is the Notepad launched by AUMID (high integrity, no job), t2 one the step started (high, in the runner's job) and t3 one the step started with a medium token (medium, in the runner's job). Run 6, `windows-latest`; the other two x64 images printed the same values:

| Client, and how it was started | Its job | Integrity | COM before any .NET call, high targets (t1, t2) | COM before, medium target (t3) | Button, COM Invoke before | After one .NET call |
|---|---|---|---|---|---|---|
| The step itself (section above) | runner's, `0x0` | high | Edit 50004, value set and read back | not tried | Button 50000, click landed | Panes, no patterns, no Invoke |
| A `.cmd` started through `explorer.exe` | none | high | Edit 50004, value set and read back | Edit 50004, set and read back | landed | the same loss, on every target |
| Started by that Explorer child in a new kill-on-close job | new, `0x2000` | high | Edit 50004, set and read back | set and read back | landed | the same loss |
| Task Scheduler, interactive logon, highest run level | Task Scheduler's, `0x0` | high | Edit 50004, set and read back | set and read back | landed | the same loss |
| The step's token lowered to medium | runner's, `0x0` | medium | title bar only; the Edit, reached by handle, reads its value; SetValue `S_OK`, value unchanged; typing lost | Edit 50004, set and read back; typing landed | `S_OK`, not landed | the same loss |
| The Explorer child's token lowered, in a new kill-on-close job | new, `0x2000` | medium | as the row above | as above | `S_OK`, not landed | the same loss |
| The Explorer child's token lowered | none | medium | as above | as above | `S_OK`, not landed | the same loss |
| The step with `CREATE_BREAKAWAY_FROM_JOB` | | | `CreateProcess error 5`: the runner's job allows no breakaway | | | |

The medium client inside a new kill-on-close job is the shape Eileen's desktop sidecar has on a user's PC. Its lines:

```text
RESULT outside.freshjobmedium.integrity = "medium"
RESULT outside.freshjobmedium.jobLimits = "0x2000 breakawayOk=False silentBreakawayOk=False killOnClose=True"
RESULT outside.freshjobmedium.t1.com.children = " [] 50037"
RESULT outside.freshjobmedium.t1.com.setAndRead = ["set hr=0x00000000","get hr=0x00000000"," typed by freshjob 1set by freshjob com 1"]
RESULT outside.freshjobmedium.t3.com.children = ["Text Editor [Edit] 50004","Status Bar [msctls_statusbar32] 50017"," [] 50037","Application [] 50010"]
RESULT outside.freshjobmedium.t3.com.setAndRead = ["set hr=0x00000000","get hr=0x00000000","set by freshjobmedium com 3"]
RESULT outside.freshjobmedium.form.com.button = "Probe button [WindowsForms10.BUTTON.app.0.2e3ecc8_r8_ad1] 50000"
RESULT outside.freshjobmedium.form.com.invoke = "invoke hr=0x00000000, landed=False"
RESULT outside.freshjobmedium.t1.sendInput = "inserted=52 refused=0 error=0 landed=False"
RESULT outside.freshjobmedium.t3.sendInput = "inserted=52 refused=0 error=0 landed=True"
```

For t1, SetValue returned `S_OK` and the value read back was still the previous client's text. The probe reached t1's Edit by window handle (`FindWindowEx`, then `ElementFromHandle`); from medium, t1's tree held only the title bar (`t1.com.children = " [] 50037"`), so a client that walks the tree to find the Edit does not find it. The `form` lines are the only Invoke a medium client made, and their target, the probe window, is elevated: no medium-to-medium Invoke was tried. Run 5 also started a high-integrity client with the runner's whole environment block outside any job: it matched the Explorer child in every check. The runner's environment has 49 variables Explorer's lacks (`ACTIONS_*`, `CI`, `GITHUB_*`, `RUNNER_*`) and Explorer's has one the runner's lacks (`PROMPT`).

On `windows-11-arm` the AUMID launch opens the packaged Windows 11 Notepad, whose text area (`RichEditD2DPT`, Document 50030, with Value and Text patterns) brings its own provider. Every high-integrity client set and read its value before and after the .NET call; the WinForms button behaved as on x64 (Button and Invoke before, Pane after); and to the medium clients the elevated Notepad offered no ValuePattern at all:

```text
RESULT outside.explorer.t1.comAfterManaged.setAndRead = ["set hr=0x00000000","get hr=0x00000000","set by explorer comAfterManaged 1"]
RESULT outside.explorer.form.comAfterManaged.invoke = "no InvokePattern (hr=0x00000000), landed=False"
RESULT outside.medium.t1.com.setAndRead = ["GetCurrentPattern hr=0x00000000","",""]
```

### The .NET client, and a corrected reading

In runs 1 to 3 the step's process used the .NET client from its first UI Automation call and saw Panes, while a child started by Explorer used only the COM client and saw everything, so the runner's Job Object looked like the cause. Run 5 gave every child one .NET call between its COM calls, and every child, inside a job or not, lost the controls from that call on. Run 6 settled it: the step's own process, in the runner's job, read the controls before its first .NET call and lost them after it, and so did every child.

Why the .NET client does this is unverified. One explanation fits: UI Automation lets a client register a process-wide callback that supplies providers for windows, `UiaRegisterProviderCallback` (Microsoft marks it deprecated and points clients to the COM interfaces, [UiaRegisterProviderCallback](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationcoreapi/nf-uiautomationcoreapi-uiaregisterprovidercallback)), and if the .NET Framework client registers its own proxies that way and they do not serve these controls, every client in the process would see bare windows. The probe measured only the before and after. Eileen's Rust code never loads the .NET client, so the rule that matters is the test-side one in the CI design below.

### Input and the foreground

On the x64 images the step's process has the foreground at the start or takes it when its window activates, and every child brought each Notepad to the front with a plain `SetForegroundWindow`. `SendInput` clicked the probe button (`cursorMoved=1 down=1 up=1 error=0`, and the click landed) and typed `typed by SendInput 123` into Notepad as 44 Unicode events (`inserted=44 refused=0`) that `WM_GETTEXT` read back exactly. `ElementFromPoint` and `WindowFromPoint` agreed on the button, and TransformPattern moved Notepad to (480, 60). Raw Input saw every injected event with no device handle, on every image:

```text
RESULT rawinput.afterClick = "mouse +2, without device +2, with device +0"
RESULT rawinput.afterShiftTap = "sendInput=2/2 keyboard +2, without device +2, with device +0"
```

On `windows-11-arm` a full-screen "Microsoft account" window (class `Shell_OOBEProxy`, owned by Explorer, its content in WWAHost) holds the foreground. In runs 1 to 5 every strategy failed (`SetForegroundWindow`, again after an Alt tap, again with `AttachThreadInput`), and in run 1 the typing went to that window: `notepad.sendInputReadBack = ""`. Runs 5 and 6 tried to clear it:

```text
RESULT oobe.owner = "explorer"
RESULT oobe.foregroundBefore = "WWAHost"
RESULT oobe.visibleAfterClose = false
RESULT oobe.foregroundAfterClose = "WWAHost"
RESULT oobe.foregroundAfterEndingWwaHost = "SearchHost"
```

`WM_CLOSE` hid the window but left WWAHost in front; ending WWAHost passed the foreground to SearchHost. The probe window was still refused, Notepad then opened in front, and the step's typing read back garbled (`SSSSSSSSSSendInput ` for `typed by SendInput 123`), while the children's typing into the same Notepad landed. The cause of the garbling is not known.

### Notepad on each image

The x64 images ship the classic Win32 Notepad (its `shell:AppsFolder` entry is `{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\notepad.exe`; an `Edit` control and a status bar). All of its UI Automation support comes from UI Automation's own proxies, so only a client that has not loaded the .NET client sees it. It opened in 0.29 to 0.82 seconds, and closing it through WindowPattern with typed text in it brought the unsaved-changes dialog in runs 2 to 6 (`close.unsavedDialogFound = true`). In those runs the step's own SetValue did not take (the .NET client had already hidden the Edit's ValuePattern), so its text arrived only by typing; in run 1 no text reached it and it closed without a dialog. A close after a change made only through SetValue was never tried on it.

On `windows-11-arm` the AUMID is the packaged Notepad's (`Microsoft.WindowsNotepad_8wekyb3d8bbwe!App`, the text area `RichEditD2DPT` inside `NotepadTextBox`), which runs in a job of its own and opened in 5.6 to 31.6 seconds. In runs 2, 3 and 5 its text was set only through ValuePattern (`notepad.managedValueRoundTrip = true`; nothing was typed, because the account window held the foreground), and it closed without a prompt. In run 6 text was also typed (`notepad.sendInputReadBack = "SSSSSSSSSSendInput "`), and it showed the save prompt (`close.unsavedDialogFound = true`). So what brought the prompt may be how the text arrived rather than chance, and whether a change made only through SetValue counts as unsaved is unverified. It matters to Eileen: text inserted with `desktop_set_value` may leave a document the app considers clean, which a later close would then discard without asking. `notepad.exe` on that image still opens the classic Notepad, with a banner "A new version of Notepad is available"; the step's own COM-first check in run 6 opened that one and looked for its text box before it existed, so on ARM that check has only the window's children and the button, which matched x64. Windows 11 ships the packaged Notepad; which one the owner's PC runs, and how it closes there, was not checked here.

### Edge

On every image, an isolated profile's window appeared 1.0 to 2.1 seconds after launch. The address bar (an Edit named "Address and search bar") read over UI Automation as `D:/a/_temp/probe/page.html?q=1#frag` for a page loaded as `file:///D:/a/_temp/probe/page.html?q=1#frag` (`C:/a/...` on ARM): the query and the fragment are kept, and the `file:///` scheme is not shown. The page's input was found 0.27 to 0.76 seconds after the window, and a ValuePattern round trip on it worked, from the .NET client too, because Chromium brings its own provider.

### Screenshots

`CopyFromScreen` of the 1024x768 screen was never black: 99.83 to 99.96 percent of pixels non-black with 6,911 to 27,463 distinct colours on x64, and 99.72 to 99.86 percent on ARM. `PrintWindow` with flag 2 (`PW_RENDERFULLCONTENT`) on Notepad succeeded every time, with 96 to 100 percent of pixels non-black.

### Audio

```text
RESULT audio.services = ["AudioEndpointBuilder Running Automatic","Audiosrv Running Automatic"]
RESULT audio.registryRenderEndpoints = "count=0 states="
RESULT audio.mmdevice.render = "enumHr=0x00000000 anyState=0 active=0 defaultHr=0xffffffff80070490"
RESULT audio.mmdevice.capture = "enumHr=0x00000000 anyState=0 active=0 defaultHr=0xffffffff80070490"
```

The audio services run on Server 2025 and on ARM and are stopped (start type Manual) on Server 2022. No image has a render or capture endpoint in any state, and asking for the default endpoint fails with `0x80070490`, `ERROR_NOT_FOUND`. BUILD.md section 4.7 already gives hosted runs the WAV source and capture sink for this reason.

### Timings

These describe GitHub's machines, not the owner's PC, and are here only so a later run can be compared.

| Measure | x64 images | `windows-11-arm` |
|---|---|---|
| The first .NET UI Automation call (a top-level window found by name) | 0.49 to 1.20 s | 0.75 to 5.13 s |
| Notepad's window after launch | 0.29 to 0.82 s | 5.6 to 31.6 s |
| Edge's window after launch | 1.02 to 1.72 s | 1.01 to 2.14 s |
| Edge's web content after the window | 0.27 to 0.32 s | 0.31 to 0.76 s |

In run 6 the .NET call took 7 to 14 milliseconds because the COM section had already started UI Automation in the process, so it is left out of the first row.

### What the probe could not do

- **A full medium-integrity session.** The medium clients are the step's elevated token with its integrity label lowered; they keep the Administrators group, so they model integrity checks and UIPI, not a standard user's token in every respect (unverified how far the difference reaches). `Add-Type`, which compiles through `csc.exe`, failed under such a token with "A required privilege is not held by the client" (`ERROR_PRIVILEGE_NOT_HELD`, run 5); the log shows only Add-Type's `Win32Exception`, so which call failed is unverified. The token did start `cmd.exe`, `powershell.exe` and Notepad. Run 6 compiled the probe's types once in a separate high-integrity process and had the children load the DLL.
- **Anything else under the lowered token.** Only Notepad, `cmd.exe` and `powershell.exe` ran under it. No testpad, no Edge and no tree of processes like `eileen run` with its sidecars were started that way, and no medium-to-medium Invoke was tried. Whatever stopped `Add-Type` could also stop Edge's multi-process sandbox (unverified).
- **A medium caller using the elevated shell.** The AUMID launch ran only from the high-integrity step. No medium process launched anything through Explorer or `IApplicationActivationManager`, so whether a medium sidecar's `user_launch` reaches the elevated shell at all, and at which integrity the app then runs, is unverified.
- **The runner account's identity.** The probe did not record the RID of `runneradmin`, so whether it is the built-in Administrator account (which would explain the elevated shell) is unverified.
- **`runas /trustlevel`.** In run 2 the child it started ran at high integrity, so it is not a way to get a medium client here.
- **Run 4's log.** After the first child exited, nothing more the step printed reached the log, although the step ran two more minutes and succeeded. Runs 5 and 6 printed the report a second time from a separate step as a guard and did not need it. The cause is not known.

## Recommended CI design for desktop scenarios

1. **Runner.** The hosted desktop lane runs on `windows-2025` by name (today's `windows-latest`), and every desktop result records `ImageOS` and `ImageVersion`, so a label move or an image update reads as a change in the record and not as a mystery. Not `windows-11-arm`, whose foreground the account window holds, and nothing is gained from `windows-2022`.
2. **The process shape.** Test binaries run as nextest starts them, inside the runner's job; no Explorer, Task Scheduler or breakaway detour is needed. Eileen's code uses `IUIAutomation` through the `windows` crate, which is the client that works. The one rule the lane needs: no test process, helper or diagnostic script loads `System.Windows.Automation` (PowerShell's UI Automation included) and then asserts anything about a Win32 or WinForms control. A PowerShell helper that has to inspect the desktop uses the COM client, as the probe does, or runs in a process of its own.
3. **Integrity.** On the hosted lane, a test helper lowers the runner's token (the probe's `Launch` with `medium`) to start the desktop sidecar at medium integrity, and starts every app a scenario drives the same way, handing it to the scenario by window handle, so the lane tests the shape users have, a medium client driving medium apps, and not an all-elevated desktop. In end-to-end scenarios the core starts its sidecars through `eileen_supervisor`, the only allowed spawner, so there the helper starts `eileen run`, the whole process tree, at medium; that shape was not measured, and the canary checks it. Apps `user_launch` starts through the shell run elevated on this image, because Explorer is elevated, and whether a medium caller can use that shell was not measured; so a scenario that launches an app through the sidecar and then drives it declares a `requires` capability (for example `medium-shell`) and runs on the owner's and fleet PCs. Two elevated fixtures stay for the refusal path, because SetValue, Invoke and typing into a window of higher integrity report success and do nothing: the classic Notepad, whose Edit the sidecar reaches by window handle, for SetValue and typing, and the testpad's button for Invoke. Each must come back as refused by read-back, never as done. Nothing on the lane compiles code at run time under a lowered token.
4. **A canary.** The nightly's first desktop job repeats this probe's core and measures what it left out, recording `ImageOS`, `ImageVersion`, the UAC values and the RID of the runner's account: from the test process, a COM round trip in the classic Notepad and an Invoke on the testpad; through the helper at medium, a round trip in a classic Notepad started at medium, the testpad and the isolated Edge profile started at medium with a medium-to-medium Invoke on the testpad and a ValuePattern round trip in Edge's page, and `eileen run` started at medium with its sidecars at medium; a medium sidecar's `user_launch` of the classic Notepad, recording the launched app's integrity, its job and any error; and the elevated refusal on both elevated fixtures. If any of them changes, the job fails with the image version and desktop tests fall back to the owner's and fleet lanes until it is understood. Until the Edge cases pass, Edge stays off the hosted lane.
5. **What can run hosted** (owned fixtures, one 1024x768 monitor at 96 DPI, no audio device, injected input only):
   - UI Automation tree reads, Invoke and SetValue with read-back and the verification hints, on the testpad and the classic Notepad started at medium;
   - on an isolated Edge profile, once the canary shows it running at medium: the same reads and actions, the web content tree for the `Dom` rung and the address bar's URL with its path, query and fragment;
   - `SendInput` typing and clicks under the foreground rule, the `ElementFromPoint` and `WindowFromPoint` checks, moving and snapping a window on one work area, and closing with the unsaved-state check (the classic Notepad's dialog after typed text, and a close after a change made only through SetValue, which the check must detect itself);
   - `BitBlt` and `PrintWindow` capture with a not-all-black assertion, and per-monitor-v2 at 96 DPI;
   - Raw Input's synthetic flag for injected input, and the `test-physical` injector's cases;
   - launching by AUMID from `shell:AppsFolder`, and "apps Eileen launches leave its jobs" through the shell's route, limited to job membership, environment and survival, with the launched app's integrity recorded (high on this image); the lane does not drive an app the sidecar launched;
   - the hung-provider fixture, the tier decisions on fixture windows, the medium-integrity sidecar and the elevated refusal;
   - the soak, which BUILD.md already runs on hosted Windows.
6. **What stays on the owner's PC and fleet PCs:**
   - anything that needs an audio device: capture, playback, echo cancellation, dictation by voice end to end;
   - two or more monitors, any scale but 100 percent, mixed DPI;
   - physical input (Raw Input with a real device handle) and the owner's by-hand repeat of WP5 acceptance 1;
   - the Windows 11 client: its shell, its packaged apps, the Windows 11 Notepad's text area and how it closes, and every real app (Resolve, Chrome with the user's profile, Premiere);
   - a medium-integrity session Windows created itself, rather than the hosted lane's lowered token;
   - launching an app through the sidecar and then driving it (`requires` `medium-shell`), since the hosted shell starts apps elevated;
   - the Edge cases, until the canary shows Edge running under the lowered token;
   - the `CREATE_BREAKAWAY_FROM_JOB` fallback of `user_launch`, which the runner's job refuses with error 5;
   - every performance number (`desktop_state` p95, the `desktop_tree` timings, the Chromium probe), since a hosted machine is not the owner's;
   - real Claude (`real_claude::`) and anything signed in.

## Changes to BUILD.md

- Section 7: the risk moves to the settled questions under "Carried into packages", with this answer and a link here. The open risk that replaces it is narrower: the hosted desktop lane rests on GitHub's image staying as measured, and the canary notices when it does not. The Raw Input risk notes that the injected half of its signal holds on hosted runners.
- Section 4.7: the End to end row runs the desktop scenarios on `windows-2025` by name (WP9 acceptance 10), with smoke runs as before; the owned-fixture sentence names the classic Notepad started at medium for the hosted lane, and its "first nightly proves" clause becomes the canary of WP5 acceptance 9.
- Section 6.7, WP5: acceptance 1 starts Notepad at medium on the hosted lane and adds a close after a SetValue-only change; acceptance 7's scenarios that open an app and then drive it declare `medium-shell`; acceptance 9 becomes the nightly canary, names the medium-integrity test helper and the rule that no test process loads the .NET client, and says which Notepad each item runs on; acceptance 12 is limited on the hosted lane to job membership, environment and survival, and tests the breakaway fallback on the owner's PC, because the runner's job refuses it.
- Section 6.11, WP9: a new acceptance 10 for the hosted desktop lane: which scenarios it runs, on which image, at which integrity (the helper starts `eileen run` at medium), with the image version in every result.
- Section 8: the revision log entry.

Section 4.6 already puts end-to-end smoke scenarios and the soak on hosted Windows, so it is unchanged.

## The probe

The workflow file and the script below are the probe as of run 6 (commit `e94c535` on `probe/hosted-desktop`). To run it again, put the script at `tools/probe/hosted-desktop.ps1` and the workflow at `.github/workflows/probe-hosted-desktop.yml` on a branch named `probe/hosted-desktop` (the workflow runs on a push to that branch, or by `workflow_dispatch`). It needs nothing installed beyond the hosted image and changes nothing outside the runner, which GitHub discards after the job. Its window titles, files and scheduled task are named after Eileen, and it records no account or machine names beyond what the image reports. Its comments are kept as they were when it ran; where they say the lowered token cannot start `csc.exe`, read that as `Add-Type` failing with `ERROR_PRIVILEGE_NOT_HELD`, the failing call unverified (see "What the probe could not do").

### `.github/workflows/probe-hosted-desktop.yml`

```yaml
# Throwaway probe for BUILD.md section 7's risk "Hosted Windows runners may not give UIA an
# interactive desktop". It lives only on the probe/hosted-desktop branch, which is deleted once
# docs/research/2026-10-hosted-runner-desktop.md records the answer. PowerShell only, no Rust.
name: Probe hosted desktop

on:
  workflow_dispatch:
  push:
    branches: [probe/hosted-desktop]

permissions:
  contents: read

jobs:
  probe:
    name: probe (${{ matrix.os }})
    runs-on: ${{ matrix.os }}
    timeout-minutes: 30
    strategy:
      fail-fast: false
      matrix:
        os: [windows-latest, windows-2025, windows-2022, windows-11-arm]
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
        with:
          persist-credentials: false
      - name: Probe the desktop with Windows PowerShell 5.1
        shell: powershell
        timeout-minutes: 22
        run: powershell.exe -NoProfile -ExecutionPolicy Bypass -File tools\probe\hosted-desktop.ps1 -OutDir "$env:RUNNER_TEMP\probe"
      - name: Print the report and the children's own output
        # From a fresh process: in run 4 the probe step's output stopped after its first child exited.
        if: always()
        shell: powershell
        run: |
          $f = Join-Path $env:RUNNER_TEMP 'probe\report.json'
          if (Test-Path $f) {
            Write-Host ("report.json is {0} bytes" -f (Get-Item $f).Length)
            $r = Get-Content $f -Raw | ConvertFrom-Json
            foreach ($p in $r.PSObject.Properties) {
              $v = $p.Value | ConvertTo-Json -Compress -Depth 4
              if ($null -eq $v) { $v = 'null' }
              if ($v.Length -gt 1500) { $v = $v.Substring(0, 1500) + (' ... ({0} characters)' -f $v.Length) }
              Write-Host ("REPORT {0} = {1}" -f $p.Name, $v)
            }
          }
          $dir = Join-Path $env:LOCALAPPDATA 'Temp\eileen-probe-child'
          Get-ChildItem $dir -Filter 'child-*.*' -ErrorAction SilentlyContinue | Where-Object { $_.Extension -in '.log', '.json' } | ForEach-Object {
            $t = [IO.File]::ReadAllText($_.FullName) -replace '[^\x20-\x7e\r\n]', '?'
            $t = ($t.Substring(0, [Math]::Min(4000, $t.Length)) -replace '\r?\n', ' | ')
            Write-Host ("FILE {0} ({1} bytes): {2}" -f $_.Name, $_.Length, $t)
          }
      - name: Keep the report and the screenshots
        if: always()
        uses: actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9 # v7.0.2
        with:
          name: probe-${{ matrix.os }}
          path: ${{ runner.temp }}/probe/*.*
          if-no-files-found: warn
          retention-days: 5
```

### `tools/probe/hosted-desktop.ps1`

```powershell
# Probe: does a GitHub-hosted Windows runner give UI Automation an interactive desktop?
# Windows PowerShell 5.1 only, no Rust. Every section records what it saw and never stops the
# others; the script always exits 0 because its job is to report. Lines that start with "RESULT"
# are the answers; report.json holds the same values.
# -Child runs only the client checks against the windows in -Targets (and the probe window's
# button in -Form) and writes its results to -ChildOut. The main run starts one child per way a
# client can be started (section 'outside'); a child given -Spawn starts further children itself.
param(
    [string]$OutDir = (Join-Path $env:TEMP 'eileen-probe'),
    [switch]$Child,
    [string]$Targets = '',
    [long]$Form = 0,
    [string]$Spawn = '',
    [string]$RunnerEnv = '',
    [string]$Dll = '',
    [string]$ChildOut = '',
    [string]$Label = 'child'
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
# Script-scoped and long-named on purpose: PowerShell scopes are dynamic and names are
# case-insensitive, so a local like $r inside a section would otherwise shadow them.
$script:ProbeResults = [ordered]@{}
$ProbeState = @{}

function Rec([string]$key, $value) {
    $script:ProbeResults[$key] = $value
    $text = if ($null -eq $value) { 'null' } else { $value | ConvertTo-Json -Compress -Depth 4 }
    if ($text.Length -gt 3000) { $text = $text.Substring(0, 3000) + (' ... ({0} characters)' -f $text.Length) }
    Write-Host ("RESULT {0} = {1}" -f $key, $text)
}

function Section([string]$name, [scriptblock]$body) {
    Write-Host "::group::$name"
    $t = [Diagnostics.Stopwatch]::StartNew()
    try { & $body } catch { Rec "$name.error" ($_.Exception.GetType().Name + ': ' + $_.Exception.Message) }
    Rec "$name.seconds" ([math]::Round($t.Elapsed.TotalSeconds, 2))
    $script:ProbeResults | ConvertTo-Json -Depth 6 | Set-Content -Path (Join-Path $OutDir 'report.json') -Encoding UTF8
    Write-Host '::endgroup::'
}

function TryRec([string]$key, [scriptblock]$expr) {
    try { Rec $key (& $expr) } catch { Rec "$key.error" ($_.Exception.GetType().Name + ': ' + $_.Exception.Message) }
}

function Hex([long]$v) { return ('0x{0:x8}' -f ($v -band 0xffffffff)) }

Add-Type -AssemblyName System.Windows.Forms, System.Drawing, UIAutomationClient, UIAutomationTypes, WindowsBase, Accessibility
$accessibilityDll = ([AppDomain]::CurrentDomain.GetAssemblies() | Where-Object { $_.GetName().Name -eq 'Accessibility' } | Select-Object -First 1).Location

$cs = @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using System.Windows.Forms;

namespace EileenProbe {
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X; public int Y; }
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT { public int dx; public int dy; public uint mouseData; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
  [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
  [StructLayout(LayoutKind.Sequential)] public struct HARDWAREINPUT { public uint uMsg; public ushort wParamL; public ushort wParamH; }
  [StructLayout(LayoutKind.Explicit)] public struct InputUnion { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; [FieldOffset(0)] public HARDWAREINPUT hi; }
  [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public InputUnion u; }
  [StructLayout(LayoutKind.Sequential)] public struct USEROBJECTFLAGS { public int fInherit; public int fReserved; public uint dwFlags; }
  [StructLayout(LayoutKind.Sequential)] public struct LASTINPUTINFO { public uint cbSize; public uint dwTime; }
  [StructLayout(LayoutKind.Sequential)] public struct RAWINPUTDEVICE { public ushort usUsagePage; public ushort usUsage; public uint dwFlags; public IntPtr hwndTarget; }
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] public struct STARTUPINFO { public int cb; public string lpReserved; public string lpDesktop; public string lpTitle; public int dwX; public int dwY; public int dwXSize; public int dwYSize; public int dwXCountChars; public int dwYCountChars; public int dwFillAttribute; public int dwFlags; public short wShowWindow; public short cbReserved2; public IntPtr lpReserved2; public IntPtr hStdInput; public IntPtr hStdOutput; public IntPtr hStdError; }
  [StructLayout(LayoutKind.Sequential)] public struct PROCESS_INFORMATION { public IntPtr hProcess; public IntPtr hThread; public int dwProcessId; public int dwThreadId; }
  [StructLayout(LayoutKind.Sequential)] public struct SID_AND_ATTRIBUTES { public IntPtr Sid; public uint Attributes; }
  [StructLayout(LayoutKind.Sequential)] public struct TOKEN_MANDATORY_LABEL { public SID_AND_ATTRIBUTES Label; }
  [StructLayout(LayoutKind.Sequential)] public struct RAWINPUTHEADER { public uint dwType; public uint dwSize; public IntPtr hDevice; public IntPtr wParam; }

  public static class Native {
    [DllImport("kernel32.dll")] public static extern uint WTSGetActiveConsoleSessionId();
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
    [DllImport("user32.dll", SetLastError = true)] public static extern IntPtr GetProcessWindowStation();
    [DllImport("user32.dll", SetLastError = true)] public static extern IntPtr GetThreadDesktop(uint threadId);
    [DllImport("user32.dll", SetLastError = true)] public static extern IntPtr OpenInputDesktop(uint flags, bool inherit, uint access);
    [DllImport("user32.dll", SetLastError = true)] public static extern bool CloseDesktop(IntPtr desktop);
    [DllImport("user32.dll", SetLastError = true, EntryPoint = "GetUserObjectInformationW")] public static extern bool GetUserObjectName(IntPtr obj, int index, byte[] info, uint length, out uint needed);
    [DllImport("user32.dll", SetLastError = true, EntryPoint = "GetUserObjectInformationW")] public static extern bool GetUserObjectFlags(IntPtr obj, int index, ref USEROBJECTFLAGS info, uint length, out uint needed);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindowW(string className, string title);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr parent, IntPtr after, string className, string title);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr SendMessageTimeoutW(IntPtr hwnd, uint msg, IntPtr wParam, StringBuilder lParam, uint flags, uint timeout, out IntPtr result);
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int index);
    [DllImport("user32.dll")] public static extern uint GetDpiForSystem();
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr hwnd);
    [DllImport("user32.dll", SetLastError = true)] public static extern bool SetProcessDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll")] public static extern IntPtr GetThreadDpiAwarenessContext();
    [DllImport("user32.dll")] public static extern int GetAwarenessFromDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll", SetLastError = true)] public static extern uint SendInput(uint count, INPUT[] inputs, int size);
    [DllImport("user32.dll")] public static extern bool GetLastInputInfo(ref LASTINPUTINFO info);
    [DllImport("user32.dll")] public static extern bool SystemParametersInfoW(uint action, uint param, ref int value, uint winIni);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out RECT r);
    [DllImport("user32.dll")] public static extern IntPtr MonitorFromPoint(POINT p, uint flags);
    [DllImport("shcore.dll")] public static extern int GetDpiForMonitor(IntPtr monitor, int type, out uint x, out uint y);
    [DllImport("dwmapi.dll")] public static extern int DwmIsCompositionEnabled(out bool enabled);
    [DllImport("user32.dll", SetLastError = true)] public static extern bool RegisterRawInputDevices(RAWINPUTDEVICE[] devices, uint count, uint size);
    [DllImport("user32.dll")] public static extern uint GetRawInputData(IntPtr rawInput, uint command, out RAWINPUTHEADER data, ref uint size, uint headerSize);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hwnd, IntPtr hdc, uint flags);
    [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint attach, uint attachTo, bool doAttach);
    [DllImport("user32.dll", EntryPoint = "GetWindowThreadProcessId")] public static extern uint GetWindowThread(IntPtr hwnd, IntPtr pidOut);
    [DllImport("user32.dll", EntryPoint = "GetWindowLongPtrW")] public static extern IntPtr GetWindowLongPtr(IntPtr hwnd, int index);
    [DllImport("kernel32.dll", SetLastError = true)] public static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("kernel32.dll")] public static extern bool CloseHandle(IntPtr handle);
    [DllImport("advapi32.dll", SetLastError = true)] public static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
    [DllImport("advapi32.dll", SetLastError = true)] public static extern bool GetTokenInformation(IntPtr token, int infoClass, IntPtr info, uint length, out uint returned);
    [DllImport("advapi32.dll")] public static extern IntPtr GetSidSubAuthority(IntPtr sid, uint index);
    [DllImport("advapi32.dll")] public static extern IntPtr GetSidSubAuthorityCount(IntPtr sid);
    [DllImport("kernel32.dll")] public static extern IntPtr GetCurrentProcess();
    [DllImport("kernel32.dll", SetLastError = true)] public static extern bool IsProcessInJob(IntPtr process, IntPtr job, out bool result);
    [DllImport("kernel32.dll", SetLastError = true)] public static extern bool QueryInformationJobObject(IntPtr job, int infoClass, out uint info, uint length, IntPtr returned);
    [DllImport("advapi32.dll", SetLastError = true)] public static extern bool DuplicateTokenEx(IntPtr existing, uint access, IntPtr attributes, int impersonation, int tokenType, out IntPtr newToken);
    [DllImport("advapi32.dll", SetLastError = true, CharSet = CharSet.Unicode)] public static extern bool ConvertStringSidToSidW(string sid, out IntPtr psid);
    [DllImport("advapi32.dll", SetLastError = true)] public static extern bool SetTokenInformation(IntPtr token, int infoClass, IntPtr info, uint length);
    [DllImport("advapi32.dll")] public static extern int GetLengthSid(IntPtr sid);
    [DllImport("advapi32.dll", SetLastError = true, CharSet = CharSet.Unicode)] public static extern bool CreateProcessAsUserW(IntPtr token, string application, StringBuilder commandLine, IntPtr processAttributes, IntPtr threadAttributes, bool inherit, uint flags, IntPtr environment, string directory, ref STARTUPINFO startup, out PROCESS_INFORMATION info);
    [DllImport("oleacc.dll")] public static extern int AccessibleObjectFromWindow(IntPtr hwnd, uint objectId, ref Guid riid, [MarshalAs(UnmanagedType.Interface)] out object accessible);
    [DllImport("kernel32.dll", EntryPoint = "QueryInformationJobObject", SetLastError = true)] public static extern bool QueryJobRaw(IntPtr job, int infoClass, IntPtr info, uint length, out uint returned);
    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)] public static extern IntPtr CreateJobObjectW(IntPtr attributes, string name);
    [DllImport("kernel32.dll", SetLastError = true)] public static extern bool SetInformationJobObject(IntPtr job, int infoClass, IntPtr info, uint length);
    [DllImport("kernel32.dll", SetLastError = true)] public static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
    [DllImport("kernel32.dll", SetLastError = true)] public static extern uint ResumeThread(IntPtr thread);
    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)] public static extern bool CreateProcessW(string application, StringBuilder commandLine, IntPtr processAttributes, IntPtr threadAttributes, bool inherit, uint flags, IntPtr environment, string directory, ref STARTUPINFO startup, out PROCESS_INFORMATION info);
    [DllImport("user32.dll", EntryPoint = "SendMessageTimeoutW")] public static extern IntPtr SendMessageTimeoutPtr(IntPtr hwnd, uint msg, IntPtr wParam, IntPtr lParam, uint flags, uint timeout, out IntPtr result);

    // JOBOBJECT_EXTENDED_LIMIT_INFORMATION is 144 bytes on 64-bit Windows; LimitFlags is at offset 16.
    static int ExtendedLimitSize() { return IntPtr.Size == 8 ? 144 : 112; }
    // The limit flags of the calling process's innermost job, or why they could not be read.
    public static string OwnJobLimitFlags() {
      int size = ExtendedLimitSize(); IntPtr buf = Marshal.AllocHGlobal(size);
      try {
        uint got;
        if (!QueryJobRaw(IntPtr.Zero, 9, buf, (uint)size, out got)) return "none readable (error " + Marshal.GetLastWin32Error() + ")";
        uint f = (uint)Marshal.ReadInt32(buf, 16);
        return "0x" + f.ToString("x") + " breakawayOk=" + ((f & 0x800) != 0) + " silentBreakawayOk=" + ((f & 0x1000) != 0) + " killOnClose=" + ((f & 0x2000) != 0);
      } finally { Marshal.FreeHGlobal(buf); }
    }
    static List<IntPtr> heldJobs = new List<IntPtr>();
    // Starts a command line in one of the ways the probe compares: at medium integrity (this
    // token lowered, as StartAtMedium does), in a new kill-on-close Job Object (the shape
    // eileen_supervisor gives every child), with extra creation flags such as
    // CREATE_BREAKAWAY_FROM_JOB, and with the environment block read from a NAME=VALUE file.
    public static string Launch(string commandLine, bool medium, bool newJob, uint extraFlags, string envFile) {
      IntPtr env = IntPtr.Zero, token = IntPtr.Zero, dup = IntPtr.Zero, job = IntPtr.Zero;
      uint flags = 0x00000010u | extraFlags | (newJob ? 0x00000004u : 0u);
      if (!String.IsNullOrEmpty(envFile)) {
        List<string> vars = new List<string>(System.IO.File.ReadAllLines(envFile, Encoding.UTF8));
        vars.RemoveAll(delegate(string s) { return s.IndexOf('=') <= 0; });
        vars.Sort(StringComparer.OrdinalIgnoreCase);
        env = Marshal.StringToHGlobalUni(String.Join("\0", vars.ToArray()) + "\0");
        flags |= 0x00000400u;
      }
      if (newJob) {
        job = CreateJobObjectW(IntPtr.Zero, null);
        if (job == IntPtr.Zero) return "CreateJobObject error " + Marshal.GetLastWin32Error();
        int size = ExtendedLimitSize(); IntPtr buf = Marshal.AllocHGlobal(size);
        try {
          for (int i = 0; i < size; i++) Marshal.WriteByte(buf, i, 0);
          Marshal.WriteInt32(buf, 16, 0x2000);
          if (!SetInformationJobObject(job, 9, buf, (uint)size)) return "SetInformationJobObject error " + Marshal.GetLastWin32Error();
        } finally { Marshal.FreeHGlobal(buf); }
      }
      STARTUPINFO si = new STARTUPINFO(); si.cb = Marshal.SizeOf(typeof(STARTUPINFO)); si.lpDesktop = "WinSta0\\Default";
      PROCESS_INFORMATION pi; bool ok; int err;
      try {
        if (medium) {
          if (!OpenProcessToken(GetCurrentProcess(), 0x0002 | 0x0008 | 0x0001 | 0x0080, out token)) return "OpenProcessToken error " + Marshal.GetLastWin32Error();
          if (!DuplicateTokenEx(token, 0x02000000, IntPtr.Zero, 2, 1, out dup)) return "DuplicateTokenEx error " + Marshal.GetLastWin32Error();
          IntPtr sid;
          if (!ConvertStringSidToSidW("S-1-16-8192", out sid)) return "ConvertStringSidToSid error " + Marshal.GetLastWin32Error();
          TOKEN_MANDATORY_LABEL label = new TOKEN_MANDATORY_LABEL(); label.Label.Sid = sid; label.Label.Attributes = 0x20;
          int lsize = Marshal.SizeOf(typeof(TOKEN_MANDATORY_LABEL)) + GetLengthSid(sid);
          IntPtr lbuf = Marshal.AllocHGlobal(lsize);
          Marshal.StructureToPtr(label, lbuf, false);
          if (!SetTokenInformation(dup, 25, lbuf, (uint)lsize)) return "SetTokenInformation error " + Marshal.GetLastWin32Error();
          ok = CreateProcessAsUserW(dup, null, new StringBuilder(commandLine), IntPtr.Zero, IntPtr.Zero, false, flags, env, null, ref si, out pi);
        } else {
          ok = CreateProcessW(null, new StringBuilder(commandLine), IntPtr.Zero, IntPtr.Zero, false, flags, env, null, ref si, out pi);
        }
        err = Marshal.GetLastWin32Error();
      } finally {
        if (env != IntPtr.Zero) Marshal.FreeHGlobal(env);
        if (dup != IntPtr.Zero) CloseHandle(dup);
        if (token != IntPtr.Zero) CloseHandle(token);
      }
      if (!ok) return "CreateProcess error " + err;
      string note = "";
      if (newJob) {
        if (!AssignProcessToJobObject(job, pi.hProcess)) note = ", AssignProcessToJobObject error " + Marshal.GetLastWin32Error();
        else heldJobs.Add(job);
        ResumeThread(pi.hThread);
      }
      CloseHandle(pi.hThread); CloseHandle(pi.hProcess);
      return "started pid " + pi.dwProcessId + note;
    }

    public static string ObjectName(IntPtr obj) {
      byte[] buf = new byte[1024]; uint needed;
      if (!GetUserObjectName(obj, 2, buf, (uint)buf.Length, out needed)) return "error " + Marshal.GetLastWin32Error();
      return Encoding.Unicode.GetString(buf, 0, (int)needed).TrimEnd('\0');
    }
    public static long ObjectFlags(IntPtr obj) {
      USEROBJECTFLAGS f = new USEROBJECTFLAGS(); uint needed;
      if (!GetUserObjectFlags(obj, 1, ref f, (uint)Marshal.SizeOf(typeof(USEROBJECTFLAGS)), out needed)) return -Marshal.GetLastWin32Error();
      return f.dwFlags;
    }
    public static string InputDesktopName() {
      IntPtr d = OpenInputDesktop(0, false, 0x0001);
      if (d == IntPtr.Zero) return "none (error " + Marshal.GetLastWin32Error() + ")";
      try { return ObjectName(d); } finally { CloseDesktop(d); }
    }
    public static string ThreadDesktopName() { return ObjectName(GetThreadDesktop(GetCurrentThreadId())); }
    public static string WindowStationName() { return ObjectName(GetProcessWindowStation()); }
    public static long WindowStationFlags() { return ObjectFlags(GetProcessWindowStation()); }

    public static uint IdleMilliseconds() {
      LASTINPUTINFO i = new LASTINPUTINFO(); i.cbSize = (uint)Marshal.SizeOf(typeof(LASTINPUTINFO));
      if (!GetLastInputInfo(ref i)) return uint.MaxValue;
      return (uint)Environment.TickCount - i.dwTime;
    }
    public static int Spi(uint action) { int v = 0; SystemParametersInfoW(action, 0, ref v, 0); return v; }
    public static uint WindowPid(IntPtr hwnd) { uint pid; GetWindowThreadProcessId(hwnd, out pid); return pid; }
    public static int[] Rect(IntPtr hwnd) { RECT r; GetWindowRect(hwnd, out r); return new int[] { r.Left, r.Top, r.Right - r.Left, r.Bottom - r.Top }; }
    public static IntPtr WindowAt(int x, int y) { POINT p; p.X = x; p.Y = y; return WindowFromPoint(p); }
    public static uint[] PrimaryMonitorDpi() {
      POINT p; p.X = 0; p.Y = 0; IntPtr m = MonitorFromPoint(p, 1); uint x, y;
      int hr = GetDpiForMonitor(m, 0, out x, out y);
      return new uint[] { (uint)hr, x, y };
    }
    public static string GetText(IntPtr hwnd) {
      StringBuilder sb = new StringBuilder(8192); IntPtr result;
      IntPtr ok = SendMessageTimeoutW(hwnd, 0x000D, (IntPtr)8192, sb, 0x0002, 2000, out result);
      if (ok == IntPtr.Zero) return "<WM_GETTEXT failed or timed out, error " + Marshal.GetLastWin32Error() + ">";
      return sb.ToString();
    }
    public static string JobOf(uint pid) {
      IntPtr process = OpenProcess(0x1000, false, pid);
      if (process == IntPtr.Zero) return "open error " + Marshal.GetLastWin32Error();
      bool inJob; bool ok = IsProcessInJob(process, IntPtr.Zero, out inJob); CloseHandle(process);
      if (!ok) return "query error " + Marshal.GetLastWin32Error();
      return inJob ? "in a job" : "no job";
    }
    // JOBOBJECT_BASIC_UI_RESTRICTIONS of the calling process's job, or why it could not be read.
    public static string OwnJobUiRestrictions() {
      uint ui;
      if (!QueryInformationJobObject(IntPtr.Zero, 4, out ui, 4, IntPtr.Zero)) return "none readable (error " + Marshal.GetLastWin32Error() + ")";
      return "0x" + ui.ToString("x");
    }
    // MSAA straight from oleacc: { role, name, value } of the window's client object.
    public static string[] Msaa(IntPtr hwnd) {
      Guid iid = new Guid("618736E0-3C3D-11CF-810C-00AA00389B71");
      object o; int hr = AccessibleObjectFromWindow(hwnd, 0xFFFFFFFC, ref iid, out o);
      Accessibility.IAccessible a = o as Accessibility.IAccessible;
      if (hr != 0 || a == null) return new string[] { "hr=0x" + hr.ToString("x8"), "", "" };
      string role, name, value;
      try { role = Convert.ToString(a.get_accRole(0)); } catch (Exception e) { role = "error " + e.Message; }
      try { name = a.get_accName(0); } catch (Exception e) { name = "error " + e.Message; }
      try { value = a.get_accValue(0); } catch (Exception e) { value = "error " + e.Message; }
      return new string[] { "role=" + role, "name=" + name, "value=" + value };
    }
    // Starts a command line with this process's token lowered to medium integrity, the level an
    // ordinary unelevated program runs at, in the same session and on the same desktop.
    public static string StartAtMedium(string commandLine) {
      IntPtr token;
      if (!OpenProcessToken(GetCurrentProcess(), 0x0002 | 0x0008 | 0x0001 | 0x0080, out token)) return "OpenProcessToken error " + Marshal.GetLastWin32Error();
      IntPtr dup;
      if (!DuplicateTokenEx(token, 0x02000000, IntPtr.Zero, 2, 1, out dup)) return "DuplicateTokenEx error " + Marshal.GetLastWin32Error();
      IntPtr sid;
      if (!ConvertStringSidToSidW("S-1-16-8192", out sid)) return "ConvertStringSidToSid error " + Marshal.GetLastWin32Error();
      TOKEN_MANDATORY_LABEL label = new TOKEN_MANDATORY_LABEL(); label.Label.Sid = sid; label.Label.Attributes = 0x20;
      int size = Marshal.SizeOf(typeof(TOKEN_MANDATORY_LABEL)) + GetLengthSid(sid);
      IntPtr buffer = Marshal.AllocHGlobal(size);
      Marshal.StructureToPtr(label, buffer, false);
      if (!SetTokenInformation(dup, 25, buffer, (uint)size)) return "SetTokenInformation error " + Marshal.GetLastWin32Error();
      STARTUPINFO si = new STARTUPINFO(); si.cb = Marshal.SizeOf(typeof(STARTUPINFO)); si.lpDesktop = "WinSta0\\Default";
      PROCESS_INFORMATION pi;
      if (!CreateProcessAsUserW(dup, null, new StringBuilder(commandLine), IntPtr.Zero, IntPtr.Zero, false, 0x00000010, IntPtr.Zero, null, ref si, out pi)) return "CreateProcessAsUser error " + Marshal.GetLastWin32Error();
      CloseHandle(pi.hThread); CloseHandle(pi.hProcess); CloseHandle(dup); CloseHandle(token);
      return "started pid " + pi.dwProcessId;
    }
    public static bool IsTopmost(IntPtr hwnd) { return (GetWindowLongPtr(hwnd, -20).ToInt64() & 0x8) != 0; }

    // The mandatory integrity level of a process: 0x2000 medium, 0x3000 high. Negative is an error.
    public static long IntegrityOf(uint pid) {
      IntPtr process = OpenProcess(0x1000, false, pid);
      if (process == IntPtr.Zero) return -Marshal.GetLastWin32Error();
      IntPtr token;
      if (!OpenProcessToken(process, 0x0008, out token)) { int e = Marshal.GetLastWin32Error(); CloseHandle(process); return -e; }
      uint length;
      GetTokenInformation(token, 25, IntPtr.Zero, 0, out length);
      IntPtr buffer = Marshal.AllocHGlobal((int)length);
      try {
        if (!GetTokenInformation(token, 25, buffer, length, out length)) return -Marshal.GetLastWin32Error();
        IntPtr sid = Marshal.ReadIntPtr(buffer);
        int count = Marshal.ReadByte(GetSidSubAuthorityCount(sid));
        return Marshal.ReadInt32(GetSidSubAuthority(sid, (uint)(count - 1)));
      } finally { Marshal.FreeHGlobal(buffer); CloseHandle(token); CloseHandle(process); }
    }

    // Tries, in order, the ways a background process can take the foreground, and says which
    // one worked: already, plain, after-alt-tap, attach-thread-input, or refused.
    public static string BringToFront(IntPtr hwnd) {
      if (GetForegroundWindow() == hwnd) return "already";
      SetForegroundWindow(hwnd); Thread.Sleep(150);
      if (GetForegroundWindow() == hwnd) return "plain";
      TapKey(0x12); SetForegroundWindow(hwnd); Thread.Sleep(150);
      if (GetForegroundWindow() == hwnd) return "after-alt-tap";
      uint foregroundThread = GetWindowThread(GetForegroundWindow(), IntPtr.Zero);
      uint me = GetCurrentThreadId();
      bool attached = foregroundThread != 0 && foregroundThread != me && AttachThreadInput(me, foregroundThread, true);
      SetForegroundWindow(hwnd); Thread.Sleep(150);
      if (attached) AttachThreadInput(me, foregroundThread, false);
      if (GetForegroundWindow() == hwnd) return "attach-thread-input";
      return "refused";
    }

    static int InputSize() { return Marshal.SizeOf(typeof(INPUT)); }
    public static int InputStructSize() { return InputSize(); }
    static uint SendOne(INPUT i) { return SendInput(1, new INPUT[] { i }, InputSize()); }
    static INPUT Key(ushort vk, ushort scan, uint flags) { INPUT i = new INPUT(); i.type = 1; i.u.ki.wVk = vk; i.u.ki.wScan = scan; i.u.ki.dwFlags = flags; return i; }
    static INPUT Mouse(uint flags) { INPUT i = new INPUT(); i.type = 0; i.u.mi.dwFlags = flags; return i; }

    // Unicode typing, one event at a time. Returns { inserted, refused, first error }.
    public static int[] TypeUnicode(string text) {
      int sent = 0, failed = 0, firstError = 0;
      foreach (char c in text) {
        uint[] flags = new uint[] { 0x0004, 0x0004 | 0x0002 };
        foreach (uint f in flags) {
          uint n = SendOne(Key(0, c, f));
          if (n == 1) sent++; else { failed++; if (firstError == 0) firstError = Marshal.GetLastWin32Error(); }
          Thread.Sleep(4);
        }
      }
      return new int[] { sent, failed, firstError };
    }
    // A virtual-key tap (down and up). Returns { inserted, refused, first error }.
    public static int[] TapKey(ushort vk) {
      int sent = 0, failed = 0, firstError = 0;
      uint[] flags = new uint[] { 0, 0x0002 };
      foreach (uint f in flags) {
        uint n = SendOne(Key(vk, 0, f));
        if (n == 1) sent++; else { failed++; if (firstError == 0) firstError = Marshal.GetLastWin32Error(); }
        Thread.Sleep(20);
      }
      return new int[] { sent, failed, firstError };
    }
    // A left click at a screen point. Returns { cursor moved there (1/0), down inserted, up inserted, error }.
    public static int[] ClickAt(int x, int y) {
      SetCursorPos(x, y); Thread.Sleep(30);
      POINT p; GetCursorPos(out p);
      int moved = (p.X == x && p.Y == y) ? 1 : 0;
      int a = (int)SendOne(Mouse(0x0002)); int e1 = Marshal.GetLastWin32Error(); Thread.Sleep(40);
      int b = (int)SendOne(Mouse(0x0004)); int e2 = Marshal.GetLastWin32Error();
      return new int[] { moved, a, b, a == 1 && b == 1 ? 0 : (e1 != 0 ? e1 : e2) };
    }
  }

  public class ProbeForm : Form {
    public volatile int Clicks;
    public volatile int RawKeyboard;
    public volatile int RawMouse;
    public volatile int RawWithDevice;
    public volatile int RawWithoutDevice;
    public string RegisterResult = "not registered";
    public ProbeForm(string title) {
      Text = title; Width = 440; Height = 240; StartPosition = FormStartPosition.Manual; Left = 30; Top = 30;
      Button b = new Button(); b.Text = "Probe button"; b.Name = "ProbeButton"; b.Left = 30; b.Top = 30; b.Width = 220; b.Height = 70;
      // The title counts clicks, so a client in another process can tell that its Invoke landed.
      b.Click += delegate(object s, EventArgs e) { Clicks++; Text = "Eileen probe window, clicks " + Clicks; };
      Controls.Add(b);
    }
    protected override void OnHandleCreated(EventArgs e) {
      base.OnHandleCreated(e);
      RAWINPUTDEVICE[] d = new RAWINPUTDEVICE[2];
      d[0].usUsagePage = 1; d[0].usUsage = 6; d[0].dwFlags = 0x00000100; d[0].hwndTarget = Handle;
      d[1].usUsagePage = 1; d[1].usUsage = 2; d[1].dwFlags = 0x00000100; d[1].hwndTarget = Handle;
      bool ok = Native.RegisterRawInputDevices(d, 2, (uint)Marshal.SizeOf(typeof(RAWINPUTDEVICE)));
      RegisterResult = ok ? "ok" : "error " + Marshal.GetLastWin32Error();
    }
    protected override void WndProc(ref Message m) {
      if (m.Msg == 0x00FF) {
        RAWINPUTHEADER h; uint size = (uint)Marshal.SizeOf(typeof(RAWINPUTHEADER));
        uint r = Native.GetRawInputData(m.LParam, 0x10000005, out h, ref size, (uint)Marshal.SizeOf(typeof(RAWINPUTHEADER)));
        if (r != uint.MaxValue) {
          if (h.dwType == 1) RawKeyboard++; else if (h.dwType == 0) RawMouse++;
          if (h.hDevice == IntPtr.Zero) RawWithoutDevice++; else RawWithDevice++;
        }
      }
      base.WndProc(ref m);
    }
  }

  // Runs the probe window on its own STA thread with its own message loop, so the PowerShell
  // thread is an ordinary out-of-thread UI Automation client.
  public class FormHost {
    public ProbeForm Form;
    public IntPtr Handle;
    private ManualResetEvent ready = new ManualResetEvent(false);
    public bool Start(string title) {
      Thread t = new Thread(delegate() {
        Form = new ProbeForm(title);
        Form.Shown += delegate(object s, EventArgs e) { Handle = Form.Handle; ready.Set(); };
        Application.Run(Form);
      });
      t.SetApartmentState(ApartmentState.STA); t.IsBackground = true; t.Start();
      return ready.WaitOne(15000);
    }
    public void Activate() { Form.Invoke(new MethodInvoker(delegate() { Form.Activate(); })); }
    public void Close() { Form.BeginInvoke(new MethodInvoker(delegate() { Form.Close(); })); }
  }

  public static class Shot {
    // { width, height, pixels, non-black pixels, distinct colours (capped at 100000) }
    public static long[] Stats(System.Drawing.Bitmap bmp) {
      System.Drawing.Rectangle rect = new System.Drawing.Rectangle(0, 0, bmp.Width, bmp.Height);
      System.Drawing.Imaging.BitmapData data = bmp.LockBits(rect, System.Drawing.Imaging.ImageLockMode.ReadOnly, System.Drawing.Imaging.PixelFormat.Format32bppArgb);
      try {
        int bytes = Math.Abs(data.Stride) * bmp.Height; byte[] buf = new byte[bytes];
        Marshal.Copy(data.Scan0, buf, 0, bytes);
        long nonBlack = 0; Dictionary<int, bool> colours = new Dictionary<int, bool>();
        for (int y = 0; y < bmp.Height; y++) {
          for (int x = 0; x < bmp.Width; x++) {
            int o = y * data.Stride + x * 4; int b = buf[o], g = buf[o + 1], r = buf[o + 2];
            if (r > 8 || g > 8 || b > 8) nonBlack++;
            if (colours.Count < 100000) colours[(r << 16) | (g << 8) | b] = true;
          }
        }
        return new long[] { bmp.Width, bmp.Height, (long)bmp.Width * bmp.Height, nonBlack, colours.Count };
      } finally { bmp.UnlockBits(data); }
    }
    public static System.Drawing.Bitmap PrintWin(IntPtr hwnd, out bool ok) {
      int[] r = Native.Rect(hwnd);
      System.Drawing.Bitmap bmp = new System.Drawing.Bitmap(Math.Max(1, r[2]), Math.Max(1, r[3]));
      using (System.Drawing.Graphics g = System.Drawing.Graphics.FromImage(bmp)) {
        IntPtr hdc = g.GetHdc();
        ok = Native.PrintWindow(hwnd, hdc, 2);
        g.ReleaseHdc(hdc);
      }
      return bmp;
    }
  }

  // The COM client Eileen's Rust code will use (IUIAutomation), declared only as far as needed;
  // the slots before the ones called keep the vtable order of UIAutomationClient.h.
  [ComImport, Guid("d22108aa-8ac5-49a5-837b-37bbb3d7591e"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  public interface IUIAutomationElement {
    [PreserveSig] int SetFocus();
    [PreserveSig] int GetRuntimeId(out IntPtr runtimeId);
    [PreserveSig] int FindFirst(int scope, IntPtr condition, out IUIAutomationElement found);
    [PreserveSig] int FindAll(int scope, IntPtr condition, out IUIAutomationElementArray found);
    [PreserveSig] int FindFirstBuildCache(int scope, IntPtr condition, IntPtr cache, out IntPtr found);
    [PreserveSig] int FindAllBuildCache(int scope, IntPtr condition, IntPtr cache, out IntPtr found);
    [PreserveSig] int BuildUpdatedCache(IntPtr cache, out IntPtr updated);
    [PreserveSig] int GetCurrentPropertyValue(int propertyId, [MarshalAs(UnmanagedType.Struct)] out object value);
    [PreserveSig] int GetCurrentPropertyValueEx(int propertyId, int ignoreDefault, [MarshalAs(UnmanagedType.Struct)] out object value);
    [PreserveSig] int GetCachedPropertyValue(int propertyId, [MarshalAs(UnmanagedType.Struct)] out object value);
    [PreserveSig] int GetCachedPropertyValueEx(int propertyId, int ignoreDefault, [MarshalAs(UnmanagedType.Struct)] out object value);
    [PreserveSig] int GetCurrentPatternAs(int patternId, ref Guid riid, out IntPtr pattern);
    [PreserveSig] int GetCachedPatternAs(int patternId, ref Guid riid, out IntPtr pattern);
    [PreserveSig] int GetCurrentPattern(int patternId, [MarshalAs(UnmanagedType.IUnknown)] out object pattern);
  }
  [ComImport, Guid("14314595-b4bc-4055-95f2-58f2e42c9855"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  public interface IUIAutomationElementArray {
    [PreserveSig] int get_Length(out int length);
    [PreserveSig] int GetElement(int index, out IUIAutomationElement element);
  }
  [ComImport, Guid("a94cd8b1-0844-4cd6-9d2d-640537ab39e9"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  public interface IUIAutomationValuePattern {
    [PreserveSig] int SetValue([MarshalAs(UnmanagedType.BStr)] string value);
    [PreserveSig] int get_CurrentValue([MarshalAs(UnmanagedType.BStr)] out string value);
  }
  [ComImport, Guid("30cbe57d-d9d0-452a-ab13-7ac5ac4825ee"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  public interface IUIAutomation {
    [PreserveSig] int CompareElements(IntPtr a, IntPtr b, out int same);
    [PreserveSig] int CompareRuntimeIds(IntPtr a, IntPtr b, out int same);
    [PreserveSig] int GetRootElement(out IUIAutomationElement root);
    [PreserveSig] int ElementFromHandle(IntPtr hwnd, out IUIAutomationElement element);
    [PreserveSig] int ElementFromPoint(POINT pt, out IUIAutomationElement element);
    [PreserveSig] int GetFocusedElement(out IUIAutomationElement element);
    [PreserveSig] int GetRootElementBuildCache(IntPtr cache, out IntPtr root);
    [PreserveSig] int ElementFromHandleBuildCache(IntPtr hwnd, IntPtr cache, out IntPtr element);
    [PreserveSig] int ElementFromPointBuildCache(POINT pt, IntPtr cache, out IntPtr element);
    [PreserveSig] int GetFocusedElementBuildCache(IntPtr cache, out IntPtr element);
    [PreserveSig] int CreateTreeWalker(IntPtr condition, out IntPtr walker);
    [PreserveSig] int get_ControlViewWalker(out IntPtr walker);
    [PreserveSig] int get_ContentViewWalker(out IntPtr walker);
    [PreserveSig] int get_RawViewWalker(out IntPtr walker);
    [PreserveSig] int get_RawViewCondition(out IntPtr condition);
    [PreserveSig] int get_ControlViewCondition(out IntPtr condition);
    [PreserveSig] int get_ContentViewCondition(out IntPtr condition);
    [PreserveSig] int CreateCacheRequest(out IntPtr request);
    [PreserveSig] int CreateTrueCondition(out IntPtr condition);
  }
  [ComImport, Guid("fb377fbe-8ea6-46d5-9c73-6499642d3059"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  public interface IUIAutomationInvokePattern {
    [PreserveSig] int Invoke();
  }
  public static class ComUia {
    // IUIAutomationInvokePattern::Invoke on the element of an HWND (pattern 10000).
    public static string Invoke(IntPtr hwnd) {
      IUIAutomationElement e; int hr = Client().ElementFromHandle(hwnd, out e);
      if (hr != 0 || e == null) return "ElementFromHandle hr=0x" + hr.ToString("x8");
      object p; hr = e.GetCurrentPattern(10000, out p);
      IUIAutomationInvokePattern ip = p as IUIAutomationInvokePattern;
      if (hr != 0 || ip == null) return "no InvokePattern (hr=0x" + hr.ToString("x8") + ")";
      return "invoke hr=0x" + ip.Invoke().ToString("x8");
    }
    static IUIAutomation Client() {
      Type t = Type.GetTypeFromCLSID(new Guid("ff48dba4-60ef-4201-aa87-54103eef594e"));
      return (IUIAutomation)Activator.CreateInstance(t);
    }
    static string Describe(IUIAutomationElement e) {
      object cls, ct, name;
      e.GetCurrentPropertyValue(30012, out cls);
      e.GetCurrentPropertyValue(30003, out ct);
      e.GetCurrentPropertyValue(30005, out name);
      return name + " [" + cls + "] " + ct;
    }
    // { element description, value property hr, value }
    public static string[] Read(IntPtr hwnd) {
      IUIAutomationElement e; int hr = Client().ElementFromHandle(hwnd, out e);
      if (hr != 0 || e == null) return new string[] { "ElementFromHandle hr=0x" + hr.ToString("x8"), "", "" };
      object val; int hr2 = e.GetCurrentPropertyValue(30045, out val);
      return new string[] { Describe(e), "hr=0x" + hr2.ToString("x8"), val == null ? "" : val.ToString() };
    }
    // The element's children in the raw view: "name [class] controlTypeId" each.
    public static string[] Children(IntPtr hwnd) {
      IUIAutomation a = Client();
      IUIAutomationElement e; int hr = a.ElementFromHandle(hwnd, out e);
      if (hr != 0 || e == null) return new string[] { "ElementFromHandle hr=0x" + hr.ToString("x8") };
      IntPtr cond; hr = a.CreateTrueCondition(out cond);
      if (hr != 0) return new string[] { "CreateTrueCondition hr=0x" + hr.ToString("x8") };
      IUIAutomationElementArray arr; hr = e.FindAll(2, cond, out arr);
      if (hr != 0 || arr == null) return new string[] { "FindAll hr=0x" + hr.ToString("x8") };
      int n; arr.get_Length(out n);
      List<string> list = new List<string>();
      for (int i = 0; i < n && i < 30; i++) { IUIAutomationElement c; arr.GetElement(i, out c); if (c != null) list.Add(Describe(c)); }
      return list.ToArray();
    }
    // Whether the element of an HWND offers a UIA pattern (10002 Value, 10018 LegacyIAccessible).
    public static string PatternPresent(IntPtr hwnd, int patternId) {
      IUIAutomationElement e; int hr = Client().ElementFromHandle(hwnd, out e);
      if (hr != 0 || e == null) return "ElementFromHandle hr=0x" + hr.ToString("x8");
      object p; hr = e.GetCurrentPattern(patternId, out p);
      return "hr=0x" + hr.ToString("x8") + " present=" + (p != null);
    }
    // IUIAutomationValuePattern::SetValue, then get_CurrentValue: { set hr, get hr, value }.
    public static string[] SetAndRead(IntPtr hwnd, string text) {
      IUIAutomationElement e; int hr = Client().ElementFromHandle(hwnd, out e);
      if (hr != 0 || e == null) return new string[] { "ElementFromHandle hr=0x" + hr.ToString("x8"), "", "" };
      object p; hr = e.GetCurrentPattern(10002, out p);
      IUIAutomationValuePattern vp = p as IUIAutomationValuePattern;
      if (hr != 0 || vp == null) return new string[] { "GetCurrentPattern hr=0x" + hr.ToString("x8"), "", "" };
      int hrSet = vp.SetValue(text);
      Thread.Sleep(200);
      string value; int hrGet = vp.get_CurrentValue(out value);
      return new string[] { "set hr=0x" + hrSet.ToString("x8"), "get hr=0x" + hrGet.ToString("x8"), value };
    }
  }

  [ComImport, Guid("A95664D2-9614-4F35-A746-DE8DB63617E6"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  public interface IMMDeviceEnumerator {
    [PreserveSig] int EnumAudioEndpoints(int dataFlow, uint stateMask, out IMMDeviceCollection devices);
    [PreserveSig] int GetDefaultAudioEndpoint(int dataFlow, int role, out IntPtr device);
  }
  [ComImport, Guid("0BD7A1BE-7A1A-44DB-8397-CC5392387B5E"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  public interface IMMDeviceCollection {
    [PreserveSig] int GetCount(out uint count);
  }
  public static class Audio {
    // { enum hr, endpoints in any state, enum hr, active endpoints, default endpoint hr }
    public static long[] Endpoints(int flow) {
      Type t = Type.GetTypeFromCLSID(new Guid("BCDE0395-E52F-467C-8E3D-C4579291692E"));
      IMMDeviceEnumerator en = (IMMDeviceEnumerator)Activator.CreateInstance(t);
      IMMDeviceCollection all, active; uint nAll = 0, nActive = 0;
      int hr1 = en.EnumAudioEndpoints(flow, 0x0000000F, out all); if (hr1 == 0 && all != null) all.GetCount(out nAll);
      int hr2 = en.EnumAudioEndpoints(flow, 0x00000001, out active); if (hr2 == 0 && active != null) active.GetCount(out nActive);
      IntPtr def; int hr3 = en.GetDefaultAudioEndpoint(flow, 0, out def);
      if (def != IntPtr.Zero) Marshal.Release(def);
      return new long[] { hr1, nAll, hr2, nActive, hr3 };
    }
  }
}
'@
# A child given -Dll loads the types the main run compiled: a medium-integrity child made by
# lowering this elevated token cannot start csc.exe (Add-Type fails with "A required privilege
# is not held by the client", run 5).
if ($Dll) { Add-Type -Path $Dll } else { Add-Type -TypeDefinition $cs -ReferencedAssemblies System.Windows.Forms, System.Drawing, $accessibilityDll -IgnoreWarnings }
$N = [EileenProbe.Native]
$AE = [System.Windows.Automation.AutomationElement]
$Scope = [System.Windows.Automation.TreeScope]
$CT = [System.Windows.Automation.ControlType]

function Cond($property, $value) { return New-Object System.Windows.Automation.PropertyCondition($property, $value) }
function AndC($a, $b) { return [System.Windows.Automation.AndCondition]::new([System.Windows.Automation.Condition[]]@($a, $b)) }
function OrC($a, $b) { return [System.Windows.Automation.OrCondition]::new([System.Windows.Automation.Condition[]]@($a, $b)) }
$NoName = [NullString]::Value
function WaitFor([scriptblock]$probe, [int]$seconds) {
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $deadline) {
        $v = & $probe
        if ($null -ne $v) { return $v }
        Start-Sleep -Milliseconds 250
    }
    return $null
}
function Patterns($element) { return @($element.GetSupportedPatterns() | ForEach-Object { $_.ProgrammaticName }) }
function ProcName([uint32]$procId) { try { return (Get-Process -Id $procId).ProcessName } catch { return "pid $procId (gone)" } }
function IntegrityName([long]$rid) {
    switch ($rid) { 0x1000 { 'low' } 0x2000 { 'medium' } 0x2100 { 'medium-plus' } 0x3000 { 'high' } 0x4000 { 'system' } default { "rid $rid" } }
}
function EditOf([IntPtr]$top) {
    $e = $N::FindWindowExW($top, [IntPtr]::Zero, 'Edit', $NoName)
    if ($e -eq [IntPtr]::Zero) {
        # Windows 11 Notepad nests its RichEdit inside a NotepadTextBox child.
        $box = $N::FindWindowExW($top, [IntPtr]::Zero, 'NotepadTextBox', $NoName)
        $parent = if ($box -ne [IntPtr]::Zero) { $box } else { $top }
        $e = $N::FindWindowExW($parent, [IntPtr]::Zero, 'RichEditD2DPT', $NoName)
    }
    return $e
}
function Describe($el) { return ("{0} [{1}] {2}" -f $el.Current.Name, $el.Current.ClassName, $el.Current.ControlType.ProgrammaticName) }
function RawChildren($el) {
    $walker = [System.Windows.Automation.TreeWalker]::RawViewWalker
    $list = @(); $c = $walker.GetFirstChild($el)
    while ($null -ne $c -and $list.Count -lt 30) { $list += (Describe $c); $c = $walker.GetNextSibling($c) }
    return $list
}

if ($Child) {
    # One UI Automation client started in one particular way (see section 'outside'). It uses
    # the COM client alone first, then the .NET client (System.Windows.Automation) once, then
    # the COM client again, because run 5 showed the COM client losing the Win32 proxies in a
    # process once the .NET client had been used there.
    Section 'child' {
        Rec 'child.label' $Label
        Rec 'child.integrity' (IntegrityName ($N::IntegrityOf([uint32]$PID)))
        Rec 'child.job' ($N::JobOf([uint32]$PID))
        Rec 'child.jobLimits' $N::OwnJobLimitFlags()
        if ($Label -eq 'explorer') { $script:ProbeResults['child.envNames'] = @(Get-ChildItem env: | ForEach-Object { $_.Name } | Sort-Object) }
        $tops = @($Targets -split ',' | ForEach-Object { [IntPtr][long]$_ })
        $fh = [IntPtr]$Form
        $bh = if ($Form -ne 0) { $N::FindWindowExW($fh, [IntPtr]::Zero, $NoName, 'Probe button') } else { [IntPtr]::Zero }
        function ComPass([string]$tag) {
            $i = 0
            foreach ($top in $tops) {
                $i++; $e = EditOf $top
                TryRec "child.t$i.$tag.children" { [EileenProbe.ComUia]::Children($top) }
                TryRec "child.t$i.$tag.read" { [EileenProbe.ComUia]::Read($e) }
                TryRec "child.t$i.$tag.setAndRead" { [EileenProbe.ComUia]::SetAndRead($e, "set by $Label $tag $i") }
            }
            if ($bh -ne [IntPtr]::Zero) {
                TryRec "child.form.$tag.button" { [EileenProbe.ComUia]::Read($bh)[0] }
                TryRec "child.form.$tag.invoke" { $before = $N::GetText($fh); $r = [EileenProbe.ComUia]::Invoke($bh); Start-Sleep -Milliseconds 400; "{0}, landed={1}" -f $r, ($N::GetText($fh) -ne $before) }
            }
        }
        $i = 0
        foreach ($top in $tops) { $i++; TryRec "child.t$i.targetIntegrity" { IntegrityName ($N::IntegrityOf($N::WindowPid($top))) } }
        ComPass 'com'
        TryRec 'child.t1.managed' { $el = $AE::FromHandle((EditOf $tops[0])); "{0}: {1}" -f (Describe $el), ((Patterns $el) -join ' ') }
        if ($bh -ne [IntPtr]::Zero) { TryRec 'child.form.managed' { $el = $AE::FromHandle($bh); "{0}: {1}" -f (Describe $el), ((Patterns $el) -join ' ') } }
        ComPass 'comAfterManaged'
        $i = 0
        foreach ($top in $tops) {
            $i++; $e = EditOf $top
            TryRec "child.t$i.foreground" { $N::BringToFront($top) }
            TryRec "child.t$i.sendInput" {
                if ($N::GetForegroundWindow() -eq $top) {
                    $mark = "typed by $Label $i"
                    $k = $N::TypeUnicode(" $mark"); Start-Sleep -Milliseconds 300
                    "inserted={0} refused={1} error={2} landed={3}" -f $k[0], $k[1], $k[2], ($N::GetText($e)).Contains($mark)
                } else { 'skipped: target is not the foreground window' }
            }
        }
    }
    if ($Spawn) {
        # Further clients started from this process, which is outside the runner's job.
        Section 'spawn' {
            $base = Split-Path -Parent $ChildOut
            foreach ($s in ($Spawn -split ',')) {
                $out = Join-Path $base "child-$s.json"; $log = Join-Path $base "child-$s.log"
                $argText = "-Child -Label $s -Targets $Targets -Form $Form -Dll $Dll -ChildOut $out -OutDir $OutDir-$s"
                $cmd = "cmd.exe /c powershell.exe -NoProfile -ExecutionPolicy Bypass -File $PSCommandPath $argText > $log 2>&1"
                $envFile = if ($s -eq 'runnerenv') { $RunnerEnv } else { $null }
                $r = $N::Launch($cmd, ($s -like '*medium*'), ($s -like 'freshjob*'), 0, $envFile)
                Rec "child.spawn.$s" $r
                $procId = if ($r -match 'pid (\d+)') { [int]$Matches[1] } else { 0 }
                [void](WaitFor { if (Test-Path $out) { $true } elseif ($procId -eq 0 -or -not (Get-Process -Id $procId -ErrorAction SilentlyContinue)) { $false } } 90)
            }
        }
    }
    $script:ProbeResults | ConvertTo-Json -Depth 6 | Set-Content -Path $ChildOut -Encoding UTF8
    exit 0
}

Section 'environment' {
    Rec 'environment.image' ("{0} {1}" -f $env:ImageOS, $env:ImageVersion)
    $os = Get-CimInstance Win32_OperatingSystem
    Rec 'environment.os' ("{0} build {1}" -f $os.Caption, $os.BuildNumber)
    Rec 'environment.arch' $env:PROCESSOR_ARCHITECTURE
    Rec 'environment.psVersion' $PSVersionTable.PSVersion.ToString()
    $principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
    Rec 'environment.elevated' $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    Rec 'environment.apartment' ([Threading.Thread]::CurrentThread.GetApartmentState().ToString())
    Rec 'environment.integrity' (IntegrityName ($N::IntegrityOf([uint32]$PID)))
    Rec 'environment.job' ($N::JobOf([uint32]$PID))
    Rec 'environment.jobUiRestrictions' $N::OwnJobUiRestrictions()
    Rec 'environment.uiaRegistry' @(foreach ($k in 'HKLM:\SOFTWARE\Microsoft\UIAutomation', 'HKCU:\Software\Microsoft\UIAutomation') { if (Test-Path $k) { Get-ChildItem -Recurse $k | ForEach-Object { $_.Name } ; (Get-ItemProperty $k | Out-String).Trim() -replace '\s+', ' ' } else { "$k absent" } })
    $uac = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System'
    Rec 'environment.uac' ("EnableLUA={0} ConsentPromptBehaviorAdmin={1} FilterAdministratorToken={2}" -f $uac.EnableLUA, $uac.ConsentPromptBehaviorAdmin, $uac.FilterAdministratorToken)
    Rec 'environment.runnerServices' @(Get-Service | Where-Object { $_.Name -like '*runner*' } | ForEach-Object { "$($_.Name) $($_.Status)" })
}

Section 'session' {
    $me = [Diagnostics.Process]::GetCurrentProcess()
    Rec 'session.processSessionId' $me.SessionId
    Rec 'session.activeConsoleSessionId' $N::WTSGetActiveConsoleSessionId()
    Rec 'session.userInteractive' ([Environment]::UserInteractive)
    Rec 'session.remoteSession' ($N::GetSystemMetrics(0x1000) -ne 0)
    Rec 'session.windowStation' $N::WindowStationName()
    $flags = $N::WindowStationFlags()
    Rec 'session.windowStationVisible' (($flags -band 1) -eq 1)
    Rec 'session.threadDesktop' $N::ThreadDesktopName()
    Rec 'session.inputDesktop' $N::InputDesktopName()
    $listener = Get-Process -Name 'Runner.Listener', 'Runner.Worker' -ErrorAction SilentlyContinue | ForEach-Object { "$($_.ProcessName) session $($_.SessionId)" }
    Rec 'session.runnerProcesses' @($listener)
    $explorer = @(Get-Process -Name explorer -ErrorAction SilentlyContinue)
    Rec 'session.explorerRunning' ($explorer.Count -gt 0)
    Rec 'session.taskbarFound' ($N::FindWindowW('Shell_TrayWnd', $NoName) -ne [IntPtr]::Zero)
    $q = (cmd.exe /c 'query session 2>&1' | Out-String).Trim() -split "`r?`n" | ForEach-Object { ($_ -replace '\s+', ' ').Trim() }
    Rec 'session.querySession' @($q)
    Rec 'session.idleMilliseconds' $N::IdleMilliseconds()
    Rec 'session.screenSaverActive' $N::Spi(0x0010)
    Rec 'session.screenSaverTimeoutSeconds' $N::Spi(0x000E)
}

Section 'display' {
    $ctx = $N::GetThreadDpiAwarenessContext()
    Rec 'display.awarenessBefore' $N::GetAwarenessFromDpiAwarenessContext($ctx)
    Rec 'display.systemDpiBefore' $N::GetDpiForSystem()
    $ok = $N::SetProcessDpiAwarenessContext([IntPtr](-4))
    Rec 'display.setPerMonitorV2' $(if ($ok) { 'ok' } else { 'error ' + [Runtime.InteropServices.Marshal]::GetLastWin32Error() })
    Rec 'display.awarenessAfter' $N::GetAwarenessFromDpiAwarenessContext($N::GetThreadDpiAwarenessContext())
    Rec 'display.systemDpi' $N::GetDpiForSystem()
    $m = $N::PrimaryMonitorDpi()
    Rec 'display.primaryMonitorDpi' ("hr={0} x={1} y={2}" -f (Hex $m[0]), $m[1], $m[2])
    Rec 'display.monitorCount' $N::GetSystemMetrics(80)
    Rec 'display.screens' @([System.Windows.Forms.Screen]::AllScreens | ForEach-Object { "{0} {1}x{2} at {3},{4} primary={5}" -f $_.DeviceName, $_.Bounds.Width, $_.Bounds.Height, $_.Bounds.X, $_.Bounds.Y, $_.Primary })
    Rec 'display.virtualScreen' ("{0}x{1}" -f $N::GetSystemMetrics(78), $N::GetSystemMetrics(79))
    $dwm = $false; $hr = $N::DwmIsCompositionEnabled([ref]$dwm)
    Rec 'display.dwmComposition' ("hr={0} enabled={1}" -f (Hex $hr), $dwm)
    Rec 'display.videoControllers' @(Get-CimInstance Win32_VideoController | ForEach-Object { "{0} {1}x{2} {3}Hz" -f $_.Name, $_.CurrentHorizontalResolution, $_.CurrentVerticalResolution, $_.CurrentRefreshRate })
    Rec 'display.inputStructSize' $N::InputStructSize()
}

# The windows-11-arm image showed a full-screen "Microsoft account" window (Shell_OOBEProxy,
# hosted by WWAHost) holding the foreground. Record it, then try WM_CLOSE and, failing that,
# ending WWAHost, to learn whether a CI job could clear it.
Section 'oobe' {
    $o = $N::FindWindowW('Shell_OOBEProxy', $NoName)
    Rec 'oobe.windowFound' ($o -ne [IntPtr]::Zero)
    if ($o -eq [IntPtr]::Zero) { return }
    Rec 'oobe.owner' (ProcName ($N::WindowPid($o)))
    Rec 'oobe.foregroundBefore' (ProcName ($N::WindowPid($N::GetForegroundWindow())))
    $res = [IntPtr]::Zero
    [void]$N::SendMessageTimeoutPtr($o, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero, 0x0002, 3000, [ref]$res)
    Start-Sleep -Seconds 2
    Rec 'oobe.visibleAfterClose' $N::IsWindowVisible($o)
    Rec 'oobe.foregroundAfterClose' (ProcName ($N::WindowPid($N::GetForegroundWindow())))
    if ((ProcName ($N::WindowPid($N::GetForegroundWindow()))) -eq 'WWAHost') {
        Get-Process -Name WWAHost -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
        Start-Sleep -Seconds 2
        Rec 'oobe.foregroundAfterEndingWwaHost' (ProcName ($N::WindowPid($N::GetForegroundWindow())))
    }
}

# The COM client (IUIAutomation, the one Eileen's Rust code uses) in this process, inside the
# runner's job, before anything here has touched System.Windows.Automation; then the .NET client
# once; then the same COM calls again.
Section 'comfirst' {
    $p = Start-Process -FilePath notepad.exe -PassThru
    $top = WaitFor { $h = $N::FindWindowW('Notepad', $NoName); if ($h -ne [IntPtr]::Zero) { $h } } 30
    Rec 'comfirst.windowFound' ($null -ne $top)
    if ($null -eq $top) { return }
    $tp = $N::WindowPid($top)
    Rec 'comfirst.notepad' ("{0}, {1}, {2}" -f (ProcName $tp), (IntegrityName ($N::IntegrityOf($tp))), $N::JobOf($tp))
    Start-Sleep -Milliseconds 500
    $e = EditOf $top
    $hf = New-Object EileenProbe.FormHost
    $formUp = $hf.Start('Eileen COM-first window')
    Start-Sleep -Milliseconds 300
    $bh = if ($formUp) { $N::FindWindowExW($hf.Handle, [IntPtr]::Zero, $NoName, 'Probe button') } else { [IntPtr]::Zero }
    foreach ($tag in 'com', 'comAfterManaged') {
        if ($tag -eq 'comAfterManaged') {
            TryRec 'comfirst.managedEdit' { $el = $AE::FromHandle($e); "{0}: {1}" -f (Describe $el), ((Patterns $el) -join ' ') }
            if ($bh -ne [IntPtr]::Zero) { TryRec 'comfirst.managedButton' { $el = $AE::FromHandle($bh); "{0}: {1}" -f (Describe $el), ((Patterns $el) -join ' ') } }
        }
        TryRec "comfirst.$tag.children" { [EileenProbe.ComUia]::Children($top) }
        TryRec "comfirst.$tag.read" { [EileenProbe.ComUia]::Read($e) }
        TryRec "comfirst.$tag.legacyPattern" { [EileenProbe.ComUia]::PatternPresent($e, 10018) }
        TryRec "comfirst.$tag.setAndRead" { [EileenProbe.ComUia]::SetAndRead($e, "set over COM, $tag") }
        if ($bh -ne [IntPtr]::Zero) {
            TryRec "comfirst.$tag.button" { [EileenProbe.ComUia]::Read($bh)[0] }
            TryRec "comfirst.$tag.invoke" { $r = [EileenProbe.ComUia]::Invoke($bh); Start-Sleep -Milliseconds 400; "{0}, clicks={1}" -f $r, $hf.Form.Clicks }
        }
    }
    if ($formUp) { $hf.Close() }
    Stop-Process -Id $tp -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 500
}

Section 'window' {
    $host1 = New-Object EileenProbe.FormHost
    $started = $host1.Start('Eileen probe window')
    Rec 'window.created' $started
    if (-not $started) { return }
    $ProbeState.form = $host1
    Start-Sleep -Milliseconds 500
    $h = $host1.Handle
    Rec 'window.registerRawInput' $host1.Form.RegisterResult
    Rec 'window.findWindowMatches' ($N::FindWindowW($NoName, 'Eileen probe window') -eq $h)
    Rec 'window.isVisible' $N::IsWindowVisible($h)
    Rec 'window.dpi' $N::GetDpiForWindow($h)
    Rec 'window.rect' $N::Rect($h)
    $fg = $N::GetForegroundWindow()
    Rec 'window.foregroundAtStart' ("{0} ({1})" -f (ProcName ($N::WindowPid($fg))), $(if ($fg -eq $h) { 'the probe window' } else { 'another window' }))
    $host1.Activate(); Start-Sleep -Milliseconds 300
    Rec 'window.foregroundIsProbeAfterActivate' ($N::GetForegroundWindow() -eq $h)
    Rec 'window.bringToFront' $N::BringToFront($h)
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $w = $AE::RootElement.FindFirst($Scope::Children, (Cond $AE::NameProperty 'Eileen probe window'))
    Rec 'window.uiaFoundTopLevel' ($null -ne $w)
    Rec 'window.uiaFindMs' $sw.ElapsedMilliseconds
    $rootKids = $AE::RootElement.FindAll($Scope::Children, [System.Windows.Automation.Condition]::TrueCondition)
    Rec 'window.uiaTopLevelWindows' @($rootKids | ForEach-Object {
        $wh = [IntPtr]$_.Current.NativeWindowHandle; $rc = $N::Rect($wh)
        "{0} [{1}] at {2},{3} {4}x{5} topmost={6}" -f $_.Current.Name, $_.Current.ClassName, $rc[0], $rc[1], $rc[2], $rc[3], $N::IsTopmost($wh)
    })
    $btn = $null
    if ($null -ne $w) {
        $btn = $w.FindFirst($Scope::Descendants, (AndC (Cond $AE::ControlTypeProperty $CT::Button) (Cond $AE::NameProperty 'Probe button')))
        Rec 'window.uiaFoundButton' ($null -ne $btn)
        if ($null -eq $btn) { TryRec 'window.rawChildren' { RawChildren $w } }
        TryRec 'window.comChildren' { [EileenProbe.ComUia]::Children($h) }
        $bh = $N::FindWindowExW($h, [IntPtr]::Zero, $NoName, 'Probe button')
        if ($bh -ne [IntPtr]::Zero) { TryRec 'window.buttonMsaa' { [EileenProbe.Native]::Msaa($bh) } }
    }
    if ($null -ne $btn) {
        Rec 'window.buttonAutomationId' $btn.Current.AutomationId
        $before = $host1.Form.Clicks
        $btn.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke(); Start-Sleep -Milliseconds 400
        Rec 'window.invokePatternClicked' ($host1.Form.Clicks -eq $before + 1)
    }
    $btnHwnd = $N::FindWindowExW($h, [IntPtr]::Zero, $NoName, 'Probe button')
    if ($btnHwnd -eq [IntPtr]::Zero) { Rec 'window.buttonHwndFound' $false; return }
    $bx = $N::Rect($btnHwnd)
    $x = $bx[0] + [int]($bx[2] / 2); $y = $bx[1] + [int]($bx[3] / 2)
    Rec 'window.buttonCenter' @($x, $y)
    TryRec 'window.elementFromPoint' { Describe ($AE::FromPoint((New-Object System.Windows.Point($x, $y)))) }
    TryRec 'window.windowFromPointIsButton' { $N::WindowAt($x, $y) -eq $btnHwnd }
    $rawBefore = @($host1.Form.RawMouse, $host1.Form.RawWithoutDevice, $host1.Form.RawWithDevice)
    $before = $host1.Form.Clicks
    if ($N::WindowAt($x, $y) -eq $btnHwnd) {
        $c = $N::ClickAt($x, $y); Start-Sleep -Milliseconds 400
        Rec 'window.sendInputClick' ("cursorMoved={0} down={1} up={2} error={3}" -f $c[0], $c[1], $c[2], $c[3])
        Rec 'window.sendInputClickLanded' ($host1.Form.Clicks -eq $before + 1)
        Rec 'rawinput.afterClick' ("mouse +{0}, without device +{1}, with device +{2}" -f ($host1.Form.RawMouse - $rawBefore[0]), ($host1.Form.RawWithoutDevice - $rawBefore[1]), ($host1.Form.RawWithDevice - $rawBefore[2]))
    } else {
        Rec 'window.sendInputClick' 'skipped: another window covers the button'
    }
    $kb = @($host1.Form.RawKeyboard, $host1.Form.RawWithoutDevice, $host1.Form.RawWithDevice)
    $k = $N::TapKey(0x10)
    Start-Sleep -Milliseconds 300
    Rec 'rawinput.afterShiftTap' ("sendInput={0}/{1} keyboard +{2}, without device +{3}, with device +{4}" -f $k[0], ($k[0] + $k[1]), ($host1.Form.RawKeyboard - $kb[0]), ($host1.Form.RawWithoutDevice - $kb[1]), ($host1.Form.RawWithDevice - $kb[2]))
}

Section 'apps' {
    $shell = New-Object -ComObject Shell.Application
    $folder = $shell.NameSpace('shell:::{4234d49b-0245-4df3-b780-3893943456e1}')
    $items = @($folder.Items())
    Rec 'apps.appsFolderCount' $items.Count
    $np = @($items | Where-Object { $_.Name -eq 'Notepad' } | ForEach-Object { $_.Path })
    Rec 'apps.notepadEntries' $np
    if ($np.Count -gt 0) { $ProbeState.notepadAumid = $np[0] }
    Rec 'apps.edgeEntry' @($items | Where-Object { $_.Name -eq 'Microsoft Edge' } | ForEach-Object { $_.Path })
}

Section 'notepad' {
    $existing = @($AE::RootElement.FindAll($Scope::Children, (Cond $AE::ClassNameProperty 'Notepad')) | ForEach-Object { $_.Current.NativeWindowHandle })
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $route = 'notepad.exe'
    if ($ProbeState.notepadAumid) {
        $route = 'shell:AppsFolder\' + $ProbeState.notepadAumid
        Start-Process -FilePath explorer.exe -ArgumentList $route
    } else {
        Start-Process -FilePath notepad.exe
    }
    Rec 'notepad.launchRoute' $route
    $np = WaitFor {
        $all = @($AE::RootElement.FindAll($Scope::Children, (Cond $AE::ClassNameProperty 'Notepad')))
        $all | Where-Object { $existing -notcontains $_.Current.NativeWindowHandle } | Select-Object -First 1
    } 30
    if ($null -eq $np -and $route -ne 'notepad.exe') {
        Rec 'notepad.aumidLaunchFound' $false
        $route = 'notepad.exe'; Start-Process -FilePath notepad.exe
        $np = WaitFor { $AE::RootElement.FindFirst($Scope::Children, (Cond $AE::ClassNameProperty 'Notepad')) } 30
    }
    Rec 'notepad.windowFound' ($null -ne $np)
    Rec 'notepad.secondsToWindow' ([math]::Round($sw.Elapsed.TotalSeconds, 2))
    if ($null -eq $np) { return }
    $hwnd = [IntPtr]$np.Current.NativeWindowHandle
    $ProbeState.notepad = $np; $ProbeState.notepadHwnd = $hwnd
    $ProbeState.notepadProcess = (ProcName $np.Current.ProcessId)
    Rec 'notepad.window' ("{0} [{1}] process {2}" -f $np.Current.Name, $np.Current.ClassName, $ProbeState.notepadProcess)
    Rec 'notepad.integrity' (IntegrityName ($N::IntegrityOf([uint32]$np.Current.ProcessId)))
    Rec 'notepad.isVisible' $N::IsWindowVisible($hwnd)
    Rec 'notepad.rect' $N::Rect($hwnd)
    $editHwnd = EditOf $hwnd
    $ProbeState.editHwnd = $editHwnd
    Rec 'notepad.editHwndFound' ($editHwnd -ne [IntPtr]::Zero)
    Rec 'notepad.job' ($N::JobOf([uint32]$np.Current.ProcessId))
    if ($editHwnd -ne [IntPtr]::Zero) {
        TryRec 'notepad.msaa' { [EileenProbe.Native]::Msaa($editHwnd) }
        TryRec 'notepad.comLegacyPattern' { [EileenProbe.ComUia]::PatternPresent($editHwnd, 10018) }
    }
    TryRec 'notepad.comChildren' { [EileenProbe.ComUia]::Children($hwnd) }
    TryRec 'notepad.managedRawChildren' { RawChildren $np }

    $sw = [Diagnostics.Stopwatch]::StartNew()
    $editCond = OrC (Cond $AE::ControlTypeProperty $CT::Edit) (Cond $AE::ControlTypeProperty $CT::Document)
    $edit = WaitFor { $np.FindFirst($Scope::Descendants, $editCond) } 5
    Rec 'notepad.managedFindFirstFoundEdit' ($null -ne $edit)
    Rec 'notepad.managedFindFirstMs' $sw.ElapsedMilliseconds
    if ($null -eq $edit -and $editHwnd -ne [IntPtr]::Zero) { $edit = $AE::FromHandle($editHwnd); Rec 'notepad.editVia' 'AutomationElement.FromHandle' }
    if ($null -ne $edit) {
        $ProbeState.edit = $edit
        Rec 'notepad.edit' (Describe $edit)
        Rec 'notepad.editPatterns' (Patterns $edit)
        $vp = $null
        if ($edit.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$vp)) {
            $ProbeState.vp = $vp
            $want = 'Eileen probe: set by ValuePattern'
            $vp.SetValue($want); Start-Sleep -Milliseconds 300
            $back = $vp.Current.Value
            Rec 'notepad.managedValueRoundTrip' ($back -eq $want)
            if ($back -ne $want) { Rec 'notepad.managedValueReadBack' $back }
            if ($editHwnd -ne [IntPtr]::Zero) { Rec 'notepad.wmGetTextAfterManagedSet' ($N::GetText($editHwnd) -eq $want) }
        } else { Rec 'notepad.managedValuePattern' 'not supported' }
    }
    if ($editHwnd -ne [IntPtr]::Zero) {
        $want = 'Eileen probe: set by IUIAutomationValuePattern'
        $com = [EileenProbe.ComUia]::SetAndRead($editHwnd, $want)
        Rec 'notepad.comSetAndRead' $com
        Rec 'notepad.comValueRoundTrip' ($com[2] -eq $want)
        Rec 'notepad.wmGetTextAfterComSet' ($N::GetText($editHwnd) -eq $want)
        TryRec 'notepad.comRead' { [EileenProbe.ComUia]::Read($editHwnd) }
    }

    # SendInput typing, only once Notepad holds the foreground (Eileen's own rule).
    Rec 'notepad.bringToFront' $N::BringToFront($hwnd)
    $fg = $N::GetForegroundWindow()
    Rec 'notepad.foregroundIsNotepad' ($fg -eq $hwnd)
    Rec 'notepad.foregroundWindow' (ProcName ($N::WindowPid($fg)))
    if ($fg -eq $hwnd -and $editHwnd -ne [IntPtr]::Zero) {
        if ($null -ne $edit) { try { $edit.SetFocus() } catch { Rec 'notepad.uiaSetFocusError' $_.Exception.Message } }
        [void][EileenProbe.ComUia]::SetAndRead($editHwnd, '')
        $typed = 'typed by SendInput 123'
        $t = $N::TypeUnicode($typed); Start-Sleep -Milliseconds 500
        Rec 'notepad.sendInputEvents' ("inserted={0} refused={1} firstError={2}" -f $t[0], $t[1], $t[2])
        $back = $N::GetText($editHwnd)
        Rec 'notepad.sendInputRoundTrip' ($back -eq $typed)
        if ($back -ne $typed) { Rec 'notepad.sendInputReadBack' $back }
    } else {
        Rec 'notepad.sendInput' 'skipped: Notepad is not the foreground window'
    }
    if ($ProbeState.form) { Rec 'rawinput.totals' ("keyboard {0}, mouse {1}, without device {2}, with device {3}" -f $ProbeState.form.Form.RawKeyboard, $ProbeState.form.Form.RawMouse, $ProbeState.form.Form.RawWithoutDevice, $ProbeState.form.Form.RawWithDevice) }

    $tr = $null
    if ($np.TryGetCurrentPattern([System.Windows.Automation.TransformPattern]::Pattern, [ref]$tr)) {
        $tr.Move(480, 60); Start-Sleep -Milliseconds 300
        Rec 'notepad.transformMoveRect' $N::Rect($hwnd)
    } else { Rec 'notepad.transformMove' 'not supported' }
}

Section 'screenshot' {
    $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bmp = New-Object System.Drawing.Bitmap($b.Width, $b.Height)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    try { $g.CopyFromScreen($b.X, $b.Y, 0, 0, $bmp.Size) } finally { $g.Dispose() }
    $st = [EileenProbe.Shot]::Stats($bmp)
    Rec 'screenshot.size' ("{0}x{1}" -f $st[0], $st[1])
    Rec 'screenshot.nonBlackFraction' ([math]::Round($st[3] / [double]$st[2], 4))
    Rec 'screenshot.allBlack' ($st[3] -eq 0)
    Rec 'screenshot.distinctColours' $st[4]
    $bmp.Save((Join-Path $OutDir 'screen.png'), [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    if ($ProbeState.notepadHwnd) {
        $ok = $false
        $pw = [EileenProbe.Shot]::PrintWin($ProbeState.notepadHwnd, [ref]$ok)
        $ps = [EileenProbe.Shot]::Stats($pw)
        Rec 'screenshot.printWindow' ("ok={0} {1}x{2} nonBlack={3} colours={4}" -f $ok, $ps[0], $ps[1], [math]::Round($ps[3] / [double][math]::Max(1, $ps[2]), 4), $ps[4])
        $pw.Save((Join-Path $OutDir 'notepad-printwindow.png'), [System.Drawing.Imaging.ImageFormat]::Png)
        $pw.Dispose()
    }
}

Section 'outside' {
    if (-not $ProbeState.notepadHwnd) { return }
    Rec 'outside.ownJobLimits' $N::OwnJobLimitFlags()
    $dir = Join-Path $env:LOCALAPPDATA 'Temp\eileen-probe-child'
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    # An explicit medium label, so the medium-integrity children can write their reports here.
    [void](icacls.exe $dir /setintegritylevel '(OI)(CI)M')
    $probe = Join-Path $dir 'probe.ps1'
    Copy-Item -Path $PSCommandPath -Destination $probe -Force
    # The targets: t1 is the Notepad from section 'notepad'. On images with the classic Notepad,
    # t2 is one this process starts (inside the runner's job) and t3 one started at medium
    # integrity, the level of an ordinary program on a user's PC.
    $targets = @([long]$ProbeState.notepadHwnd)
    if ($ProbeState.notepadProcess -ceq 'notepad') {
        $p2 = Start-Process -FilePath notepad.exe -PassThru
        $h2 = WaitFor { $p2.Refresh(); if ($p2.MainWindowHandle -ne [IntPtr]::Zero) { $p2.MainWindowHandle } } 15
        if ($null -ne $h2) { $targets += [long]$h2 }
        $m = $N::Launch('notepad.exe', $true, $false, 0, $null)
        $p3 = if ($m -match 'pid (\d+)') { [int]$Matches[1] } else { 0 }
        $h3 = WaitFor { $pp = Get-Process -Id $p3 -ErrorAction SilentlyContinue; if ($pp -and $pp.MainWindowHandle -ne [IntPtr]::Zero) { $pp.MainWindowHandle } } 15
        if ($null -ne $h3) { $targets += [long]$h3 } else { Rec 'outside.mediumNotepad' $m }
    }
    $i = 0
    Rec 'outside.targets' @(foreach ($t in $targets) { $i++; $tp = $N::WindowPid([IntPtr]$t); "t{0}: {1}, {2}, {3}" -f $i, (ProcName $tp), (IntegrityName ($N::IntegrityOf($tp))), $N::JobOf($tp) })
    $list = $targets -join ','
    $form = if ($ProbeState.form) { [long]$ProbeState.form.Handle } else { 0 }
    $envFile = Join-Path $dir 'runner-env.txt'
    Get-ChildItem env: | ForEach-Object { "{0}={1}" -f $_.Name, $_.Value } | Set-Content -Path $envFile -Encoding UTF8
    # The children load these types from a DLL built here by a separate process, so that a
    # medium-integrity child never needs csc.exe.
    $dll = Join-Path $dir 'EileenProbe.dll'
    $csFile = Join-Path $dir 'probe.cs'
    Set-Content -Path $csFile -Value $cs -Encoding UTF8
    $build = "Add-Type -AssemblyName System.Windows.Forms, System.Drawing, Accessibility; Add-Type -TypeDefinition (Get-Content -Raw '$csFile') -ReferencedAssemblies System.Windows.Forms, System.Drawing, '$accessibilityDll' -OutputAssembly '$dll' -OutputType Library -IgnoreWarnings"
    $ErrorActionPreference = 'Continue'
    $buildOut = & powershell.exe -NoProfile -EncodedCommand ([Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($build))) 2>&1 | Out-String
    $ErrorActionPreference = 'Stop'
    Rec 'outside.dllBuilt' (Test-Path $dll)
    if (-not (Test-Path $dll)) { Rec 'outside.dllBuildOutput' (($buildOut -replace '[^\x20-\x7e]', ' ').Trim()) }
    function ChildLine([string]$label, [string]$spawn) {
        $s = if ($spawn) { " -Spawn $spawn -RunnerEnv $envFile" } else { '' }
        return "powershell.exe -NoProfile -ExecutionPolicy Bypass -File $probe -Child -Label $label -Targets $list -Form $form -Dll $dll -ChildOut $dir\child-$label.json -OutDir $dir\out-$label$s > $dir\child-$label.log 2>&1"
    }
    # Waits for one child's report and copies it in as outside.<label>.*; without a report, keeps
    # the first lines of the child's own output instead.
    function Collect([string]$label, [string]$started, [int]$seconds) {
        Rec "outside.$label.start" $started
        if ($started -notmatch '^started') { return $null }
        $out = Join-Path $dir "child-$label.json"
        $procId = if ($started -match 'pid (\d+)') { [int]$Matches[1] } else { 0 }
        $done = WaitFor { if (Test-Path $out) { $true } elseif ($procId -ne 0 -and -not (Get-Process -Id $procId -ErrorAction SilentlyContinue)) { $false } } $seconds
        Start-Sleep -Milliseconds 500
        if (Test-Path $out) {
            $rep = Get-Content $out -Raw | ConvertFrom-Json
            foreach ($prop in $rep.PSObject.Properties) {
                if ($prop.Name -like '*.seconds' -or $prop.Name -eq 'child.envNames') { continue }
                Rec ($prop.Name -replace '^child\.', "outside.$label.") $prop.Value
            }
            return $rep
        }
        Rec "outside.$label.report" $(if ($done -eq $false) { 'none: the process exited' } else { "none within $seconds s" })
        $log = Join-Path $dir "child-$label.log"
        if (Test-Path $log) {
            # Printable ASCII only, and at most 1500 characters, whatever the child wrote.
            $text = [IO.File]::ReadAllText($log) -replace '[^\x20-\x7e\r\n]', '?'
            Rec "outside.$label.output" (($text.Substring(0, [Math]::Min(1500, $text.Length)) -replace '\r?\n', ' | ').Trim())
        }
        return $null
    }
    # 1. Inside the runner's job at medium integrity.
    [void](Collect 'jobmedium' ($N::Launch(('cmd.exe /c ' + (ChildLine 'jobmedium' '')), $true, $false, 0, $null)) 90)
    # 2. Out of the runner's job by CREATE_BREAKAWAY_FROM_JOB, with the runner's environment.
    [void](Collect 'breakaway' ($N::Launch(('cmd.exe /c ' + (ChildLine 'breakaway' '')), $false, $false, 0x01000000, $null)) 90)
    # 3. Started by Explorer, outside any job, as a user starts a program. It then starts three
    #    clients itself: in a new kill-on-close job at high and at medium integrity (the shape
    #    eileen_supervisor gives its sidecars), and at medium integrity in no job. (Run 5 also
    #    started one with the runner's environment block; it matched Explorer's in every check.)
    $grand = 'freshjob,freshjobmedium,medium'
    $cmdFile = Join-Path $dir 'child-explorer.cmd'
    Set-Content -Path $cmdFile -Encoding ASCII -Value ('@' + (ChildLine 'explorer' $grand))
    Start-Process -FilePath explorer.exe -ArgumentList $cmdFile
    $ex = Collect 'explorer' 'started by explorer.exe' 400
    if ($null -ne $ex) {
        $runnerNames = @(Get-ChildItem env: | ForEach-Object { $_.Name })
        $explorerNames = @($ex.'child.envNames')
        Rec 'outside.envOnlyInRunner' @($runnerNames | Where-Object { $explorerNames -notcontains $_ } | Sort-Object)
        Rec 'outside.envOnlyInExplorer' @($explorerNames | Where-Object { $runnerNames -notcontains $_ } | Sort-Object)
        foreach ($g in ($grand -split ',')) { [void](Collect $g ([string]$ex."child.spawn.$g") 5) }
    }
    # 4. Started by Task Scheduler in the interactive session, a route a CI job could use.
    $started = $(try {
        $action = New-ScheduledTaskAction -Execute 'cmd.exe' -Argument ('/c ' + (ChildLine 'schtask' ''))
        $principal = New-ScheduledTaskPrincipal -UserId ([Security.Principal.WindowsIdentity]::GetCurrent().Name) -LogonType Interactive -RunLevel Highest
        Register-ScheduledTask -TaskName 'EileenProbeChild' -Action $action -Principal $principal -Force | Out-Null
        Start-ScheduledTask -TaskName 'EileenProbeChild'
        'started by Task Scheduler'
    } catch { 'error ' + $_.Exception.Message })
    [void](Collect 'schtask' $started 90)
    Unregister-ScheduledTask -TaskName 'EileenProbeChild' -Confirm:$false -ErrorAction SilentlyContinue
    $i = 0
    foreach ($t in $targets) { $i++; Rec "outside.t$i.textAtEnd" ($N::GetText((EditOf ([IntPtr]$t)))) }
    Rec 'outside.formTitleAtEnd' $(if ($form -ne 0) { $N::GetText([IntPtr]$form) } else { 'no form' })
}

Section 'audio' {
    Rec 'audio.services' @(Get-Service -Name Audiosrv, AudioEndpointBuilder -ErrorAction SilentlyContinue | ForEach-Object { "{0} {1} {2}" -f $_.Name, $_.Status, $_.StartType })
    Rec 'audio.soundDevices' @(Get-CimInstance Win32_SoundDevice | ForEach-Object { $_.Name })
    $key = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render'
    $render = @(if (Test-Path $key) { Get-ChildItem $key | ForEach-Object { (Get-ItemProperty $_.PSPath).DeviceState } })
    Rec 'audio.registryRenderEndpoints' ("count={0} states={1}" -f $render.Count, ($render -join ','))
    foreach ($flow in @(@('render', 0), @('capture', 1))) {
        $e = [EileenProbe.Audio]::Endpoints($flow[1])
        Rec ("audio.mmdevice." + $flow[0]) ("enumHr={0} anyState={1} active={2} defaultHr={3}" -f (Hex $e[0]), $e[1], $e[3], (Hex $e[4]))
    }
}

Section 'edge' {
    $edge = @("${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe", "$env:ProgramFiles\Microsoft\Edge\Application\msedge.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1
    Rec 'edge.installed' ($null -ne $edge)
    if ($null -eq $edge) { return }
    $page = Join-Path $OutDir 'page.html'
    Set-Content -Path $page -Encoding UTF8 -Value '<!doctype html><title>Eileen probe page</title><p>probe</p><input id="f" aria-label="Probe field" value="hello">'
    $url = ([Uri]$page).AbsoluteUri + '?q=1#frag'
    $profileDir = Join-Path $OutDir 'edge-profile'
    $sw = [Diagnostics.Stopwatch]::StartNew()
    Start-Process -FilePath $edge -ArgumentList @("--user-data-dir=`"$profileDir`"", '--no-first-run', '--no-default-browser-check', '--disable-sync', '--new-window', $url) | Out-Null
    $win = WaitFor {
        @($AE::RootElement.FindAll($Scope::Children, (Cond $AE::ClassNameProperty 'Chrome_WidgetWin_1'))) | Where-Object { $_.Current.Name -like '*Eileen probe page*' } | Select-Object -First 1
    } 45
    Rec 'edge.windowFound' ($null -ne $win)
    Rec 'edge.secondsToWindow' ([math]::Round($sw.Elapsed.TotalSeconds, 2))
    if ($null -eq $win) {
        Rec 'edge.topLevel' @($AE::RootElement.FindAll($Scope::Children, [System.Windows.Automation.Condition]::TrueCondition) | ForEach-Object { "{0} [{1}]" -f $_.Current.Name, $_.Current.ClassName })
        return
    }
    Rec 'edge.windowName' $win.Current.Name
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $bar = WaitFor { $win.FindFirst($Scope::Descendants, (AndC (Cond $AE::ControlTypeProperty $CT::Edit) (Cond $AE::NameProperty 'Address and search bar'))) } 15
    Rec 'edge.addressBarFound' ($null -ne $bar)
    if ($null -ne $bar) {
        $bv = $null
        if ($bar.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$bv)) {
            $shown = $bv.Current.Value
            Rec 'edge.addressBarValue' $shown
            Rec 'edge.addressBarKeepsQueryAndFragment' (($shown -like '*q=1*') -and ($shown -like '*#frag*'))
        }
    }
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $field = WaitFor { $win.FindFirst($Scope::Descendants, (Cond $AE::NameProperty 'Probe field')) } 20
    Rec 'edge.webContentFieldFound' ($null -ne $field)
    Rec 'edge.secondsToWebContent' ([math]::Round($sw.Elapsed.TotalSeconds, 2))
    if ($null -ne $field) {
        $fv = $null
        if ($field.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$fv)) {
            $fv.SetValue('set over UIA'); Start-Sleep -Milliseconds 300
            Rec 'edge.webFieldRoundTrip' ($fv.Current.Value -eq 'set over UIA')
        }
    }
}

# Last of the UI sections: if WM_CLOSE handling ever blocks, only cleanup is lost.
Section 'close' {
    if (-not $ProbeState.notepad) { return }
    $wp = $null
    if (-not $ProbeState.notepad.TryGetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern, [ref]$wp)) { Rec 'close.windowPattern' 'not supported'; return }
    $npPid = $ProbeState.notepad.Current.ProcessId
    $wp.Close()
    $gone = WaitFor { if (-not $N::IsWindowVisible($ProbeState.notepadHwnd)) { $true } } 4
    Rec 'close.windowGoneWithoutPrompt' ($gone -eq $true)
    if ($gone -eq $true) { return }
    $dialog = WaitFor {
        $found = $AE::RootElement.FindFirst($Scope::Children, (AndC (Cond $AE::ProcessIdProperty $npPid) (Cond $AE::ClassNameProperty '#32770')))
        if ($null -eq $found) { $found = $ProbeState.notepad.FindFirst($Scope::Descendants, (AndC (Cond $AE::ProcessIdProperty $npPid) (Cond $AE::ControlTypeProperty $CT::Window))) }
        $found
    } 8
    Rec 'close.unsavedDialogFound' ($null -ne $dialog)
    if ($null -eq $dialog) { return }
    $buttons = @($dialog.FindAll($Scope::Descendants, (Cond $AE::ControlTypeProperty $CT::Button)) | ForEach-Object { $_.Current.Name })
    Rec 'close.dialogButtons' $buttons
    $dont = $dialog.FindFirst($Scope::Descendants, (AndC (Cond $AE::ControlTypeProperty $CT::Button) (Cond $AE::NameProperty "Don't Save")))
    if ($null -eq $dont) { $dont = $dialog.FindFirst($Scope::Descendants, (AndC (Cond $AE::ControlTypeProperty $CT::Button) (Cond $AE::NameProperty "Don't save"))) }
    if ($null -ne $dont) {
        $dont.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
        $gone = WaitFor { if (-not $N::IsWindowVisible($ProbeState.notepadHwnd)) { $true } } 8
        Rec 'close.closedWithoutSaving' ($gone -eq $true)
    }
}

Section 'cleanup' {
    # The children's folder is kept: the workflow's next step prints their logs.
    Get-Process -Name msedge, notepad -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Remove-Item -Force -ErrorAction SilentlyContinue (Join-Path $env:LOCALAPPDATA 'Temp\eileen-probe-child\runner-env.txt')
    if ($ProbeState.form) { $ProbeState.form.Close() }
}

if ($env:GITHUB_STEP_SUMMARY) {
    $lines = @('| Check | Result |', '|---|---|')
    foreach ($k in $script:ProbeResults.Keys) {
        if ($k -like '*.seconds') { continue }
        $v = $script:ProbeResults[$k] | ConvertTo-Json -Compress -Depth 3
        $lines += ('| {0} | {1} |' -f $k, ($v -replace '\|', '/'))
    }
    $lines | Add-Content -Path $env:GITHUB_STEP_SUMMARY -Encoding UTF8
}
Write-Host "report written to $(Join-Path $OutDir 'report.json')"
exit 0
```
