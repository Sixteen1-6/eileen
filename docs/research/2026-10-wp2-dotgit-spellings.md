# WP2 spike: how Windows resolves the other spellings of `.git`

Date: 2026-10-21. Branch `wp2/git-path-spike`. This note settles the question BUILD.md section 7 carried into WP2 acceptance 10: does Windows' path handling hand the path classifier the spellings of `.git` that git's own `core.protectNTFS` guards against, or resolve them first, for paths that exist and paths that do not? Decision D71 made Eileen recognise a `.git` component the way git does; this note measures whether that is enough.

## The answer

Windows resolves the spellings in two places: the Win32 string layer (which GetFullPathNameW models) and NTFS itself when a file is opened. Neither place alone is safe to inspect, and git's own rule covers only part of what reaches `.git`.

1. **The string layer strips less than git assumes.** Trailing dots and spaces are removed from the last component only. An inner component loses one trailing dot and nothing else. So `sub\.git.\config` reaches `.git`, while `sub\.git \hooks\x`, `sub\.git..\config` and `sub\.git. .\config` name folders that do not exist. Git's rule flags all of them, which is safe.
2. **NTFS resolves the rest at open time.** Case (ASCII only), 8.3 short names and the `::$INDEX_ALLOCATION` and `:$I30:$INDEX_ALLOCATION` suffixes all reach `.git` and pass through GetFullPathNameW unchanged.
3. **Creating a path follows the same rules.** `mkdir sub\.git..`, `sub\.git ` and `sub\.git::$INDEX_ALLOCATION` each create `.git`. A file created as `.git::$DATA` or `.git:$DATA` is a `.git` file. `mkdir sub\GIT~1` creates a literal `GIT~1` folder, not `.git`.
4. **The final path from an open handle names the real `.git`** for every existing spelling, including junctions into `.git`, the `\\?\`, `\\.\`, `\??\`, GLOBALROOT and volume-GUID prefixes, and UNC paths back to this machine. Two cases escape it. A hard link keeps the name it was opened by. A `.git` that is itself a junction loses its name, so only the path as written shows it.
5. **`GIT~<n>` is not the only short name.** Once `git~1` to `git~4` exist in a folder, NTFS names `.git` `GI2837~1`, the same in two different folders. Git's own rule knows only `git~1`, because git guarantees `.git` is the first entry of a checkout.
6. **NTFS ignores no Unicode code point.** The ignorable code points in git's source belong to its HFS+ rule (`is_hfs_dotgit`), not to `is_ntfs_dotgit`. A sweep of all 65,535 BMP code units found no non-ASCII character that NTFS reads as `.`, `g`, `i` or `t` on this volume. The ANSI best-fit mapping is another matter: an ANSI program handed `sub\．ｇｉｔ\config` (fullwidth `.git`, U+FF0E U+FF47 U+FF49 U+FF54) or `sub＼.git＼config` (fullwidth backslashes, U+FF3C) opens `sub\.git\config`.
7. **The shells add their own spellings.** PowerShell's `-Path` wildcards `[t]`, `?` and `*` reach the hidden `.git` when `-Force` is given. Git Bash expands `[t]`, `?`, `*` and `{t,x}`, in redirections too. Git Bash's `/c/...` form reads as `C:\c\...` to GetFullPathNameW, so a string-only classifier misplaces it.
8. **This machine's own shares reach the job's `.git`.** An unelevated process here opened the nested repository's files through `\\localhost\C$`, `\\127.0.0.1\C$`, `\\?\UNC\localhost\C$`, `\\.\UNC\localhost\C$` and the computer's own name. BUILD.md classified every UNC path `Elsewhere`, which would also turn a tier 3 credential read into a tier 2 one.

So the `GitInternals` rule in BUILD.md was right in spirit and incomplete in its mechanism. The recommended rule is in the section "What the classifier must do", and the amendment to BUILD.md is listed after it.

## Setup

| Item | Value |
|---|---|
| Machine | The owner's PC: Windows 11 Home, build 26200, not elevated, Developer Mode off |
| Volume | The NTFS system volume. The 8.3 registry state is 2 (per volume); the per-volume query needs elevation, so short-name generation was confirmed by observing it |
| Code page | Active ANSI code page 1252 |
| Tools | Python 3.13 with ctypes (CreateFileW, CreateFileA, GetFullPathNameW, GetLongPathNameW, GetShortPathNameW, GetFinalPathNameByHandleW, GetFileInformationByHandle, FindFirstFileNameW, CreateHardLinkW, CreateSymbolicLinkW, WideCharToMultiByte); Windows PowerShell 5.1; cmd.exe; the bash of Git for Windows 2.49.0 |
| Fixture | A fresh temporary folder: a job repository `job`, a nested repository `job\sub` (its `.git` holds a probe file `eileen-probe`), a worktree pointer file `job\wt\.git`, a junction `job\alias` to `sub\.git`, a folder `job\sub2` whose `.git` is a junction to a git directory `job\plain`, a junction `job\ext` to an unrelated repository `other`, a junction `outside\jobalias` to the job folder, a hard link `job\hard.txt` to the probe, and folders where `git~1` (and `git~1` to `git~4`) exist before `.git` is made |
| Identity | Each spelling is opened and its volume serial and file index compared with the real objects, so "reaches" means the same file, not a similar name |
| Settings | None changed. Symbolic links could not be created (no privilege without Developer Mode), so they are not measured |

The script that produced every table below is at the end of this note. It needs only Python and runs in about 30 seconds with `--sweep --shells`. It prints `<root>` and `<job>` in place of the temporary folder, `<computer-name>` for the computer's name and `Volume{<guid>}` for the volume GUID.

## What git guards against

The rules below are from git v2.55.0.

- `is_ntfs_dotgit` in `path.c` matches a path component that is `.git` or `git~1` in any ASCII case, followed only by dots and spaces up to the end of the component or a colon. The colon clause covers every alternate data stream, so `.git::$INDEX_ALLOCATION` is caught. Its comment says `.git` could get `git~2` if `git~1` were taken, and that git avoids this by making `.git` the first entry of a directory.
- `verify_path` in `read-cache.c` applies it to every component of a tracked path when `core.protectNTFS` is on, rejects a backslash in a path on Windows, and rejects `.git` in any case on every platform.
- `is_valid_win32_path` in `compat/mingw.c` refuses segments that end in a space or a dot, reserved device names and the characters `<>:"|?*`.
- `is_hfs_dotgit` in `utf8.c` skips sixteen code points HFS+ ignores (U+200C to U+200F, U+202A to U+202E, U+206A to U+206F, U+FEFF). That list is for HFS+, not NTFS.
- `is_ntfs_dot_generic` handles `.gitmodules`, `.gitignore` and `.gitattributes`, including a hashed fallback short name such as `gi7eba~1`. There is no hashed form for `.git` itself.
- CVE-2014-9390 (git 2.2.1) introduced the case, `git~1` and HFS+ checks. CVE-2019-1352 added the alternate-data-stream case. CVE-2019-1353 found the NTFS checks inactive under WSL; git's source now defaults `protect_ntfs` to 1 on every platform (`environment.c`), although `core.adoc` still says the default is false outside Windows.

Git's problem is narrower than Eileen's. Git decides from tree data before it writes, with no file to open. Eileen's runtime can open the file, so it can ask Windows what a spelling reaches instead of predicting it.

## Results

In every table `␣` is a space and `<U+XXXX>` a code point. "Caught by" names the steps of the recommended rule that flag the path as git internals: 2 is the path as written, after Win32 normalisation; 4 is the final path from the handle; 5 is a hard link's other names; 6 is an ancestor that holds a git directory; 7 is this machine's own share mapped back to its local path. Every write test (a new file created through the same folder spelling) landed in the folder the read test reached, with one exception: in r35, `name:$DATA` names a stream called `$DATA`, so the read found none while the write created a new file inside `.git` carrying that stream. The final path of a named stream keeps its `:name` suffix, which the colon clause of step 2 handles.

### Existing paths

Paths are relative to `<job>` unless they start with a prefix. The probe file is `sub\.git\eileen-probe`.

