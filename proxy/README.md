# Idleview photo proxy

A Cloudflare Worker that holds the Unsplash access key so the desktop app never has to.

## Why

A key compiled into the app, or stored in a settings file on the user's machine, is not
hidden — the binary has to reconstruct it to call Unsplash, so anyone with the installer
can too. Distributing it also violates Unsplash's API terms and puts every install on one
revocable credential. So the key lives here, on a server users cannot read.

