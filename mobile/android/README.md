# JobFinder for Android

The phone and tablet app. It pairs with JobFinder on your computer over Tailscale (see
[docs/phone.md](../../docs/phone.md)), shows your saved matches with the shortlist first, opens a
match with each requirement and its verdict, and sends job posts to be matched. You can paste a
link or text, or share a post to JobFinder from your browser.

## Getting the app

Every pull request and push to `main` that touches `mobile/android` builds a debug APK. Open the
run under **Actions → android** and download `jobfinder-android-debug`. Unzip it and install
`app-debug.apk` on the device (allow installs from your browser or file manager when Android asks).

## Building it yourself

You need JDK 17 and the Android SDK, or Android Studio, which brings both. Then run:

    cd mobile/android
    ./gradlew :protocol:test :app:assembleDebug

`protocol/` holds what the app and the computer send each other, plus the client that sends it. It is
plain Kotlin, and it mirrors `app/device-protocol` in this repo, so change both together.

## How the token is kept

When the device pairs, the computer gives it a token. The app encrypts it with an AES key kept in
the Android keystore, which can't be copied off the device, and the app has backups turned off.
**Unpair this device** forgets it. The computer keeps listing the device until you remove it
under Settings, Devices.
