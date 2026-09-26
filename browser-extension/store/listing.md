# Chrome Web Store listing

The text of the store listing, kept here so it changes with the code. The store
shows the description as plain text, thus the block below is what to paste:
no markdown, and the line breaks and bullets are what the reader sees.

## Name

ESCPost Thermal Printer

## Short description (132 characters)

Print receipts and labels to your thermal printer directly from your browser

## Detailed description (16,000 characters)

```text
Print receipts and labels straight from the web app you already use.

A browser cannot talk to a thermal printer. The usual way around that is the system print dialog, which asks for a printer, a paper size and a confirmation every single time, and still turns your receipt into a page of A4. ESCPost takes that whole detour away. Your web app sends the receipt, the printer cuts it. No dialog, no paper size, no extra click.

WHAT YOU GET

• One tap, paper out. The receipt prints the moment your web app asks for it. Nothing appears on screen and nothing waits for a confirmation.
• The printer you already own. ESCPost speaks ESC/POS, the language of thermal receipt and label printers, over USB or over the network.
• Exactly the layout you designed. Your bytes reach the printer unchanged, so fonts, logos, barcodes, cash drawer kicks and the cut come out as intended.
• No driver hunting. No printer driver, no port setup, no vendor tool.
• No certificate, no yearly fee. Nothing to buy, nothing to sign, nothing to renew.

WHO IT IS FOR

Anyone whose counter runs on a web app: shops, restaurants, cafés, bakeries, market stalls, pharmacies, workshops, warehouses. If your point of sale, order system or label tool runs in a browser tab, it can print with ESCPost.

If you build that software, the page-side library is @receiptful/escpost. Two lines of code print a receipt, and printers can be listed and watched live.

HOW IT WORKS

1. Install ESCPost on the computer the printer is attached to, from https://escpost.dev. The extension points you there on first use.
2. Open your web app and allow it to print. You do this once per website.
3. Print. That is the whole setup.

WHAT IT IS ALLOWED TO DO

ESCPost prints for the websites you approve, and for no others. Each website is approved by you, by name, in the extension's popup, and you can withdraw that approval at any time. An unapproved site gets nothing: no printer list, no printing, no answer at all.

Your receipts stay on your own computer. They go from the page to the extension to the ESCPost app on the same machine, and from there to the printer. Nothing is uploaded, no account is needed, and the extension collects no analytics.

OPEN SOURCE

ESCPost is open source under the Apache 2.0 licence. The extension, the app and the page library are all public, so anyone can read exactly what talks to their printer.

Home: https://escpost.dev
Source: https://github.com/receiptful/escpost
```

## Single purpose

Plain text, one field in the dashboard:

```text
Websites the user approves can print receipts and labels on a thermal printer attached to the user's own computer. The website sends the print data to the extension, the extension passes it to the ESCPost application on the same computer, and that application sends it to the printer. Nothing else: no other feature, no access to sites the user has not approved, and nothing sent off the computer.
```

## Permission justifications

One field each in the dashboard, plain text, 1,000 characters each.

### storage

```text
Stores two small values on the user's own computer with chrome.storage.local: the list of website origins the user has approved for printing, and the address of the local ESCPost application once it has been found. The approved-origin list is the permission check itself. The bridge reads it for every request, so a site the user has not approved gets no printer list and no printing. Without storage the user would have to approve the site again on every page load. Storing the application address avoids probing ports on each request. No browsing history, no page content, no personal data, and nothing that leaves the computer.
```

### activeTab

```text
The popup is the place where the user approves or withdraws printing for a website, so it has to name the site the user is looking at. activeTab gives the address of the tab in front only while the user has the popup open, which is exactly when that is needed. It is used to read the origin of the current tab and to show it, then to store or remove that origin when the user clicks the button. Nothing is injected into the page, no page content is read, and no other tab is touched.
```

### Host permission

```text
The extension prints by calling the ESCPost application on the user's own computer, which listens on 127.0.0.1. Only loopback addresses are requested: http://127.0.0.1:9000/* through http://127.0.0.1:9009/*. No public host, no remote server, no wildcard. The range exists because the application takes the first free port when 9000 is already in use, so the extension probes the range to find it. 127.0.0.1 is reachable only from the same computer, thus these permissions grant no access to any website and no access to the network. All print data goes to that local application and nowhere else.
```
