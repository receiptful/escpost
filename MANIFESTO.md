# The ESCPost Manifesto

Existing developer tools for thermal printing are fragmented: printing
utilities are often limited to one operating system, while printing frameworks
require a particular programming language. Developers should be free to choose
their operating system, programming language, and application stack when
integrating thermal printing.

We want thermal printing to be plug-and-play rather than trial and error.
Finding a printer should not mean hunting for its IP address or configuring a
USB connection by hand. A small change to a long receipt should not require
printing it again in full. That is why ESCPost automatically discovers
available printers and previews receipts in the browser before paper is
consumed.

We want developers and teams to choose how they work. Every print workflow
should be automatable through a fast, cross-platform CLI and accessible through
the integrated web app for people who prefer a visual interface.

We want debugging and print automation to work locally, without a mandatory
user account or internet connection. Our optional paid cloud services at
[receiptful.io](https://receiptful.io) extend the ESCPost toolbox and fund its
continued development without making the free local tools dependent on them.

We want printer knowledge to flow both ways between ESCPost and its community.
Printer differences are real: paper widths reshape layouts, line feeds vary,
and a wrong code page corrupts currency signs and umlauts. No team can test
every printer, but every printer in the community can make ESCPost better. By
contributing real print jobs, profiles, edge cases, vendor quirks, and
photo-based calibrations, developers and system maintainers turn isolated
discoveries into reliable support for everyone using the same hardware.