| id | spelling | Win32 string (GetFullPathNameW) | reaches | final path (by handle) | git's rule | caught by | class |
|---|---|---|---|---|---|---|---|
| r00 | `sub\.git\eileen-probe` | `<job>\sub\.git\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | yes | 2, 4, 5, 6 | GitInternals |
| r01 | `sub\.git` | `<job>\sub\.git` | yes: `sub\.git` | `\\?\<job>\sub\.git` | yes | 2, 4, 6 | GitInternals |
| r02 | `sub\.GIT\eileen-probe` | `<job>\sub\.GIT\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | yes | 2, 4, 5, 6 | GitInternals |
| r03 | `sub\.Git\eileen-probe` | `<job>\sub\.Git\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | yes | 2, 4, 5, 6 | GitInternals |
| r04 | `SUB\.gIT\EILEEN-PROBE` | `<job>\SUB\.gIT\EILEEN-PROBE` | yes: `sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | yes | 2, 4, 5, 6 | GitInternals |
| r05 | `sub\.git.\eileen-probe` | `<job>\sub\.git\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | yes | 2, 4, 5, 6 | GitInternals |
| r06 | `sub\.git..\eileen-probe` | `<job>\sub\.git..\eileen-probe` | no: path not found | - | yes | 2, 4, 6 | GitInternals |
| r07 | `sub\.git...\eileen-probe` | `<job>\sub\.git...\eileen-probe` | no: path not found | - | yes | 2, 4, 6 | GitInternals |
| r08 | `sub\.git␣\eileen-probe` | `<job>\sub\.git␣\eileen-probe` | no: path not found | - | yes | 2, 4, 6 | GitInternals |
| r09 | `sub\.git.␣.\eileen-probe` | `<job>\sub\.git.␣\eileen-probe` | no: path not found | - | yes | 2, 4, 6 | GitInternals |
| r10 | `sub\.git␣.␣.␣.\eileen-probe` | `<job>\sub\.git␣.␣.␣\eileen-probe` | no: path not found | - | yes | 2, 4, 6 | GitInternals |
| r11 | `sub\.git.` | `<job>\sub\.git` | yes: `sub\.git` | `\\?\<job>\sub\.git` | yes | 2, 4, 6 | GitInternals |
| r12 | `sub\.git␣` | `<job>\sub\.git` | yes: `sub\.git` | `\\?\<job>\sub\.git` | yes | 2, 4, 6 | GitInternals |
| r13 | `sub\.git.␣.␣.` | `<job>\sub\.git` | yes: `sub\.git` | `\\?\<job>\sub\.git` | yes | 2, 4, 6 | GitInternals |
| r14 | `sub\.git\eileen-probe.` | `<job>\sub\.git\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | yes | 2, 4, 5, 6 | GitInternals |
| r15 | `sub\.git\eileen-probe␣␣` | `<job>\sub\.git\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | yes | 2, 4, 5, 6 | GitInternals |
| r16 | `sub\.git␣\` | `<job>\sub\.git␣\` | no: not found | - | yes | 2, 4 | GitInternals |
| r17 | `sub\␣.git\eileen-probe` | `<job>\sub\␣.git\eileen-probe` | no: path not found | - | no | - | JobFolder |
| r18 | `sub\GIT~1\eileen-probe` | `<job>\sub\GIT~1\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | yes | 2, 4, 5, 6 | GitInternals |
| r19 | `sub\git~1\eileen-probe` | `<job>\sub\git~1\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | yes | 2, 4, 5, 6 | GitInternals |
| r20 | `sub\GIT~1.\eileen-probe` | `<job>\sub\GIT~1\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | yes | 2, 4, 5, 6 | GitInternals |
| r21 | `sub\GIT~1␣\eileen-probe` | `<job>\sub\GIT~1␣\eileen-probe` | no: path not found | - | yes | 2, 4, 6 | GitInternals |
| r22 | `sub\GIT~2\eileen-probe` | `<job>\sub\GIT~2\eileen-probe` | no: path not found | - | no | 2, 4 | GitInternals |
| r23 | `sub\.git\EILEEN~1` | `<job>\sub\.git\EILEEN~1` | yes: `sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | yes | 2, 4, 5, 6 | GitInternals |
| r24 | `..\collide1\GIT~2` | `<root>\collide1\GIT~2` | yes: `collide1\.git` | `\\?\<root>\collide1\.git` | no | 2, 4 | Elsewhere, git |
| r25 | `..\collide4\GI2837~1` | `<root>\collide4\GI2837~1` | yes: `collide4\.git` | `\\?\<root>\collide4\.git` | no | 2, 4 | Elsewhere, git |
| r26 | `sub\.git::$INDEX_ALLOCATION\eileen-probe` | `<job>\sub\.git::$INDEX_ALLOCATION\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | yes | 2, 4, 5, 6 | GitInternals |
| r27 | `sub\.git:$I30:$INDEX_ALLOCATION\eileen-probe` | `<job>\sub\.git:$I30:$INDEX_ALLOCATION\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | yes | 2, 4, 5, 6 | GitInternals |
| r28 | `sub\.GIT::$index_allocation\eileen-probe` | `<job>\sub\.GIT::$index_allocation\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | yes | 2, 4, 5, 6 | GitInternals |
| r29 | `sub\.git::$INDEX_ALLOCATION` | `<job>\sub\.git::$INDEX_ALLOCATION` | yes: `sub\.git` | `\\?\<job>\sub\.git` | yes | 2, 4, 6 | GitInternals |
| r30 | `sub\.git:$DATA\eileen-probe` | `<job>\sub\.git:$DATA\eileen-probe` | no: invalid name | - | yes | 2, 4 | GitInternals |
| r31 | `sub\.git::$DATA\eileen-probe` | `<job>\sub\.git::$DATA\eileen-probe` | no: invalid name | - | yes | 2, 4 | GitInternals |
| r32 | `sub\.git:\eileen-probe` | `<job>\sub\.git:\eileen-probe` | no: invalid name | - | yes | 2, 4 | GitInternals |
| r33 | `sub\.git:` | `<job>\sub\.git:` | no: invalid name | - | yes | 2, 4 | GitInternals |
| r34 | `sub\.git\eileen-probe::$DATA` | `<job>\sub\.git\eileen-probe::$DATA` | yes: `sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | yes | 2, 4, 5, 6 | GitInternals |
| r35 | `sub\.git\eileen-probe:$DATA` | `<job>\sub\.git\eileen-probe:$DATA` | no: not found | - | yes | 2, 4, 6 | GitInternals |
| r36 | `wt\.git::$DATA` | `<job>\wt\.git::$DATA` | yes: `wt\.git` (pointer file) | `\\?\<job>\wt\.git` | yes | 2, 4 | GitInternals |
| r37 | `wt\.git:$DATA` | `<job>\wt\.git:$DATA` | no: not found | - | yes | 2, 4 | GitInternals |
| r38 | `wt\.git.` | `<job>\wt\.git` | yes: `wt\.git` (pointer file) | `\\?\<job>\wt\.git` | yes | 2, 4 | GitInternals |
| r39 | `wt\GIT~1` | `<job>\wt\GIT~1` | yes: `wt\.git` (pointer file) | `\\?\<job>\wt\.git` | yes | 2, 4 | GitInternals |
| r40 | `sub\.g<U+200C>it\eileen-probe` | `<job>\sub\.g<U+200C>it\eileen-probe` | no: path not found | - | no | - | JobFolder |
| r41 | `sub\<U+FEFF>.git\eileen-probe` | `<job>\sub\<U+FEFF>.git\eileen-probe` | no: path not found | - | no | - | JobFolder |
| r42 | `sub\.git<U+200B>\eileen-probe` | `<job>\sub\.git<U+200B>\eileen-probe` | no: path not found | - | no | - | JobFolder |
| r43 | `sub\.g<U+0131>t\eileen-probe` | `<job>\sub\.g<U+0131>t\eileen-probe` | no: path not found | - | no | - | JobFolder |
| r44 | `sub\.g<U+0130>t\eileen-probe` | `<job>\sub\.g<U+0130>t\eileen-probe` | no: path not found | - | no | - | JobFolder |
| r45 | `sub\<U+FF0E><U+FF47><U+FF49><U+FF54>\eileen-probe` | `<job>\sub\<U+FF0E><U+FF47><U+FF49><U+FF54>\eileen-probe` | no: path not found | - | no | - | JobFolder |
| r46 | `sub\.git<U+00A0>\eileen-probe` | `<job>\sub\.git<U+00A0>\eileen-probe` | no: path not found | - | no | - | JobFolder |
| r47 | `sub\.git<U+3000>\eileen-probe` | `<job>\sub\.git<U+3000>\eileen-probe` | no: path not found | - | no | - | JobFolder |
| r48 | `sub/.git/eileen-probe` | `<job>\sub\.git\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | yes | 2, 4, 5, 6 | GitInternals |
| r49 | `sub\.git/eileen-probe` | `<job>\sub\.git\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | yes | 2, 4, 5, 6 | GitInternals |
| r50 | `sub//.git//eileen-probe` | `<job>\sub\.git\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | yes | 2, 4, 5, 6 | GitInternals |
| r51 | `sub\.git\.\eileen-probe` | `<job>\sub\.git\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | yes | 2, 4, 5, 6 | GitInternals |
| r52 | `sub\x\..\.git\eileen-probe` | `<job>\sub\.git\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | yes | 2, 4, 5, 6 | GitInternals |
| r53 | `sub\.git\hooks\..\eileen-probe` | `<job>\sub\.git\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | yes | 2, 4, 5, 6 | GitInternals |
| r54 | `.\sub\.git\eileen-probe` | `<job>\sub\.git\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | yes | 2, 4, 5, 6 | GitInternals |
| r55 | `C:sub\.git\eileen-probe` | `<job>\sub\.git\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | yes | 2, 4, 5, 6 | GitInternals |
| r56 | `<job-without-drive>\sub\.git\eileen-probe` | `<job>\sub\.git\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | yes | 2, 4, 5, 6 | GitInternals |
| r57 | `<job-in-git-bash-form>/sub/.git/eileen-probe` | `C:\c<job-without-drive>\sub\.git\eileen-probe` | no: path not found | - | yes | 2, 4 | Elsewhere, git |
| r58 | `alias\eileen-probe` | `<job>\alias\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | no | 4, 5, 6 | GitInternals |
| r59 | `alias` | `<job>\alias` | yes: `sub\.git` | `\\?\<job>\sub\.git` | no | 4, 6 | GitInternals |
| r60 | `sub2\.git\eileen-probe2` | `<job>\sub2\.git\eileen-probe2` | yes: `plain\eileen-probe2` | `\\?\<job>\plain\eileen-probe2` | yes | 2, 6 | GitInternals |
| r61 | `plain\eileen-probe2` | `<job>\plain\eileen-probe2` | yes: `plain\eileen-probe2` | `\\?\<job>\plain\eileen-probe2` | no | 6 | GitInternals |
| r62 | `ext\.git\config` | `<job>\ext\.git\config` | yes: `other\.git\config` | `\\?\<root>\other\.git\config` | yes | 2, 4 | Elsewhere, git |
| r63 | `..\outside\jobalias\sub\.git\eileen-probe` | `<root>\outside\jobalias\sub\.git\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | yes | 2, 4, 5, 6 | GitInternals |
| r64 | `hard.txt` | `<job>\hard.txt` | yes: `sub\.git\eileen-probe` | `\\?\<job>\hard.txt` | no | 5 | GitInternals |
| r65 | `..\other\.git\config` | `<root>\other\.git\config` | yes: `other\.git\config` | `\\?\<root>\other\.git\config` | yes | 2, 4 | Elsewhere, git |
| a00 | `\\?\<job>\sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | yes | 2, 4, 5, 6 | GitInternals |
| a01 | `\\?\<job>\sub\.GIT\eileen-probe` | `\\?\<job>\sub\.GIT\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | yes | 2, 4, 5, 6 | GitInternals |
| a02 | `\\?\<job>\sub\.git.\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | no: path not found | - | yes | 2, 4, 6 | GitInternals |
| a03 | `\\?\<job>\sub\GIT~1\eileen-probe` | `\\?\<job>\sub\GIT~1\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | yes | 2, 4, 5, 6 | GitInternals |
| a04 | `\\?\<job>\sub\.git::$INDEX_ALLOCATION\eileen-probe` | `\\?\<job>\sub\.git::$INDEX_ALLOCATION\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | yes | 2, 4, 5, 6 | GitInternals |
| a05 | `\\?\<job>\sub/.git/eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | no: invalid name | - | yes | 2, 4, 6 | GitInternals |
| a06 | `\\?\<job>\sub\x\..\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | no: path not found | - | yes | 2, 4, 6 | GitInternals |
| a07 | `\\.\<job>\sub\.git\eileen-probe` | `\\.\<job>\sub\.git\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | yes | 2, 4, 5, 6 | GitInternals |
| a08 | `\\.\<job>\sub\.git.\eileen-probe` | `\\.\<job>\sub\.git\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | yes | 2, 4, 5, 6 | GitInternals |
| a09 | `\??\<job>\sub\.git\eileen-probe` | `C:\??\<job>\sub\.git\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | yes | 2, 4, 5, 6 | GitInternals |
| a10 | `\??\<job>\sub\.git.\eileen-probe` | `C:\??\<job>\sub\.git\eileen-probe` | no: path not found | - | yes | 2, 4, 6 | GitInternals |
| a11 | `\\?\GLOBALROOT\Device\HarddiskVolume3<job-without-drive>\sub\.git\eileen-probe` | `\\?\GLOBALROOT\Device\HarddiskVolume3<job-without-drive>\sub\.git\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | yes | 2, 4, 5, 6 | GitInternals |
| a12 | `\\?\Volume{<guid>}<job-without-drive>\sub\.git\eileen-probe` | `\\?\Volume{<guid>}<job-without-drive>\sub\.git\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\<job>\sub\.git\eileen-probe` | yes | 2, 4, 5, 6 | GitInternals |
| a13 | `\\localhost\C$<job-without-drive>\sub\.git\eileen-probe` | `\\localhost\C$<job-without-drive>\sub\.git\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\UNC\localhost\C$<job-without-drive>\sub\.git\eileen-probe` | yes | 7, 2, 4, 5, 6 | GitInternals |
| a14 | `\\?\UNC\localhost\C$<job-without-drive>\sub\.git\eileen-probe` | `\\?\UNC\localhost\C$<job-without-drive>\sub\.git\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\UNC\localhost\C$<job-without-drive>\sub\.git\eileen-probe` | yes | 7, 2, 4, 5, 6 | GitInternals |
| a15 | `\\127.0.0.1\C$<job-without-drive>\sub\.git\eileen-probe` | `\\127.0.0.1\C$<job-without-drive>\sub\.git\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\UNC\127.0.0.1\C$<job-without-drive>\sub\.git\eileen-probe` | yes | 7, 2, 4, 5, 6 | GitInternals |
| a16 | `\\.\UNC\localhost\C$<job-without-drive>\sub\.git\eileen-probe` | `\\.\UNC\localhost\C$<job-without-drive>\sub\.git\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\UNC\localhost\C$<job-without-drive>\sub\.git\eileen-probe` | yes | 7, 2, 4, 5, 6 | GitInternals |
| a17 | `\\<computer-name>\C$<job-without-drive>\sub\.git\eileen-probe` | `\\<computer-name>\C$<job-without-drive>\sub\.git\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\UNC\<computer-name>\C$<job-without-drive>\sub\.git\eileen-probe` | yes | 7, 2, 4, 5, 6 | GitInternals |
| a18 | `\\localhost\C$<job-without-drive>\alias\eileen-probe` | `\\localhost\C$<job-without-drive>\alias\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\UNC\localhost\C$<job-without-drive>\sub\.git\eileen-probe` | no | 7, 4, 5, 6 | GitInternals |
| a19 | `\\localhost\C$<job-without-drive>\sub\GIT~1\eileen-probe` | `\\localhost\C$<job-without-drive>\sub\GIT~1\eileen-probe` | yes: `sub\.git\eileen-probe` | `\\?\UNC\localhost\C$<job-without-drive>\sub\.git\eileen-probe` | yes | 7, 2, 4, 5, 6 | GitInternals |
| a20 | `\\localhost\C$<job-without-drive>\hard.txt` | `\\localhost\C$<job-without-drive>\hard.txt` | yes: `sub\.git\eileen-probe` | `\\?\UNC\localhost\C$<job-without-drive>\hard.txt` | no | 7, 5 | GitInternals |
| a21 | `\\localhost\C$<job-without-drive>\plain\eileen-probe2` | `\\localhost\C$<job-without-drive>\plain\eileen-probe2` | yes: `plain\eileen-probe2` | `\\?\UNC\localhost\C$<job-without-drive>\plain\eileen-probe2` | no | 7, 6 | GitInternals |

What the table shows, beyond the summary above:

- GetLongPathNameW expands short names but keeps junction names, trailing-dot spellings and stream suffixes, and fails for paths that do not exist. It is not a canonicaliser.
- GetFullPathNameW is a faithful model of ordinary paths only. For `\\?\` paths it strips the trailing dot, collapses `..` and turns `/` into `\`, none of which CreateFileW does (rows a02, a05 and a06). It reads `\??\C:\x` as the root-relative `C:\??\C:\x`, yet CreateFileW opens the NT path `C:\x` (row a09). So `\\?\` and `\??\` paths must be taken as written.
- The final path of a junction to `.git` (r58, r59), a junction from outside into the job (r63) and a loopback share through a junction (a18) all show `.git`. The final path of `sub2\.git\...` (r60) shows `plain\...`, and the hard link (r64) shows `hard.txt`.
- Git itself uses the junction: `git rev-parse` in `sub2` reports a work tree and `.git/hooks`, so a hook written to `plain\hooks` would run there.
- `ext\.git\config` (r62) is written inside the job folder but is another repository's configuration. Its class must come from the final path (`Elsewhere`, with `git_component`), not from the path as written, which would make it a tier 0 read.

### Paths that do not exist yet

Each spelling was created in its own empty folder, then the folder was listed.

| kind | spelling | result | name created | flagged by the rule |
|---|---|---|---|---|
| directory | `.git` | created | `.git` | yes |
| directory | `.git.` | created | `.git` | yes |
| directory | `.git..` | created | `.git` | yes |
| directory | `.git␣` | created | `.git` | yes |
| directory | `.git.␣.` | created | `.git` | yes |
| directory | `.GIT` | created | `.GIT` | yes |
| directory | `GIT~1` | created | `GIT~1` | yes |
| directory | `git~1.` | created | `git~1` | yes |
| directory | `.git::$INDEX_ALLOCATION` | created | `.git` | yes |
| directory | `.git:` | invalid name | - | yes |
| directory | `.git:$DATA` | error 267 | - | yes |
| directory | `.git::$DATA` | error 267 | - | yes |
| directory | `.git\` | created | `.git` | yes |
| directory | `.git␣\` | created | `.git␣` | yes |
| directory | `.git.\` | created | `.git` | yes |
| directory | `\\?\.git.` | created | `.git.` | yes |
| directory | `\\?\.git␣` | created | `.git␣` | yes |
| file | `.git` | created | `.git` | yes |
| file | `.git.` | created | `.git` | yes |
| file | `.git..` | created | `.git` | yes |
| file | `.git␣` | created | `.git` | yes |
| file | `.git.␣.` | created | `.git` | yes |
| file | `.GIT` | created | `.GIT` | yes |
| file | `GIT~1` | created | `GIT~1` | yes |
| file | `git~1.` | created | `git~1` | yes |
| file | `.git::$INDEX_ALLOCATION` | created | `.git` | yes |
| file | `.git:` | invalid name | - | yes |
| file | `.git:$DATA` | created | `.git` | yes |
| file | `.git::$DATA` | created | `.git` | yes |
| file | `\\?\.git.` | created | `.git.` | yes |
| file | `\\?\.git␣` | created | `.git␣` | yes |

`.GIT` is created with that case, so later opens of `.git` reach it. `GIT~1` and `git~1.` create folders git never reads. The two `\\?\` rows create `.git.` and `.git␣`, names that ordinary Win32 paths cannot open again (the `\\?\` prefix is needed to delete them).

### Every BMP code unit

Each of the 65,535 BMP code units was put in each position of `.git`, after it and before it, and the result opened. Only these reached the real `.git`:

- In place of `.` in `.git`: U+002E (dot).
- In place of `g`: U+0047 (G), U+0067 (g).
- In place of `i`: U+0049 (I), U+0069 (i).
- In place of `t`: U+0054 (T), U+0074 (t).
- After `.git` as the last component: U+0020 (space), U+002E (dot), U+002F (slash), U+005C (backslash).
- After `.git` as an inner component: U+002E (dot), U+002F (slash), U+005C (backslash).
- Before `.git`: U+002F (slash), U+005C (backslash).

So on this volume NTFS case folding is ASCII only for these letters (U+0131 and U+0130 do not fold to `i`), and only dots and spaces are stripped. The upcase table belongs to the volume, so another volume should be swept again.

### Shells

The same probe through the shells a worker uses. PowerShell columns are `Get-Content -LiteralPath`, `Get-Content -Path`, `Get-Content -Path -Force`, `[IO.File]::ReadAllText`, `Set-Content -LiteralPath`, `Set-Content -Path`, `Set-Content -Path -Force` and the `>` redirection; then `cmd /c type`, `cmd /c echo >`, and Git Bash reading with `$(< path)` and writing with `>`. "yes" means the real probe was read or rewritten. The last three columns are the recommended rule's class for a path in that shell.

| spelling | Get -LiteralPath | Get -Path | Get -Path -Force | .NET read | Set -LiteralPath | Set -Path | Set -Path -Force | `>` | cmd type | cmd echo | bash read | bash write | rule, PowerShell | rule, cmd | rule, Git Bash |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `sub\.git\eileen-probe` | yes | yes | yes | yes | yes | yes | yes | yes | yes | yes | yes | yes | GitInternals | GitInternals | GitInternals |
| `sub\.GIT\eileen-probe` | yes | yes | yes | yes | yes | yes | yes | yes | yes | yes | yes | yes | GitInternals | GitInternals | GitInternals |
| `sub\.git.\eileen-probe` | yes | yes | yes | yes | yes | yes | yes | yes | yes | yes | no | no | GitInternals | GitInternals | GitInternals |
| `sub\.git␣\eileen-probe` | no | no | no | no | no | no | no | no | no | no | no | no | GitInternals | GitInternals | GitInternals |
| `sub\.git.␣.\eileen-probe` | no | no | no | no | no | no | no | no | no | no | no | no | GitInternals | GitInternals | GitInternals |
| `sub\GIT~1\eileen-probe` | yes | yes | yes | yes | yes | yes | yes | yes | yes | yes | yes | yes | GitInternals | GitInternals | GitInternals |
| `sub\.git::$INDEX_ALLOCATION\eileen-probe` | no | no | no | no | no | no | no | no | yes | yes | no | no | GitInternals | GitInternals | GitInternals |
| `sub\.git\eileen-probe::$DATA` | no | no | no | no | no | no | no | no | no | yes | no | no | GitInternals | GitInternals | GitInternals |
| `sub\.git\eileen-probe.` | yes | yes | yes | yes | yes | yes | yes | yes | yes | yes | no | no | GitInternals | GitInternals | GitInternals |
| `sub/.git/eileen-probe` | yes | yes | yes | yes | yes | yes | yes | yes | no | yes | yes | yes | GitInternals | GitInternals | GitInternals |
| `alias\eileen-probe` | yes | yes | yes | yes | yes | yes | yes | yes | yes | yes | yes | yes | GitInternals | GitInternals | GitInternals |
| `hard.txt` | yes | yes | yes | yes | yes | yes | yes | yes | yes | yes | yes | yes | GitInternals | GitInternals | GitInternals |
| `sub\.gi[t]\eileen-probe` | no | no | yes | no | no | no | yes | no | no | no | yes | yes | Unknown | JobFolder | Unknown |
| `sub\.g?t\eileen-probe` | no | no | yes | no | no | no | yes | no | no | no | yes | yes | Unknown | Unknown | Unknown |
| `sub\.gi*\eileen-probe` | no | no | yes | no | no | no | yes | no | no | no | yes | yes | Unknown | Unknown | Unknown |
| `sub\<U+FF0E><U+FF47><U+FF49><U+FF54>\eileen-probe` | no | no | no | no | no | no | no | no | no | no | no | no | Unknown, git | Unknown, git | Unknown, git |
| `<job-in-git-bash-form>/alias/eileen-probe` | no | no | no | no | no | no | no | no | no | no | yes | yes | Elsewhere | Elsewhere | GitInternals |
| `<job-in-git-bash-form>/sub/.git/eileen-probe` | no | no | no | no | no | no | no | no | no | no | yes | yes | Elsewhere, git | Elsewhere, git | GitInternals |

- Windows PowerShell 5.1 refuses every stream spelling (its .NET Framework path check rejects `:`), but cmd.exe passes them to NTFS and reaches `.git`.
- PowerShell's wildcard expansion skips hidden items, and git marks `.git` hidden, so `[t]`, `?` and `*` reach it only with `-Force`. Without `-Force` the call failed, which is luck, not policy.
- Git Bash talks to NTFS through its own runtime. It keeps trailing dots (it wrote a separate file `eileen-probe.` inside `.git`) and stores `:` as U+F03A (it wrote `eileen-probe<U+F03A><U+F03A>$DATA`), so those spellings do not reach the original file. It does resolve case and short names, and it reads `/c/...` paths that a Win32 view of the string gets wrong.

### ANSI best fit

With code page 1252, WideCharToMultiByte maps these strings to ASCII, and CreateFileA on the result opens the real probe; CreateFileW on the original finds nothing:

- `sub\<U+FF0E><U+FF47><U+FF49><U+FF54>\eileen-probe`: CreateFileW finds nothing (path not found); the ANSI program receives `sub\.git\eileen-probe` and opens `sub\.git\eileen-probe`. Without step 0 the rule says `JobFolder`; with it, `Unknown`, flagged as git.
- `sub<U+FF3C>.git<U+FF3C>eileen-probe`: CreateFileW finds nothing (not found); the ANSI program receives `sub\.git\eileen-probe` and opens `sub\.git\eileen-probe`. Without step 0 the rule says `JobFolder`; with it, `Unknown`, flagged as git.
- `sub<U+2215>.git<U+2215>eileen-probe`: CreateFileW finds nothing (not found); the ANSI program receives `sub/.git/eileen-probe` and opens `sub\.git\eileen-probe`. Without step 0 the rule says `JobFolder`; with it, `Unknown`, flagged as git.
- `sub\.g<U+0131>t\eileen-probe`: CreateFileW finds nothing (path not found); the ANSI program receives `sub\.git\eileen-probe` and opens `sub\.git\eileen-probe`. Without step 0 the rule says `JobFolder`; with it, `Unknown`, flagged as git.

Code points that best-fit to a character that matters here:

- `.`: U+FF0E.
- `g`: U+011D, U+011F, U+0121, U+0123, U+01E5, U+01E7, U+0261, U+210A, U+FF47.
- `i`: U+0129, U+012B, U+012D, U+012F, U+0131, U+01D0, U+FF49.
- `t`: U+0163, U+0165, U+0167, U+01AB, U+03C4, U+FF54.
- `\`: U+2216, U+FF3C.
- `/`: U+2044, U+2215, U+FF0F.
- `:`: U+0589, U+2236, U+FF1A.
- `"`: U+02BA, U+030E, U+201F, U+2033, U+2036, U+FF02.
- a space: U+2000, U+2001, U+2002, U+2003, U+2004, U+2005, U+2006, U+2008, U+2009, U+200A, U+3000.

