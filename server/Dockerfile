# The server image, and the shortest Dockerfile that could be correct.
#
# There is no build stage. The binary is compiled by the job in
# `.github/workflows/release.yml` that builds this image, and the build context
# handed to `docker build` is a directory that job creates and puts exactly one
# file into. That is deliberate and it is the same rule the release archives
# follow: `docs/RISKS.md` R3 and R4 are about a recording, a consent record or a
# signing key leaving this machine by accident, and the way that happens is a
# tool that packages a directory rather than a list of files. `COPY . .` over a
# working tree is exactly that tool.
#
# A multi-stage build compiling from source would be the conventional shape and
# it is the shape this file refuses: its first stage would have to copy the
# whole workspace into the daemon's context, which is the glob above with more
# steps.

# Distroless: a C runtime, CA certificates, timezone data, and nothing else. No
# shell, no package manager, no `curl` — so an attacker who reaches code
# execution in the server finds no tools in the image to go anywhere with.
# `cc` rather than `static` because the server is a `gnu` build and links glibc,
# `libgcc_s` and `libm` and nothing else; `nonroot` is the variant whose default
# user is uid 65532.
#
# `debian13` rather than `debian12`, and the reason is a failure mode that only
# appears at run time. The binary is compiled on GitHub's `ubuntu-latest`, which
# is a newer glibc than Debian 12 carries, and glibc's compatibility runs one
# way: a binary linked against 2.39 asks for versioned symbols a 2.36 runtime
# does not have, and the image builds, publishes and pushes perfectly before
# failing the first time somebody runs it. Debian 13's glibc is newer than the
# builder's, which makes the question go away rather than leaving it to the
# runner image's next upgrade.
#
# Pinned by digest with the tag beside it, for the reason `docs/RISKS.md` R12
# gives about action tags: `nonroot` is a mutable pointer, and an image that
# moves under a running release pipeline is the same failure as an action that
# does. Renovate's `docker` manager keeps this digest current alongside the
# action SHAs.
FROM gcr.io/distroless/cc-debian13:nonroot@sha256:d97bc0a941b8d4be647dc0ee75b264ddbb772f1ac5ba690a4309c00723b23775

# The one file in the context.
COPY moba-server /usr/local/bin/moba-server

# Non-root, stated numerically rather than by name. The base image already sets
# this user; repeating it here means a base-image change that dropped it would
# not silently promote this server to root, and the release workflow reads this
# value back out of the built image and fails if it is 0.
USER 65532:65532

# No `EXPOSE`, and this is a statement about the server rather than an omission.
# `server/src/main.rs` binds `127.0.0.1:0` — loopback, on a port the kernel
# picks — and prints the address and the certificate it generated so that a
# client can be told what to trust. Inside a container that means the server is
# reachable from inside that container and nowhere else, and an `EXPOSE` line
# would be documentation of a port that does not exist. Publishing a reachable
# server is a change to `Listener::bind`, not to this file; until then this
# image is a way to run the authority reproducibly, not a way to host it.
ENTRYPOINT ["/usr/local/bin/moba-server"]
