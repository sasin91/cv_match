JobFinder checks the job sites for you while your PC is on, matches each post with your CV, and starts the application. Free and open source (BSD 2-Clause).

## New in 0.1.1

- **Years of work.** Letters and résumés no longer guess how long you have worked. The count comes from the dates of the jobs in your profile, and you can correct it on the Setup screen. The match without an AI model uses the same number.
- If you ran 0.1.0, start the new file the same way: your database is updated the first time it opens.

## Download

- **Windows 11:** `jobfinder.exe`. Keep it where you like and double-click it. Nothing is installed.
- **Linux:** `JobFinder-x86_64.AppImage` (Ubuntu 24.04, Debian 13, Fedora 39 or newer). Make it executable and run it.

`SHA256SUMS` lists each file's checksum.

**If Windows says MSVCP140.dll or VCRUNTIME140.dll was not found,** install Microsoft's [Visual C++ Redistributable (x64)](https://aka.ms/vs/17/release/vc_redist.x64.exe) and start JobFinder again. Most PCs have it already.

**Windows may warn you the first time.** The program isn't signed with a paid certificate yet, so Edge can say it "isn't commonly downloaded" (choose **Keep**), and SmartScreen can say "Windows protected your PC" (choose **More info**, then **Run anyway**).

## The first start

JobFinder opens on **Set up JobFinder**:

1. Your data: a new database in `%APPDATA%\JobFinder` (`~/.local/share/JobFinder` on Linux).
2. An AI model: the app's own local model when your graphics card has room for a good one, otherwise an OpenAI or Claude API key.
3. Your CV, which the model reads into your profile.
4. How often the job scan runs by itself.

The Jobnet extension for Edge or Chrome is under Settings, Jobnet extension.

## Removing it

Close JobFinder and run `jobfinder.exe cleanup` from a terminal: it lists what it keeps outside its own file (the scan's schedule, downloads, the database) and asks before removing anything. Then delete the program file.