An ANSI program started from a shell command receives its whole command line through the same mapping, so a fullwidth quote (U+FF02) can also split arguments the tokenizer saw as one. On a PC whose system code page is UTF-8 (65001) there is no best-fit change.

## What the classifier must do

The reference implementation is `classify` in the script; it flags every case above that reaches a git directory. In order:

0. **Shell paths first.** For a path in a shell command: a glob character of that shell (`*`, `?`, `[`, `]` in PowerShell and Git Bash, plus `{`, `}` in Git Bash; `*`, `?` in cmd) makes the path `Unknown`. If the active code page's best-fit mapping changes the command line, the command is unparseable. In Git Bash, `/c/x` is `C:\x`, `//host/share` is UNC, and other absolute POSIX paths are translated through Git's mount table (`/tmp` is the user's temp folder, `/` Git's install folder) or are `Unknown`.
1. **Normalise the string as CreateFileW will.** Join a relative path to the worker's tracked folder and call GetFullPathNameW on the result, which applies Windows' own rules: `/` becomes `\`, `.` and `..` collapse, the last component loses all trailing dots and spaces, and an inner component that ends in a single dot loses it. A drive-relative path on a drive other than the working folder's (`D:x`) is `Unknown`, because it depends on the worker process's hidden per-drive folder. A `\\?\` or `\??\` path is taken as written.
2. **Apply the component rule to every component of that string.** The rule is git's: `.git` in any ASCII case, then only dots and spaces to the end of the component or a colon. Add both short forms: `GIT~<n>` for any `n`, and `GI` with four hex digits and `~<n>`.
3. **Open the deepest existing prefix** with CreateFileW (no access rights, all sharing, `FILE_FLAG_BACKUP_SEMANTICS`, reparse points followed) and read GetFinalPathNameByHandleW with `FILE_NAME_NORMALIZED | VOLUME_NAME_DOS`. Append the components that do not exist yet. A volume with no drive letter gives no DOS name and the path is `Elsewhere`.
4. **Apply the component rule to every component of the final path.** The class of the path comes from this final path.
5. **Classify every name of a file with more than one hard link** (the link count from GetFileInformationByHandle, the names from FindFirstFileNameW and FindNextFileNameW) and keep the strictest result. This applies to every class: a hard link to a credential file is a credential path.
6. **Treat an existing ancestor that holds `HEAD`, `objects\` and `refs\` as a `.git` component.** That is git's own test for a git directory. It catches the target of a junction named `.git`, the target of a `gitdir:` pointer file, and a bare repository.
7. **Map this machine's own shares.** A UNC path whose server is this machine (`localhost`, any `127.x` address, `::1`, the computer's name, or its DNS name) to an administrative drive share `X$` is the local path `X:\...` and is classified from step 1 again. Any other share on this machine is `Unknown`. Any other UNC path is `Elsewhere`, with steps 2 and 4 still setting `git_component`.

`git_component` is set by step 2, 4, 5 or 6. `GitInternals` is a flagged path whose final path lies inside the job's folders, or a path inside the job's git and common directories.

Two limits remain. The check runs when the call is classified and the tool opens the path a moment later, so a concurrent command that swaps a junction or creates `.git` in between wins the race; such commands are already tier 2, and the short-name forms in step 2 cover the most likely race. And symbolic links were not created on this PC; CreateFileW follows them like junctions, so steps 3 and 4 should cover them, but WP2 must test them on a PC with Developer Mode on.

## Changes to BUILD.md

This branch amends BUILD.md as follows (revision log entry of 2026-10-21):

- Section 2.12, worker built-in tools: how the final path is read (by handle, from the deepest existing part), `\\?\` and `\??\` taken as written, every hard-link name classified, and this machine's own UNC shares mapped or `Unknown`.
- Section 2.12, the `GitInternals` row: the rule runs on the path as written and on its final path, knows both short-name forms, and counts a folder that holds a git directory.
- Section 2.12, the class order: the class is decided on the final path, with the junction to another repository (r62) as the example.
- Section 2.12, shell commands: globs include `[...]` and Git Bash's `{a,b}`, Git Bash paths are translated, drive-relative paths on another drive are unknown, and a command line the best-fit mapping changes is unparseable.
- Section 6.4, WP2's builds and acceptance 10: the new cases, with this note's table and script as the reference.
- Section 7: the carried question is marked settled.

D71 stands: this note changes how the rule is applied, not what it decides.

## The test script

Run it on Windows with `python -I dotgit_spellings.py --sweep --shells`. It exits with status 1 if the reference classifier misses a case. `--root` picks the parent folder, `--json` writes every field, and `--keep` leaves the fixture for inspection.

```python
#!/usr/bin/env python3
"""How Windows resolves alternative spellings of `.git` (Eileen, WP2 acceptance 10).

Builds a fixture in a fresh temporary folder, opens every spelling through the Win32 API with
ctypes, and reports for each one whether it reaches the real `.git`, what GetFullPathNameW,
GetLongPathNameW and GetFinalPathNameByHandleW return, what git's own is_ntfs_dotgit says,
and what the reference classifier in this file decides. It changes no system setting and
writes nothing outside the fixture folder, which it removes unless --keep is given.

    python -I dotgit_spellings.py [--root DIR] [--json FILE] [--sweep] [--shells] [--keep]

--sweep tries every BMP code unit in and around `.git` (about 460,000 opens, under a minute).
--shells runs a subset through PowerShell 5.1, .NET, cmd.exe and Git Bash.
Needs Windows, an NTFS volume and Python 3.12 or newer; git is optional.
"""

