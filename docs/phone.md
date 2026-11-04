# Your phone or tablet

A JobFinder app for the phone (or a tablet) is on its way. It will show your shortlist and send
job posts to the computer to evaluate. What is here already is the computer's side: the
**Devices** screen under Settings, which pairs a device.

## How a device reaches the computer

Through [Tailscale](https://tailscale.com), from wherever the device is. JobFinder listens for
devices on `127.0.0.1:8766` only. `tailscale serve` passes `https://<computer>.<tailnet>.ts.net:8766`
on to it, with a certificate the device trusts. Nothing goes through any other server, and
JobFinder never uses `tailscale funnel`, which would put it on the open internet.

JobFinder answers there only at the computer's tailnet name, and, apart from pairing, only a
paired device's token. Other devices on your tailnet and other programs on the computer can reach
the port, but they get nothing without a token.

## Setting it up once

1. Install Tailscale on the computer and on the device, and sign in to both with the same account.
2. In the Tailscale admin console, under DNS, turn on MagicDNS and HTTPS certificates. Note that
   a name with a certificate is listed in public certificate logs, so the computer's tailnet name
   is public. Nothing else is.
3. In JobFinder, open Settings, Devices and press **Set up tailscale serve**. It runs:

       tailscale serve --bg --https=8766 http://127.0.0.1:8766

   which stays set up after a restart. To undo it: `tailscale serve --https=8766 off`.

## Pairing a device

Press **Show a pairing code** on the Devices screen and scan the QR code with the JobFinder app on
the device. A code works once, for five minutes. The device then gets a token of its own. The
computer keeps only its hash, sealed in the database's secrets table with the key file beside
the database. The Devices screen lists each paired device with when it was last seen. **Remove**
stops its token at once.

## What a paired device can do

Each request is a `POST` with JSON, and the device's token as `Authorization: Bearer <token>`.
The shapes are in `app/device-protocol`.

| Path | What it does |
| --- | --- |
| `/v1/device/hello` | Says which JobFinder version the device is paired with. |
| `/v1/device/matches` | A batch of a list of saved matches, the shortlist when no `list` is named. |
| `/v1/device/match` | One saved match: score, job, and each requirement with its verdict. |
| `/v1/device/evaluate` | Matches a job post (a link, its text, or its page as the phone showed it) with your profile, as the Match screen would. It shows in the task manager as "From <device>: …" and is saved like any match. Without an AI model it is the free match, which isn't saved. |
| `/v1/device/evaluations` | What became of the posts this device sent since JobFinder started. |

## From before

If you set up `tailscale serve` for the old web pages on port 443, stop that with
`tailscale serve reset`, and take `--tailscale-name` out of any shortcut or task that starts
JobFinder. The app ignores it and says so in `jobfinder.log`.
