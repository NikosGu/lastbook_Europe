**Preview build** of Lastbook Europe, made by GitHub Actions from this branch. It is a draft until the maintainer publishes it.

| System | Download | Install |
|---|---|---|
| macOS (Apple silicon) | `LastbookEurope-*-macos-arm64.dmg` | Open the disk image and drag **Lastbook Europe** to Applications. The first time, Control-click the app and choose **Open** (the build is not signed with an Apple Developer ID yet). |
| Windows 10 and 11 (x64) | `LastbookEurope-*-windows-x64-setup.exe` | Run it. If SmartScreen warns, choose **More info → Run anyway** (the build is not signed yet). No administrator rights needed. A `.zip` to unpack anywhere is there too. |
| Linux (x86-64, Debian, Ubuntu and similar) | `lastbook-europe_*_amd64.deb` | `sudo apt install ./lastbook-europe_*_amd64.deb`, then open **Lastbook Europe** from the menu. A `.tar.gz` is there too; it needs WebKitGTK 4.1. |

What works in this preview: the app, its setup (country and region choice, downloads checked against their published SHA-256, a library you already have), and the local AI runner. The European library packs, region maps and models are not published yet, so setup cannot download a library from this build.

Lastbook never sends anything about you anywhere: it uses the network only when you ask setup to download, and only from its own download addresses.