from __future__ import annotations

import argparse
import ctypes
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from ctypes import wintypes as wt

if sys.platform != "win32":
    sys.exit("Windows only: this probes NTFS and the Win32 path layer.")

# --- Win32 through ctypes --------------------------------------------------------------------

K32 = ctypes.WinDLL("kernel32", use_last_error=True)
INVALID = ctypes.c_void_p(-1).value
SHARE_ALL = 0x7  # read, write, delete
OPEN_EXISTING, CREATE_NEW = 3, 1
BACKUP_SEMANTICS = 0x02000000  # needed to open a directory; reparse points are followed
GENERIC_WRITE = 0x40000000
DIRECTORY = 0x10
BUF = 32768


class FileInfo(ctypes.Structure):
    _fields_ = [
        ("attrs", wt.DWORD), ("created", wt.FILETIME), ("accessed", wt.FILETIME),
        ("written", wt.FILETIME), ("volume", wt.DWORD), ("size_hi", wt.DWORD),
        ("size_lo", wt.DWORD), ("links", wt.DWORD), ("index_hi", wt.DWORD),
        ("index_lo", wt.DWORD),
    ]


def _fn(name, restype, *argtypes):
    f = getattr(K32, name)
    f.restype, f.argtypes = restype, list(argtypes)
    return f


CreateFileW = _fn("CreateFileW", wt.HANDLE, wt.LPCWSTR, wt.DWORD, wt.DWORD, wt.LPVOID,
                  wt.DWORD, wt.DWORD, wt.HANDLE)
CreateFileA = _fn("CreateFileA", wt.HANDLE, wt.LPCSTR, wt.DWORD, wt.DWORD, wt.LPVOID,
                  wt.DWORD, wt.DWORD, wt.HANDLE)
CloseHandle = _fn("CloseHandle", wt.BOOL, wt.HANDLE)
GetFinalPathNameByHandleW = _fn("GetFinalPathNameByHandleW", wt.DWORD, wt.HANDLE, wt.LPWSTR,
                                wt.DWORD, wt.DWORD)
GetFileInformationByHandle = _fn("GetFileInformationByHandle", wt.BOOL, wt.HANDLE,
                                 ctypes.POINTER(FileInfo))
GetFullPathNameW = _fn("GetFullPathNameW", wt.DWORD, wt.LPCWSTR, wt.DWORD, wt.LPWSTR, wt.LPVOID)
GetLongPathNameW = _fn("GetLongPathNameW", wt.DWORD, wt.LPCWSTR, wt.LPWSTR, wt.DWORD)
GetShortPathNameW = _fn("GetShortPathNameW", wt.DWORD, wt.LPCWSTR, wt.LPWSTR, wt.DWORD)
CreateDirectoryW = _fn("CreateDirectoryW", wt.BOOL, wt.LPCWSTR, wt.LPVOID)
CreateHardLinkW = _fn("CreateHardLinkW", wt.BOOL, wt.LPCWSTR, wt.LPCWSTR, wt.LPVOID)
CreateSymbolicLinkW = _fn("CreateSymbolicLinkW", ctypes.c_ubyte, wt.LPCWSTR, wt.LPCWSTR, wt.DWORD)
FindFirstFileNameW = _fn("FindFirstFileNameW", wt.HANDLE, wt.LPCWSTR, wt.DWORD,
                         ctypes.POINTER(wt.DWORD), wt.LPWSTR)
FindNextFileNameW = _fn("FindNextFileNameW", wt.BOOL, wt.HANDLE, ctypes.POINTER(wt.DWORD),
                        wt.LPWSTR)
FindClose = _fn("FindClose", wt.BOOL, wt.HANDLE)
GetVolumeNameForVolumeMountPointW = _fn("GetVolumeNameForVolumeMountPointW", wt.BOOL,
                                        wt.LPCWSTR, wt.LPWSTR, wt.DWORD)
