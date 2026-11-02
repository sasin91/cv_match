# Using the app from your phone

Not yet. JobFinder is its desktop window now: the web pages a phone could open over Tailscale
(`tailscale serve` and `--tailscale-name`) are gone, and the app's server on 127.0.0.1:8765
answers only the Jobnet extension and the app itself.

A remote-control app for the phone is planned. Until then, if you set up `tailscale serve` for
JobFinder before, stop it on the PC with:

    tailscale serve reset

and take `--tailscale-name` (and `--no-browser`) out of any shortcut or Task Scheduler task that
starts the app. The app ignores them and says so in `jobfinder.log`.
