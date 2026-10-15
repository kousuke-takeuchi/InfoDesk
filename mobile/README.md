# InfoDesk mobile

Flutter app (iOS and Android) that reads the InfoDesk sync folder written by the
Mac app. Bundle id / applicationId: `jp.infodesk.mobile`. iOS deployment
target: 15.0 (the lowest that Xcode 27 accepts for the simulator; the plugins
ask for 13.0 at most).

## Requirements

- Flutter 3.44.7 (Dart 3.12.2), Xcode 27.
- Go 1.26 or newer on `PATH` (the `tailscale` package builds its Go library in
  a native-assets build hook on the first build). Homebrew Go is at
  `/opt/homebrew/bin/go`.

Run Flutter and Xcode as arm64. If your shell runs under Rosetta
(`uname -m` prints `x86_64`), wrap the commands:

```sh
arch -arm64 /bin/bash -c 'export PATH=/opt/homebrew/bin:$HOME/develop/flutter/bin:/usr/bin:/bin:/usr/sbin:/sbin; <command>'
```

## Google OAuth client (iOS)

The iOS client id is not committed. Generate `ios/Flutter/Secrets.xcconfig`
(git-ignored) from the plist downloaded from Google Cloud Console:

```sh
tool/ios_oauth_from_plist.sh ~/Downloads/client_<id>.apps.googleusercontent.com.plist
```

It writes `GOOGLE_IOS_CLIENT_ID` and `GOOGLE_IOS_REVERSED_CLIENT_ID`, which
`Info.plist` uses for `GIDClientID` and the URL scheme. See
`ios/Flutter/Secrets.example.xcconfig` for the shape. The xcconfigs include it
with `#include?`, so a missing file is not a build error, but Google sign-in
needs it.

## Test and analyze

```sh
flutter test
flutter analyze
```

## iOS simulator

```sh
flutter build ios --simulator --debug
xcrun simctl boot <UDID>   # e.g. an iPhone 15 Pro from `xcrun simctl list devices available`
xcrun simctl install booted build/ios/iphonesimulator/Runner.app
xcrun simctl launch booted jp.infodesk.mobile
xcrun simctl io booted screenshot /tmp/infodesk.png
```

The first build takes several minutes (Swift packages and the Go build for
tailscale); later builds are faster.

### Why `ARCHS[sdk=iphonesimulator*] = arm64`

`ios/Flutter/Debug.xcconfig` and `Release.xcconfig` pin the simulator build to
arm64. Without it, `flutter build ios --simulator` fails with

    Target debug_unpack_ios failed: Exception: Binary .../Flutter.framework/Flutter
    does not contain architectures "arm64 x86_64".

although `lipo -info` lists both. flutter_tools checks the framework with
`lipo <binary> -verify_arch arm64 x86_64`. The `lipo` in Xcode 27 rejects more
than one arch after `-verify_arch` ("-verify_arch requires exactly one input
file"; it reads the second arch as a second input file), so the check fails
whenever two archs are requested. One arch passes. It is not related to Rosetta
(`lipo` behaves the same under `arch -arm64` and `arch -x86_64`) or to any
plugin (a plain `flutter create` app fails the same way).

`EXCLUDED_ARCHS` does not help: the check runs in the scheme pre-action
("Run Prepare Flutter Framework Script", `xcode_backend.sh prepare`), which gets
the unfiltered `ARCHS="arm64 x86_64"` of a generic simulator build. Setting
`ARCHS` itself is what reaches that script. The cost: no x86_64 simulator
(Intel Macs). Remove the line once Flutter or Xcode fixes the call.