QueryDosDeviceW = _fn("QueryDosDeviceW", wt.DWORD, wt.LPCWSTR, wt.LPWSTR, wt.DWORD)
WideCharToMultiByte = _fn("WideCharToMultiByte", ctypes.c_int, wt.UINT, wt.DWORD, wt.LPCWSTR,
                          ctypes.c_int, wt.LPSTR, ctypes.c_int, wt.LPCSTR, ctypes.POINTER(wt.BOOL))
GetACP = _fn("GetACP", wt.UINT)

ERRORS = {2: "not found", 3: "path not found", 5: "access denied", 53: "network path not found",
          67: "network name not found", 80: "exists", 87: "invalid parameter",
          123: "invalid name", 161: "bad path name", 183: "exists", 1314: "privilege not held"}


def err_text(code: int) -> str:
    return ERRORS.get(code, f"error {code}")


def _string_call(f, arg: str) -> str | None:
    buf = ctypes.create_unicode_buffer(BUF)
    return buf.value if f(arg, buf, BUF) else None


def full_path(p: str) -> str | None:
    buf = ctypes.create_unicode_buffer(BUF)
    return buf.value if GetFullPathNameW(p, BUF, buf, None) else None


def long_path(p: str) -> str | None:
    return _string_call(GetLongPathNameW, p)


def short_path(p: str) -> str | None:
    return _string_call(GetShortPathNameW, p)


def open_handle(p: str, access: int = 0, disposition: int = OPEN_EXISTING):
    h = CreateFileW(p, access, SHARE_ALL, None, disposition, BACKUP_SEMANTICS, None)
    if h in (None, INVALID):
        return None, ctypes.get_last_error()
    return h, 0


def handle_facts(h) -> dict:
    fi = FileInfo()
    GetFileInformationByHandle(h, ctypes.byref(fi))
    buf = ctypes.create_unicode_buffer(BUF)
    n = GetFinalPathNameByHandleW(h, buf, BUF, 0)  # FILE_NAME_NORMALIZED | VOLUME_NAME_DOS
    return {"id": (fi.volume, fi.index_hi << 32 | fi.index_lo), "links": fi.links,
            "dir": bool(fi.attrs & DIRECTORY), "final": buf.value if n else None}


def probe(p: str) -> dict:
    h, err = open_handle(p)
    if h is None:
        return {"err": err}
    try:
        return {"err": 0, **handle_facts(h)}
    finally:
        CloseHandle(h)


def link_names(p: str, drive: str) -> list[str]:
    """Every hard-link name of an existing file, as drive paths."""
    size, buf = wt.DWORD(BUF), ctypes.create_unicode_buffer(BUF)
    h = FindFirstFileNameW(p, 0, ctypes.byref(size), buf)
    if h in (None, INVALID):
        return []
    names = [drive + buf.value]
    try:
        while True:
            size.value = BUF
            if not FindNextFileNameW(h, ctypes.byref(size), buf):
                return names
            names.append(drive + buf.value)
    finally:
        FindClose(h)


def best_fit(s: str) -> bytes:
    """What an ANSI program receives for `s` in the active code page (best-fit mapping on)."""
    acp = GetACP()
    n = WideCharToMultiByte(acp, 0, s, -1, None, 0, None, None)
    buf = ctypes.create_string_buffer(n)
    WideCharToMultiByte(acp, 0, s, -1, buf, n, None, None)
    return buf.value


# --- git's rule and the reference classifier -------------------------------------------------


def _ieq(text: str, lower: str) -> bool:
    """ASCII-only case-insensitive equality, the way git compares (`lower` is lower case)."""
    return len(text) == len(lower) and all(
        a == b or ("a" <= b <= "z" and a == b.upper()) for a, b in zip(text, lower))


def _dots_spaces_then_end(rest: str) -> bool:
    for c in rest:
        if c == ":":
            return True  # any alternate-data-stream suffix
        if c not in ". ":
            return False
    return True


def git_is_ntfs_dotgit(name: str) -> bool:
    """A port of git's is_ntfs_dotgit (path.c) for one component: `.git` or `git~1`, any
    case, then only dots and spaces up to the end or a colon."""
    if _ieq(name[:4], ".git"):
        return _dots_spaces_then_end(name[4:])
    if _ieq(name[:4], "git~") and name[4:5] == "1":
        return _dots_spaces_then_end(name[5:])
    return False


# NTFS gives `.git` the short name GIT~1 to GIT~4, then a hashed GI<4 hex>~<n> (seen: GI2837~1).
SHORT_GIT = re.compile(r"(?i)(?:git~[1-9][0-9]*|gi[0-9a-f]{4}~[1-9][0-9]*)")


def dotgit_component(name: str) -> bool:
    """The component rule recommended for Eileen: git's rule, with both short-name forms."""
    if git_is_ntfs_dotgit(name):
        return True
    m = SHORT_GIT.match(name)
    return bool(m) and _dots_spaces_then_end(name[m.end():])


def loopback_unc(path: str) -> bool:
    """A UNC path whose server is this machine reaches every local path (C$ and other shares)."""
    m = re.match(r"(?i)^\\\\(?:[?.]\\UNC\\)?([^\\]+)\\", path)
    if not m or path.startswith(("\\\\?\\", "\\\\.\\")) and not re.match(r"(?i)^\\\\[?.]\\UNC\\",
                                                                         path):
        return False
    host = m.group(1).lower().rstrip(".")
    me = os.environ.get("COMPUTERNAME", "").lower()
    return (host in ("localhost", "::1", "0.0.0.0", me) or host.startswith("127.")
            or host.endswith(".ipv6-literal.net") or (me and host.startswith(me + ".")))


ADMIN_SHARE = re.compile(r"(?i)^(?:\\\\[?.]\\UNC\\|\\\\)[^\\]+\\([a-z])\$(\\.*)?$")


def loopback_local(path: str) -> str | None:
    r"""\\localhost\C$\x and its other spellings name C:\x; None for anything else."""
    m = ADMIN_SHARE.match(path)
    if m and loopback_unc(path):
        return m.group(1).upper() + ":" + (m.group(2) or "\\")
    return None


def components(path: str) -> list[str]:
    return [c for c in re.split(r"[\\/]+", path) if c]


def strip_prefix(final: str) -> str:
    if final.startswith("\\\\?\\UNC\\"):
        return "\\\\" + final[8:]
    return final[4:] if final.startswith("\\\\?\\") else final


ROOT = re.compile(r"""(?ix)^(
    \\\\[?.]\\UNC\\[^\\]+\\[^\\]+       # \\?\UNC\server\share
  | \\\\[?.]\\GLOBALROOT\\Device\\[^\\]+ # \\?\GLOBALROOT\Device\HarddiskVolumeN
  | \\\\[?.]\\Volume\{[^}]+\}           # \\?\Volume{guid}
  | (?:\\\\[?.]\\|\\\?\?\\)[A-Z]:       # \\?\C:, \\.\C:, \??\C:
  | \\\\[^\\]+\\[^\\]+                  # \\server\share
  | [A-Z]:                              # C:
)""")


def path_root(path: str) -> str:
    m = ROOT.match(path)
    return m.group(1) + "\\" if m else ""


def _split_last(path: str, root: str):
    """(parent, name), or (None, None) once only the root is left."""
    stripped = path.rstrip("\\")
    if len(stripped) <= len(root.rstrip("\\")) or "\\" not in stripped:
        return None, None
    parent, _, name = stripped.rpartition("\\")
    return (root if len(parent) < len(root) else parent), name


def looks_like_git_dir(d: str) -> bool:
    """git's own test for a git directory: HEAD, objects/ and refs/."""
    return (os.path.isfile(os.path.join(d, "HEAD")) and os.path.isdir(os.path.join(d, "objects"))
            and os.path.isdir(os.path.join(d, "refs")))


def is_under(path: str, root: str) -> bool:
    p, r = components(path.upper()), components(root.upper())
    return p[:len(r)] == r


def msys_to_windows(path: str) -> str | None:
    """Git Bash's own path forms: /c/x is C:\\x, //host/share is UNC, other mounts unknown."""
    m = re.match(r"^/(?:cygdrive/)?([a-zA-Z])(?:/(.*))?$", path)
    if m:
        return m.group(1).upper() + ":\\" + (m.group(2) or "").replace("/", "\\")
    if path.startswith("//"):
        return "\\\\" + path[2:].replace("/", "\\")
    return None if path.startswith("/") else path


GLOB = {"bash": "*?[]{}", "powershell": "*?[]", "cmd": "*?"}


def unknown(norm: str, git: bool = False) -> dict:
    return {"norm": norm, "canonical": None, "string_git": git, "canonical_git": False,
            "link_git": False, "gitdir_ancestor": False, "git": git, "class": "Unknown"}


def classify(path: str, job_root: str, shell: str | None = None) -> dict:
    """The rule this note recommends (see its last section), as a reference implementation.

    0. For a shell command's path (shell = "bash", "powershell" or "cmd"): a glob character
       of that shell makes it Unknown; if the active code page's best-fit mapping changes
       the string, it is Unknown (an ANSI program would open the mapped string); in Git
       Bash, /c/... is C:\\... and any other absolute POSIX path is Unknown.
    1. Normalise the string the way CreateFileW will: GetFullPathNameW, except for \\\\?\\ and
       \\??\\ paths, which Windows passes through unchanged.
    2. Apply the component rule to every component of that string.
    3. Open the deepest existing prefix by handle (following junctions and symbolic links,
       resolving 8.3 names and streams) and take GetFinalPathNameByHandleW; append the rest.
    4. Apply the component rule to every component of that canonical path.
    5. For an existing file with more than one hard link, apply it to every name.
    6. Treat any existing ancestor that looks like a git directory as git internals.
    7. A UNC path to this machine's administrative drive share (\\\\localhost\\C$\\x) is
       classified as the local path it names (C:\\x); any other share on this machine is
       Unknown; any other UNC path is Elsewhere.
    The class comes from the canonical path; the git flag from any of 2, 4, 5 or 6.
    """
    if shell:
        if any(ch in path for ch in GLOB[shell]):
            return unknown(path, any(dotgit_component(c) for c in components(path)))
        mapped = best_fit(path).decode("mbcs", "replace")
        if mapped != path:
            a, b = classify(path, job_root), classify(mapped, job_root)
            return {**unknown(mapped), "git": a["git"] or b["git"]}
        if shell == "bash":
            translated = msys_to_windows(path)
            if translated is None:
                return unknown(path, any(dotgit_component(c) for c in components(path)))
            path = translated
    local = loopback_local(path)
    if local:
        path = local
    elif loopback_unc(path):
        return unknown(path, any(dotgit_component(c) for c in components(path)))
    literal = path.startswith(("\\\\?\\", "\\??\\"))
    norm = path if literal else (full_path(path) or path)
    string_git = any(dotgit_component(c) for c in components(norm))

    head, tail, facts, root = norm, [], None, path_root(norm)
    while head:
        h, _ = open_handle(head)
        if h is not None:
            try:
                facts = handle_facts(h)
            finally:
                CloseHandle(h)
            break
        head, name = _split_last(head, root)
        if name is None:
            break
        tail.insert(0, name)

    if facts is None or facts["final"] is None:
        return {"norm": norm, "canonical": None, "string_git": string_git, "canonical_git": False,
                "link_git": False, "gitdir_ancestor": False, "git": string_git,
                "class": "Unknown"}
    canonical = strip_prefix(facts["final"]) + "".join("\\" + t for t in tail)
    if loopback_unc(canonical):  # a link that leads back to this machine over SMB
        local = loopback_local(canonical)
        if not local or local == path:
            return unknown(canonical, string_git)
        again = classify(local, job_root)
        return {**again, "git": again["git"] or string_git}
    canonical_git = any(dotgit_component(c) for c in components(canonical))

    link_git = False
    if not tail and not facts["dir"] and facts["links"] > 1:
        for n in link_names(canonical, canonical[:2]):
            link_git = link_git or any(dotgit_component(c) for c in components(n))

    gitdir_ancestor = False
    if is_under(canonical, job_root):
        d = canonical if (not tail and facts["dir"]) else os.path.dirname(canonical)
        while is_under(d, job_root) and not gitdir_ancestor:
            gitdir_ancestor = os.path.isdir(d) and looks_like_git_dir(d)
            parent = os.path.dirname(d)
            if parent == d:
                break
            d = parent

    git = string_git or canonical_git or link_git or gitdir_ancestor
    unc = canonical.startswith("\\\\")
    inside = not unc and is_under(canonical, job_root)
    cls = ("GitInternals" if git else "JobFolder") if inside else (
        "Elsewhere, git" if git else "Elsewhere")
    return {"norm": norm, "canonical": canonical, "string_git": string_git,
            "canonical_git": canonical_git, "link_git": link_git,
            "gitdir_ancestor": gitdir_ancestor, "git": git, "class": cls}


