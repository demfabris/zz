# Security

## Supported versions

The latest release and the tip of `main`. Nothing older gets fixes.

## Reporting

Use GitHub's private vulnerability reporting: the Security tab of this
repository, then "Report a vulnerability". Do not open a public issue for
anything exploitable.

Expect a first reply within a week. If a report turns out to be a plain bug,
we'll say so and move it to a public issue.

## Attack surface worth knowing about

zz embeds CEF/Chromium and can import Chrome cookies out of the OS keychain, so
browser-profile handling and the cookie import path move secrets around on your
behalf . bugs there are security reports, not feature requests. The same goes
for the daemon's control socket and anything that lets a pane's contents reach
another pane.
