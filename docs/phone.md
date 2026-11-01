# Using the app from your phone

The app listens only on 127.0.0.1, so only the PC itself can reach it. To open it on your phone,
at home or away, you use [Tailscale](https://tailscale.com): your phone and the PC join a private
network that only your devices are on. `tailscale serve` gives the PC an HTTPS address on that
network and passes each request on to the app on 127.0.0.1:8765.

Your data stays on the PC. The traffic between your phone and the PC is encrypted end to end
(WireGuard). It usually goes straight between the two devices, and when it can't, Tailscale's
relays forward it still encrypted. Tailscale's servers only see which devices are on your network.

Neither the phone nor the PC needs a port opened on the router, and nothing is published to the
internet.

## Once

1. Install Tailscale on the PC and on the phone, and sign in to the same account on both.
2. In the [Tailscale admin console](https://login.tailscale.com/admin/dns), keep **MagicDNS**
   on and turn on **HTTPS Certificates**. The PC's name then looks like `pc.tailnet-name.ts.net`.
   You can find it on the [Machines](https://login.tailscale.com/admin/machines) page or with
   `tailscale status`.

   Turning on HTTPS lists that name in the public certificate logs. The name is listed but
   nothing behind it can be reached, because only your devices are on the network.
3. In a terminal on the PC, run:

       tailscale serve --bg http://127.0.0.1:8765

   `--bg` keeps it running after the terminal closes and after restarts. `tailscale serve status`
   shows what it serves, and `tailscale serve reset` stops it.

   Use `serve`, never `funnel`, which would put the app on the public internet.
4. Start the app with your PC's name:

       jobfinder.exe --tailscale-name pc.tailnet-name.ts.net

   Put the same flag in the shortcut or Task Scheduler task that starts it at login. The app then
   prints the address to open on your phone.

## On the phone

With Tailscale switched on, open `https://pc.tailnet-name.ts.net/match`. On Android you can
use **Add to Home screen**, and on iPhone **Share → Add to Home Screen**, to open it like an app.

The PC has to be on with jobfinder.exe running.

## Why `--tailscale-name` is needed

So that a web page open in a browser on the PC can't use the app, the app answers only requests
whose Host header is its own address (against DNS rebinding). It accepts changes only from pages
whose Origin is its own page or the Jobnet extension. `tailscale serve` keeps the phone's Host
(`pc.tailnet-name.ts.net`) when it passes a request on, so the app has to be told that this name
is its own too. Without the flag, the app turns away the phone's requests with "open it at its
own address."