# --- the fixture -----------------------------------------------------------------------------

TOKEN = "eileen-dotgit-probe-token"


def write_text(path: str, text: str) -> None:
    with open(path, "w", encoding="utf-8", newline="") as f:
        f.write(text)


def junction(link: str, target: str) -> bool:
    r = subprocess.run(["cmd", "/d", "/c", "mklink", "/J", link, target], capture_output=True)
    return r.returncode == 0


def build_fixture(root: str) -> dict:
    fx: dict = {"root": root, "notes": []}
    job = fx["job"] = os.path.join(root, "job")
    sub = fx["sub"] = os.path.join(job, "sub")
    os.makedirs(sub)
    git = shutil.which("git")
    fx["notes"].append(f"git on PATH: {'yes' if git else 'no, folders made by hand'}")

    def init(path: str) -> None:
        if git:
            subprocess.run([git, "-c", "init.defaultBranch=main", "init", "-q", path],
                           check=True, capture_output=True)
            return
        for d in ("objects", "refs", "hooks"):
            os.makedirs(os.path.join(path, ".git", d))
        write_text(os.path.join(path, ".git", "HEAD"), "ref: refs/heads/main\n")
        write_text(os.path.join(path, ".git", "config"), "[core]\n")

    init(job)
    init(sub)
    write_text(os.path.join(sub, ".git", "eileen-probe"), TOKEN)
    # a worktree's pointer file
    os.makedirs(os.path.join(job, "wt"))
    write_text(os.path.join(job, "wt", ".git"), "gitdir: ../.git/worktrees/wt\n")
    # sub2's .git is a junction to an ordinary-looking folder, plain, that holds a git directory
    init(os.path.join(job, "sub2tmp"))
    os.rename(os.path.join(job, "sub2tmp", ".git"), os.path.join(job, "plain"))
    os.rmdir(os.path.join(job, "sub2tmp"))
    os.makedirs(os.path.join(job, "sub2"))
    fx["junction sub2\\.git -> plain"] = junction(os.path.join(job, "sub2", ".git"),
                                                   os.path.join(job, "plain"))
    write_text(os.path.join(job, "plain", "eileen-probe2"), TOKEN)
    if git:
        r = subprocess.run([git, "-c", "core.fsmonitor=false", "-C", os.path.join(job, "sub2"),
                            "rev-parse", "--is-inside-work-tree", "--git-path", "hooks"],
                           capture_output=True, text=True)
        fx["notes"].append("git in sub2 (whose .git is a junction to plain): "
                           + " ".join(r.stdout.split()) if r.returncode == 0 else "git failed")
    # junctions and links
    fx["junction alias -> sub\\.git"] = junction(os.path.join(job, "alias"),
                                                  os.path.join(sub, ".git"))
    init(os.path.join(root, "other"))
    fx["junction ext -> ..\\other"] = junction(os.path.join(job, "ext"),
                                               os.path.join(root, "other"))
    os.makedirs(os.path.join(root, "outside"))
    fx["junction ..\\outside\\jobalias -> job"] = junction(
        os.path.join(root, "outside", "jobalias"), job)
    fx["hard link hard.txt -> sub\\.git\\eileen-probe"] = bool(CreateHardLinkW(
        os.path.join(job, "hard.txt"), os.path.join(sub, ".git", "eileen-probe"), None))
    ok = CreateSymbolicLinkW(os.path.join(job, "cfglink"),
                             os.path.join(sub, ".git", "eileen-probe"), 0x2)
    fx["file symlink cfglink"] = bool(ok) or err_text(ctypes.get_last_error())
    ok = CreateSymbolicLinkW(os.path.join(job, "dirlink"), os.path.join(sub, ".git"), 0x3)
    fx["directory symlink dirlink"] = bool(ok) or err_text(ctypes.get_last_error())

    # short names: plain, after one literal git~1, and after git~1 to git~4
    fx["short name of sub\\.git"] = os.path.basename(short_path(os.path.join(sub, ".git")) or "?")
    for folder, n, label in (("collide1", 1, "after git~1"), ("collide4", 4, "after git~1 to git~4"),
                             ("collide4b", 4, "after git~1 to git~4, second folder")):
        d = os.path.join(root, folder)
        os.makedirs(d)
        for i in range(1, n + 1):
            write_text(os.path.join(d, f"git~{i}"), "")
        os.makedirs(os.path.join(d, ".git"))
        fx[f"short name of .git {label}"] = os.path.basename(
            short_path(os.path.join(d, ".git")) or "?")
    fx["short name of a long file name"] = os.path.basename(
        short_path(os.path.join(sub, ".git", "eileen-probe")) or "?")

    known = {}
    for label, rel in [("sub\\.git", "sub\\.git"), ("sub\\.git\\eileen-probe",
                        "sub\\.git\\eileen-probe"), ("job\\.git", ".git"),
                       ("plain (= sub2\\.git)", "plain"), ("plain\\eileen-probe2",
                        "plain\\eileen-probe2"), ("wt\\.git (pointer file)", "wt\\.git"),
                       ("other\\.git", "..\\other\\.git"),
                       ("other\\.git\\config", "..\\other\\.git\\config"),
                       ("collide1\\.git", "..\\collide1\\.git"),
                       ("collide4\\.git", "..\\collide4\\.git")]:
        known[probe(os.path.normpath(os.path.join(job, rel)))["id"]] = label
    fx["known"] = known
    fx["git_dirs"] = {"sub\\.git", "job\\.git", "plain (= sub2\\.git)", "other\\.git",
                      "collide1\\.git", "collide4\\.git"}
    return fx


# --- the cases -------------------------------------------------------------------------------


def cases(fx: dict) -> list[tuple[str, str, str]]:
    sub = fx["sub"]
    s_nodrive, j_nodrive = sub[2:], fx["job"][2:]
    dev = ctypes.create_unicode_buffer(BUF)
    QueryDosDeviceW(sub[:2], dev, BUF)
    vol = ctypes.create_unicode_buffer(BUF)
    GetVolumeNameForVolumeMountPointW(sub[:3], vol, BUF)
    P = "eileen-probe"
    rel = [
        ("baseline", "sub\\.git\\" + P), ("baseline", "sub\\.git"),
        ("case", "sub\\.GIT\\" + P), ("case", "sub\\.Git\\" + P), ("case", "SUB\\.gIT\\EILEEN-PROBE"),
        ("trailing, inner", "sub\\.git.\\" + P), ("trailing, inner", "sub\\.git..\\" + P),
        ("trailing, inner", "sub\\.git...\\" + P), ("trailing, inner", "sub\\.git \\" + P),
        ("trailing, inner", "sub\\.git. .\\" + P), ("trailing, inner", "sub\\.git . . .\\" + P),
        ("trailing, last", "sub\\.git."), ("trailing, last", "sub\\.git "),
        ("trailing, last", "sub\\.git. . ."), ("trailing, last", "sub\\.git\\" + P + "."),
        ("trailing, last", "sub\\.git\\" + P + "  "), ("trailing, last", "sub\\.git \\"),
        ("leading", "sub\\ .git\\" + P),
        ("8.3", "sub\\GIT~1\\" + P), ("8.3", "sub\\git~1\\" + P), ("8.3", "sub\\GIT~1.\\" + P),
        ("8.3", "sub\\GIT~1 \\" + P), ("8.3", "sub\\GIT~2\\" + P), ("8.3", "sub\\.git\\EILEEN~1"),
        ("8.3", "..\\collide1\\GIT~2"), ("8.3", "..\\collide4\\" + fx[
            "short name of .git after git~1 to git~4"]),
        ("stream", "sub\\.git::$INDEX_ALLOCATION\\" + P),
        ("stream", "sub\\.git:$I30:$INDEX_ALLOCATION\\" + P),
        ("stream", "sub\\.GIT::$index_allocation\\" + P), ("stream", "sub\\.git::$INDEX_ALLOCATION"),
        ("stream", "sub\\.git:$DATA\\" + P), ("stream", "sub\\.git::$DATA\\" + P),
        ("stream", "sub\\.git:\\" + P), ("stream", "sub\\.git:"),
        ("stream", "sub\\.git\\" + P + "::$DATA"), ("stream", "sub\\.git\\" + P + ":$DATA"),
        ("stream", "wt\\.git::$DATA"), ("stream", "wt\\.git:$DATA"), ("stream", "wt\\.git."),
        ("stream", "wt\\GIT~1"),
        ("unicode", "sub\\.g‌it\\" + P), ("unicode", "sub\\﻿.git\\" + P),
        ("unicode", "sub\\.git​\\" + P), ("unicode", "sub\\.gıt\\" + P),
        ("unicode", "sub\\.gİt\\" + P), ("unicode", "sub\\．ｇｉｔ\\" + P),
        ("unicode", "sub\\.git \\" + P), ("unicode", "sub\\.git　\\" + P),
        ("slashes", "sub/.git/" + P), ("slashes", "sub\\.git/" + P),
        ("slashes", "sub//.git//" + P), ("slashes", "sub\\.git\\.\\" + P),
        ("slashes", "sub\\x\\..\\.git\\" + P), ("slashes", "sub\\.git\\hooks\\..\\" + P),
        ("slashes", ".\\sub\\.git\\" + P), ("relative", sub[:2] + "sub\\.git\\" + P),
        ("relative", s_nodrive + "\\.git\\" + P),
        ("relative", "/" + sub[0].lower() + s_nodrive.replace("\\", "/") + "/.git/" + P),
        ("link", "alias\\" + P), ("link", "alias"), ("link", "sub2\\.git\\eileen-probe2"),
        ("link", "plain\\eileen-probe2"), ("link", "ext\\.git\\config"),
        ("link", "..\\outside\\jobalias\\sub\\.git\\" + P), ("link", "hard.txt"),
        ("link", "..\\other\\.git\\config"),
    ]
    if fx["file symlink cfglink"] is True:
        rel += [("link", "cfglink"), ("link", "dirlink\\" + P)]
    absolute = [
        ("prefix", "\\\\?\\" + sub + "\\.git\\" + P), ("prefix", "\\\\?\\" + sub + "\\.GIT\\" + P),
        ("prefix", "\\\\?\\" + sub + "\\.git.\\" + P),
        ("prefix", "\\\\?\\" + sub + "\\GIT~1\\" + P),
        ("prefix", "\\\\?\\" + sub + "\\.git::$INDEX_ALLOCATION\\" + P),
        ("prefix", "\\\\?\\" + sub + "/.git/" + P),
        ("prefix", "\\\\?\\" + sub + "\\x\\..\\.git\\" + P),
        ("prefix", "\\\\.\\" + sub + "\\.git\\" + P), ("prefix", "\\\\.\\" + sub + "\\.git.\\" + P),
        ("prefix", "\\??\\" + sub + "\\.git\\" + P), ("prefix", "\\??\\" + sub + "\\.git.\\" + P),
        ("prefix", "\\\\?\\GLOBALROOT" + dev.value + s_nodrive + "\\.git\\" + P),
        ("prefix", vol.value.rstrip("\\") + s_nodrive + "\\.git\\" + P),
        ("prefix", "\\\\localhost\\" + sub[0] + "$" + s_nodrive + "\\.git\\" + P),
        ("prefix", "\\\\?\\UNC\\localhost\\" + sub[0] + "$" + s_nodrive + "\\.git\\" + P),
        ("prefix", "\\\\127.0.0.1\\" + sub[0] + "$" + s_nodrive + "\\.git\\" + P),
        ("prefix", "\\\\.\\UNC\\localhost\\" + sub[0] + "$" + s_nodrive + "\\.git\\" + P),
        ("prefix", "\\\\" + os.environ.get("COMPUTERNAME", "localhost") + "\\" + sub[0] + "$"
         + s_nodrive + "\\.git\\" + P),
        ("unc link", "\\\\localhost\\" + sub[0] + "$" + j_nodrive + "\\alias\\" + P),
        ("unc link", "\\\\localhost\\" + sub[0] + "$" + s_nodrive + "\\GIT~1\\" + P),
        ("unc link", "\\\\localhost\\" + sub[0] + "$" + j_nodrive + "\\hard.txt"),
        ("unc link", "\\\\localhost\\" + sub[0] + "$" + j_nodrive + "\\plain\\eileen-probe2"),
    ]
    return [(f"r{i:02d}", g, p) for i, (g, p) in enumerate(rel)] + [
        (f"a{i:02d}", g, p) for i, (g, p) in enumerate(absolute)]


