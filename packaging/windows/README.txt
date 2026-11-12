ESCPost for Windows x64
======================

Windows 11 x64 is the initial supported platform. Rust, Python, Bun, and Docker
are not needed to run this distribution. The web app, renderer, fonts, and
printer profiles are embedded in escpost.exe.

Installer: run the setup executable. Installation is per user and does not
request administrator privileges. "Add ESCPost to my PATH" is checked by
default. Open a NEW terminal after installation to use the escpost command.

Portable ZIP: extract the entire archive and run .\escpost.exe from PowerShell
in that folder. It does not change PATH or add Start-menu shortcuts.

Start the workbench from the Start menu, or run:

    escpost serve --listen --web-listen

Keep its console open while using the workbench. Stop it with Ctrl+C.
The web app opens in your browser during an interactive run. Both listeners
bind to localhost by default. Use escpost --help for other commands.

Unsigned downloads
------------------
This release is unsigned. Edge may report an uncommon download. Windows
SmartScreen may display "Windows protected your PC" and an unknown publisher.
After checking that you downloaded the release from our official project,
personal PCs may offer "More info" and "Run anyway". Organization policies can
prevent continuation. Windows 11 Smart App Control can block unsigned apps
without an individual-app override. The ZIP is subject to security checks too.
Do not disable system-wide security protections to run ESCPost.

Release checksums detect a damaged or changed download; they do not replace
a verified publisher signature. A new unsigned version may show warnings again.

Printing and configuration
--------------------------
RAW TCP and direct USB printing are included. On Windows the USB interface
needs WinUSB; replacing its vendor driver can disrupt ordinary Windows printing.
The installer does not install or replace printer drivers. Printing through
existing Windows printer queues (the Windows spooler) is not supported yet.

Configuration defaults to %APPDATA%\receiptful\escpost\config\ and is preserved
on upgrades and uninstall. Uninstall removes only the PATH entry it added.
USB hardware support requires validation for the printer and its driver.

Documentation: https://escpost.dev
Source and issues: https://github.com/receiptful/escpost

Bundled assets
--------------
Noto Sans Mono: https://github.com/notofonts/latin-greek-cyrillic
SIL Open Font License 1.1; see licenses/NotoSansMono-OFL.txt.
Printer capability data derives from Mike Billington and contributors'
https://github.com/receipt-print-hq/escpos-printer-db, with ESCPost enrichment
and generated profiles. CC BY 4.0; see licenses/escpos-printer-db-LICENSE.md.
