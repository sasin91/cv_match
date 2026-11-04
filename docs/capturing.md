# Screenshots and clips for the web page

`tools/capture.ps1` takes them on Windows, the same way every time. The window is set to 1280 × 800,
and each screenshot is taken in both light and dark. Files go to `site/media/`.

Your real matches are fine to show. Keep these out of the frame: your CV, your profile, the
letters and résumés, and your name, address and phone number.

## Once

- Clips need ffmpeg: `winget install Gyan.FFmpeg`.
- The phone needs adb (`winget install Google.PlatformTools`) and USB debugging turned on. Plug it
  in, accept the prompt on the phone, and pair it with the PC (Settings → Devices).
- If PowerShell refuses to run the script, run it for this session only:
  `powershell -ExecutionPolicy Bypass -File .\tools\capture.ps1 ...`

## The desktop

Start JobFinder, open the screen, and from the repository folder:

| Name | Screen | Command |
| --- | --- | --- |
| dashboard | Match, with today's matches | `.\tools\capture.ps1 shot dashboard` |
| saved-matches | Saved matches, Shortlist | `.\tools\capture.ps1 shot saved-matches` |
| match-why | A match's requirements and verdicts, scrolled to the top, application panel closed | `.\tools\capture.ps1 shot match-why` |
| job-searches | Job searches | `.\tools\capture.ps1 shot job-searches` |
| task-manager | Task manager during a scan | `.\tools\capture.ps1 shot task-manager` |
| notifications | The bell's drawer open, with a few notifications | `.\tools\capture.ps1 shot notifications` |
| learned-terms | Settings → Learned terms, with a term open | `.\tools\capture.ps1 shot learned-terms` |
| devices | Settings → Devices, with the phone paired | `.\tools\capture.ps1 shot devices` |

Clips (about 10 seconds, in the look Windows has now). Start the command, then click through in
the window when the countdown ends:

| Name | What to do | Command |
| --- | --- | --- |
| scan | Press Scan now and let the task manager fill | `.\tools\capture.ps1 clip scan` |
| match-why | Open a saved match and scroll its verdicts | `.\tools\capture.ps1 clip match-why` |

## The phone

| Name | Screen | Command |
| --- | --- | --- |
| matches | Matches, Shortlist | `.\tools\capture.ps1 phone-shot matches` |
| match | One match: score ring and requirements | `.\tools\capture.ps1 phone-shot match` |

`-Theme light` or `-Theme dark` takes one look only. `-Out <folder>` saves somewhere else.