def known_label(ident, fx) -> str | None:
    return fx["known"].get(ident)


def measure(fx: dict, cid: str, group: str, path: str) -> dict:
    row = {"id": cid, "group": group, "spelling": path, "full": full_path(path),
           "long": long_path(path), "git_rule": any(git_is_ntfs_dotgit(c)
                                                    for c in components(path))}
    pr = probe(path)
    if pr["err"]:
        row.update(opens=err_text(pr["err"]), reaches="-", final=None)
    else:
        row.update(opens="yes", final=pr["final"], links=pr["links"],
                   reaches=known_label(pr["id"], fx) or "another object")
    cl = classify(path, fx["job"])
    row["classifier"] = cl
    reaches_git = row["reaches"] in fx["known"].values()
    row["verdict"] = "ok" if not reaches_git or cl["git"] or cl["class"] == "Unknown" else "MISSED"

    # a write of a new file through the same directory spelling
    if "eileen-probe" in path and not path.endswith("eileen-probe2"):
        wpath = path.replace("eileen-probe", f"new-{cid}").replace("EILEEN-PROBE", f"NEW-{cid}")
        wcl = classify(wpath, fx["job"])  # before it exists
        h, err = open_handle(wpath, GENERIC_WRITE, CREATE_NEW)
        if h is None:
            row["write"] = {"lands": err_text(err), "classifier_git": wcl["git"], "verdict": "ok"}
        else:
            try:
                final = handle_facts(h)["final"]
            finally:
                CloseHandle(h)
            parent = probe(os.path.dirname(strip_prefix(final)))
            lands = known_label(parent.get("id"), fx) or "another folder"
            bad = lands in fx["git_dirs"] and not wcl["git"] and wcl["class"] != "Unknown"
            row["write"] = {"lands": lands, "name": os.path.basename(final),
                            "classifier_git": wcl["git"], "verdict": "MISSED" if bad else "ok"}
    return row


CREATE_SPELLINGS = [".git", ".git.", ".git..", ".git ", ".git. .", ".GIT", "GIT~1", "git~1.",
                    ".git::$INDEX_ALLOCATION", ".git:", ".git:$DATA", ".git::$DATA",
                    ".git\\", ".git \\", ".git.\\", "\\\\?\\.git.", "\\\\?\\.git "]


def creation_probes(fx: dict) -> list[dict]:
    """Paths that do not exist yet: what CreateDirectoryW and CreateFileW make of each."""
    out = []
    base = os.path.join(fx["job"], "mk")
    os.makedirs(base)
    n = 0
    for kind in ("directory", "file"):
        for s in CREATE_SPELLINGS:
            if kind == "file" and s.endswith("\\"):
                continue
            n += 1
            d = os.path.join(base, f"{n:02d}")
            os.makedirs(d)
            target = ("\\\\?\\" + d + "\\" + s[4:]) if s.startswith("\\\\?\\") else d + "\\" + s
            cl = classify(target, fx["job"])
            if kind == "directory":
                ok, err = bool(CreateDirectoryW(target, None)), ctypes.get_last_error()
            else:
                h, err = open_handle(target, GENERIC_WRITE, CREATE_NEW)
                ok = h is not None
                if ok:
                    CloseHandle(h)
            names = os.listdir(d)
            made = ".git" in names
            out.append({"kind": kind, "spelling": s, "result": "created" if ok else err_text(err),
                        "names": names, "made_dotgit": made, "classifier_git": cl["git"],
                        "verdict": "MISSED" if made and not cl["git"] else "ok"})
    return out


def sweep(fx: dict) -> dict:
    """Every BMP code unit in each position of `.git`, after it and before it."""
    sub = fx["sub"]
    ids = {v: k for k, v in fx["known"].items()}
    gid, pid = ids["sub\\.git"], ids["sub\\.git\\eileen-probe"]

    def hits(make, want) -> list[str]:
        out = []
        for cp in range(1, 0x10000):
            p = make(chr(cp))
            h = CreateFileW(p, 0, SHARE_ALL, None, OPEN_EXISTING, BACKUP_SEMANTICS, None)
            if h in (None, INVALID):
                continue
            try:
                fi = FileInfo()
                GetFileInformationByHandle(h, ctypes.byref(fi))
                ident = (fi.volume, fi.index_hi << 32 | fi.index_lo)
            finally:
                CloseHandle(h)
            if ident == want:
                out.append(f"U+{cp:04X}")
        return out

    res = {}
    for i in range(4):
        res[f"in place of {'.git'[i]!r} in .git"] = hits(
            lambda ch, i=i: sub + "\\" + ".git"[:i] + ch + ".git"[i + 1:], gid)
    res["after .git, last component"] = hits(lambda ch: sub + "\\.git" + ch, gid)
    res["after .git, inner component"] = hits(lambda ch: sub + "\\.git" + ch + "\\eileen-probe",
                                              pid)
    res["before .git"] = hits(lambda ch: sub + "\\" + ch + ".git", gid)
    return res


def ansi_probes(fx: dict) -> dict:
    """Unicode spellings an ANSI program would receive as ASCII through best-fit mapping."""
    job = fx["job"]
    out = {"acp": GetACP(), "cases": []}
    for s in ["sub\\．ｇｉｔ\\eileen-probe", "sub＼.git＼eileen-probe",
              "sub∕.git∕eileen-probe", "sub\\.gıt\\eileen-probe"]:
        p = job + "\\" + s
        wide = probe(p)
        ansi = best_fit(p)
        h = CreateFileA(ansi, 0, SHARE_ALL, None, OPEN_EXISTING, BACKUP_SEMANTICS, None)
        if h in (None, INVALID):
            a = err_text(ctypes.get_last_error())
        else:
            try:
                a = known_label(handle_facts(h)["id"], fx) or "another object"
            finally:
                CloseHandle(h)
        wide_cl, shell_cl = classify(p, job), classify(p, job, shell="cmd")
        out["cases"].append({"spelling": s, "wide": known_label(wide.get("id"), fx)
                             or err_text(wide["err"]), "ansi_bytes": ansi[len(job):].decode(
                                 "latin-1"), "ansi": a, "wide_rule": wide_cl["class"],
                             "shell_rule": shell_cl["class"] + (", git" if shell_cl["git"]
                                                                else "")})
    targets = {c: [] for c in ".git\\/:\" "}
    for cp in range(0x80, 0x10000):
        if 0xD800 <= cp <= 0xDFFF:
            continue
        b = best_fit(chr(cp))
        if len(b) == 1 and chr(b[0]) in targets:
            targets[chr(b[0])].append(f"U+{cp:04X}")
    out["best_fit_to_ascii"] = targets
    return out


# --- shells ----------------------------------------------------------------------------------

SHELL_CASES = ["sub\\.git\\eileen-probe", "sub\\.GIT\\eileen-probe", "sub\\.git.\\eileen-probe",
               "sub\\.git \\eileen-probe", "sub\\.git. .\\eileen-probe", "sub\\GIT~1\\eileen-probe",
               "sub\\.git::$INDEX_ALLOCATION\\eileen-probe", "sub\\.git\\eileen-probe::$DATA",
               "sub\\.git\\eileen-probe.", "sub/.git/eileen-probe", "alias\\eileen-probe",
               "hard.txt", "sub\\.gi[t]\\eileen-probe", "sub\\.g?t\\eileen-probe",
               "sub\\.gi*\\eileen-probe", "sub\\．ｇｉｔ\\eileen-probe"]

PS_SCRIPT = r"""
param([string]$Job, [string]$Real, [string]$Cases)
$ErrorActionPreference = 'Stop'
Set-Location -LiteralPath $Job
[Environment]::CurrentDirectory = $Job
$orig = [IO.File]::ReadAllText($Real)
$list = [IO.File]::ReadAllText($Cases, [Text.Encoding]::UTF8) | ConvertFrom-Json
$rows = foreach ($p in $list) {
  $r = [ordered]@{ spelling = $p }
  $r.get_literal = try { if ((Get-Content -LiteralPath $p -Raw) -eq $orig) { 'reaches' } else { 'other' } } catch { 'error' }
  $r.get_path = try { if ((Get-Content -Path $p -Raw) -eq $orig) { 'reaches' } else { 'other' } } catch { 'error' }
  $r.get_path_force = try { if ((Get-Content -Path $p -Raw -Force) -eq $orig) { 'reaches' } else { 'other' } } catch { 'error' }
  $r.dotnet_read = try { if ([IO.File]::ReadAllText($p) -eq $orig) { 'reaches' } else { 'other' } } catch { 'error' }
  foreach ($m in 'set_literal', 'set_path', 'set_path_force', 'redirect') {
    $tok = 'w-' + $m
    try {
      if ($m -eq 'set_literal') { Set-Content -LiteralPath $p -Value $tok -NoNewline }
      elseif ($m -eq 'set_path') { Set-Content -Path $p -Value $tok -NoNewline }
      elseif ($m -eq 'set_path_force') { Set-Content -Path $p -Value $tok -NoNewline -Force }
      else { $tok > $p }
      $r[$m] = if ([IO.File]::ReadAllText($Real).Contains($tok)) { 'reaches' } else { 'other' }
    } catch { $r[$m] = 'error' }
    [IO.File]::WriteAllText($Real, $orig)
  }
  [pscustomobject]$r
}
$rows | ConvertTo-Json -Compress
"""


