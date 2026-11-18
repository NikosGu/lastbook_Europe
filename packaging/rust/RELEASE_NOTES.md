**Preview** of Lastbook Europe, built by GitHub Actions from this repository.

Lastbook Europe is an independent European version of [Lastbook](https://github.com/Immortal-Protocols/lastbook), the offline library made by the Lastbook contributors for the United States. Its authors did not make this version and are not responsible for it: what was changed is in the [README](https://github.com/NikosGu/lastbook_Europe#lastbook-europe-and-lastbook).

| System | Download | Install |
|---|---|---|
| macOS 13 or later (Apple silicon) | `LastbookEurope-macos-arm64.dmg` | Open the disk image and drag **Lastbook Europe** to Applications. The app is signed ad hoc, not with a paid Apple Developer ID, so the first time macOS asks: on macOS 15 and later open **System Settings → Privacy & Security** and choose **Open Anyway**; on macOS 13 and 14, Control-click the app and choose **Open**. |
| Windows 10 and 11 (x64) | `LastbookEurope-windows-x64-setup.exe` | Run it. If SmartScreen warns, choose **More info → Run anyway** (the build is not signed yet). No administrator rights needed. A `.zip` to unpack anywhere is there too. |
| Linux (x86-64, Debian, Ubuntu and similar) | `LastbookEurope-linux-amd64.deb` | `sudo apt install ./LastbookEurope-linux-amd64.deb`, then open **Lastbook Europe** from the menu. A `.tar.gz` is there too; it needs WebKitGTK 4.1. |

What works in this preview: the app; setup, with the choice of country and region, every download checked against its published SHA-256, and a library or model file you already have; the starter library (19 public documents in six languages, from the data release [data-v1](https://github.com/NikosGu/lastbook_Europe/releases/tag/data-v1)); the maps and address search of the regions built so far, with more added as they are built (setup offers those ready); and the local AI, with a model downloaded in setup or a file already on the computer.

Problems or questions: write to nikos.gu.devs@gmail.com, or open an issue on GitHub.

Lastbook Europe never sends anything about you anywhere: it uses the network only when you ask setup to download, and only from its own download addresses.