def git_bash() -> str | None:
    git = shutil.which("git")
    if not git:
        return None
    r = subprocess.run([git, "--exec-path"], capture_output=True, text=True)
    root = os.path.normpath(os.path.join(r.stdout.strip(), "..", "..", ".."))
    bash = os.path.join(root, "usr", "bin", "bash.exe")
    return bash if os.path.isfile(bash) else None


def shell_probes(fx: dict) -> dict:
    job = fx["job"]
    real = os.path.join(fx["sub"], ".git", "eileen-probe")
    work = os.path.join(fx["root"], "shell")
    os.makedirs(work)
    msys = "/" + job[0].lower() + job[2:].replace("\\", "/")
    shell_cases = SHELL_CASES + [msys + "/alias/eileen-probe", msys + "/sub/.git/eileen-probe"]
    cases_file = os.path.join(work, "cases.json")
    with open(cases_file, "w", encoding="utf-8") as f:
        json.dump(shell_cases, f, ensure_ascii=False)
    ps1 = os.path.join(work, "probe.ps1")
    write_text(ps1, PS_SCRIPT)
    r = subprocess.run(["powershell", "-NoProfile", "-NonInteractive", "-File", ps1, "-Job", job,
                        "-Real", real, "-Cases", cases_file], capture_output=True)
    try:
        ps_rows = {row["spelling"]: row for row in json.loads(r.stdout.decode("utf-8", "replace"))}
    except ValueError:
        ps_rows = {}
        fx["notes"].append("PowerShell run failed: " + r.stderr.decode("utf-8", "replace")[:300])

    def real_has(tok: str) -> bool:
        with open(real, encoding="utf-8", errors="replace") as f:
            return tok in f.read()

    def restore() -> None:
        write_text(real, TOKEN)

    bash = git_bash()
    rows = []
    for p in shell_cases:
        row = {"spelling": p, **{k: v for k, v in ps_rows.get(p, {}).items() if k != "spelling"}}
        c = subprocess.run(f'cmd /d /c type "{p}"', cwd=job, capture_output=True)
        row["cmd_type"] = "reaches" if TOKEN.encode() in c.stdout else "error"
        subprocess.run(f'cmd /d /c echo w-cmd> "{p}"', cwd=job, capture_output=True)
        row["cmd_echo"] = "reaches" if real_has("w-cmd") else "other"
        restore()
        if bash:
            glob = any(ch in p for ch in "[?*")
            fwd = p.replace("\\", "/")
            # builtins only: a non-login Git Bash may not have /usr/bin on PATH
            read = (["-c", f"printf %s \"$(< {fwd})\""] if glob
                    else ["-c", 'printf %s "$(< "$1")"', "_", p])
            b = subprocess.run([bash, *read], cwd=job, capture_output=True)
            row["bash_read"] = "reaches" if TOKEN.encode() in b.stdout else "error"
            wr = (["-c", f"printf %s w-bash > {fwd}"] if glob
                  else ["-c", 'printf %s w-bash > "$1"', "_", p])
            subprocess.run([bash, *wr], cwd=job, capture_output=True)
            row["bash_write"] = "reaches" if real_has("w-bash") else "other"
            restore()
        verdict = "ok"
        for shell, cols in (("powershell", ("get_literal", "get_path", "get_path_force",
                                            "dotnet_read", "set_literal", "set_path",
                                            "set_path_force", "redirect")),
                            ("cmd", ("cmd_type", "cmd_echo")), ("bash", ("bash_read",
                                                                     "bash_write"))):
            cl = classify(p, job, shell=shell)
            flag = cl["git"] and cl["class"] not in ("GitInternals", "Elsewhere, git")
            row[f"rule ({shell})"] = cl["class"] + (", git" if flag else "")
            reached = any(row.get(c) == "reaches" for c in cols)
            if reached and not cl["git"] and cl["class"] != "Unknown":
                verdict = "MISSED"
        row["verdict"] = verdict
        rows.append(row)
    if bash:
        b = subprocess.run([bash, "-c", 'for f in sub/.gi{t,x}/eileen-probe; do [ -f "$f" ] && '
                            'printf %s "$(< "$f")"; done'], cwd=job, capture_output=True)
        fx["notes"].append("Git Bash brace expansion sub/.gi{t,x}/eileen-probe reads .git: "
                           + ("yes" if TOKEN.encode() in b.stdout else "no"))
    left = sorted(n for n in os.listdir("\\\\?\\" + os.path.join(fx["sub"], ".git"))
                  if n.lower().startswith("eileen-probe") and n != "eileen-probe")
    fx["notes"].append("extra probe names in sub\\.git after the shells ran: "
                       + (", ".join(repr(n) for n in left) or "none"))
    return {"bash": bool(bash), "rows": rows}


# --- output ----------------------------------------------------------------------------------


def sanitiser(root: str):
    subs = [(root, "<root>"), (root[2:], "<root-without-drive>"),
            ("/" + root[0].lower() + root[2:].replace("\\", "/"), "<root-in-git-bash-form>")]
    guid =re.compile(r"Volume\{[0-9a-fA-F-]+\}")
    name = os.environ.get("COMPUTERNAME")
    host = re.compile(r"(?i)\\" + re.escape(name) + r"\\") if name else None

    def clean(v):
        if isinstance(v, str):
            for a, b in subs:
                v = v.replace(a, b)
            if host:
                v = host.sub(r"\\<computer-name>\\", v)
            return guid.sub("Volume{<guid>}", v)
        if isinstance(v, dict):
            return {clean(k): clean(x) for k, x in v.items()}
        if isinstance(v, (list, tuple)):
            return [clean(x) for x in v]
        return v
    return clean


def shown(s) -> str:
    if s is None:
        return "-"
    s = "".join(c if 32 < ord(c) < 127 or c == " " else f"<U+{ord(c):04X}>" for c in s)
    return "`" + s.replace(" ", "␣").replace("|", "\\|") + "`"


def print_report(rep: dict) -> None:
    print("## Fixture\n")
    for k, v in rep["fixture"].items():
        print(f"- {k}: {v}")
    print("\n## Spellings\n")
    print("| id | group | spelling | GetFullPathNameW | GetLongPathNameW | opens | reaches |"
          " final path | git's rule | caught by | classifier | write lands | verdict |")
    print("|---|---|---|---|---|---|---|---|---|---|---|---|---|")
    steps = {"string_git": "2", "canonical_git": "4", "link_git": "5", "gitdir_ancestor": "6"}
    for r in rep["rows"]:
        cl, w = r["classifier"], r.get("write")
        wl = "-" if not w else f"{w['lands']}{' (MISSED)' if w['verdict'] != 'ok' else ''}"
        caught = ", ".join(v for k, v in steps.items() if cl[k]) or "-"
        print(f"| {r['id']} | {r['group']} | {shown(r['spelling'])} | {shown(r['full'])} | "
              f"{shown(r['long'])} | {r['opens']} | {r['reaches']} | {shown(r['final'])} | "
              f"{'yes' if r['git_rule'] else 'no'} | {caught} | {cl['class']} | {wl} | "
              f"{r['verdict']} |")
    print("\n## Paths that do not exist yet\n")
    print("| kind | spelling | result | names in the folder afterwards | made .git | classifier"
          " git | verdict |")
    print("|---|---|---|---|---|---|---|")
    for r in rep["create"]:
        print(f"| {r['kind']} | {shown(r['spelling'])} | {r['result']} | "
              f"{', '.join(shown(n) for n in r['names']) or '-'} | {r['made_dotgit']} | "
              f"{r['classifier_git']} | {r['verdict']} |")
    if "sweep" in rep:
        print("\n## BMP sweep (code units that reach the real .git)\n")
        for k, v in rep["sweep"].items():
            print(f"- {k}: {', '.join(v) or 'none'}")
    a = rep["ansi"]
    print(f"\n## ANSI best fit (active code page {a['acp']})\n")
    for c in a["cases"]:
        print(f"- {shown(c['spelling'])}: wide API {c['wide']}; ANSI program receives "
              f"{shown(c['ansi_bytes'])} and reaches {c['ansi']}; classifier without step 0: "
              f"{c['wide_rule']}; with step 0: {c['shell_rule']}")
    for k, v in a["best_fit_to_ascii"].items():
        print(f"- best-fits to {k!r}: {len(v)} code points, e.g. {', '.join(v[:8])}")
    if "shells" in rep:
        rows = rep["shells"]["rows"]
        keys = [k for k in rows[0] if k != "spelling"]
        print("\n## Shells\n")
        print("| spelling | " + " | ".join(keys) + " |")
        print("|---|" + "---|" * len(keys))
        for r in rows:
            print(f"| {shown(r['spelling'])} | " + " | ".join(str(r.get(k, '-')) for k in keys)
                  + " |")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--root", help="parent folder for the fixture (default: the temp folder)")
    ap.add_argument("--json", help="write the full results here")
    ap.add_argument("--sweep", action="store_true")
    ap.add_argument("--shells", action="store_true")
    ap.add_argument("--keep", action="store_true", help="leave the fixture in place")
    args = ap.parse_args()
    sys.stdout.reconfigure(encoding="utf-8")

    made =tempfile.mkdtemp(prefix="dotgit-", dir=args.root)
    h, _ = open_handle(made)
    root = strip_prefix(handle_facts(h)["final"])
    CloseHandle(h)
    os.chdir(root)
    try:
        fx = build_fixture(root)
        os.chdir(fx["job"])
        rows = [measure(fx, cid, g, p) for cid, g, p in cases(fx)]
        rep = {"fixture": {k: v for k, v in fx.items()
                           if k not in ("known", "git_dirs", "root", "job", "sub")},
               "rows": rows, "create": creation_probes(fx), "ansi": ansi_probes(fx)}
        if args.sweep:
            rep["sweep"] = sweep(fx)
        if args.shells:
            rep["shells"] = shell_probes(fx)
        rep["fixture"]["notes"] = fx["notes"]
        rep = sanitiser(root)(rep)
        if args.json:
            with open(args.json, "w", encoding="utf-8") as f:
                json.dump(rep, f, ensure_ascii=False, indent=1)
        print_report(rep)
        missed = [r["id"] for r in rows if r["verdict"] != "ok"
                  or r.get("write", {}).get("verdict", "ok") != "ok"]
        missed += [c["spelling"] for c in rep["create"] if c["verdict"] != "ok"]
        missed += [r["spelling"] for r in rep.get("shells", {}).get("rows", [])
                   if r["verdict"] != "ok"]
        print(f"\nclassifier misses: {', '.join(missed) or 'none'}")
        return 1 if missed else 0
    finally:
        os.chdir(os.path.dirname(root))
        if not args.keep:
            literal = "\\\\?\\" + root  # names with trailing dots or spaces need the prefix
            for dirpath, dirnames, _ in os.walk(literal):
                for d in list(dirnames):
                    full = os.path.join(dirpath, d)
                    if os.path.islink(full) or os.path.isjunction(full):
                        os.rmdir(full)
                        dirnames.remove(d)
            shutil.rmtree(literal)


if __name__ == "__main__":
    sys.exit(main())
```
