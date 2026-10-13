# Enrolling a security key for the CSB login

Committee members log in to the CSB section with a security key (a YubiKey
with a PIN). Keys are registered in advance: the administrator enrols each
key with the committee member present, using the script below, and puts the
resulting line in the `CSB_WEBAUTHN_USERS` environment variable. There is no
registration page in e-KS.

## Why in person

The line the script prints holds no secret: the private key never leaves the
security key. It does decide who can log in, though: whoever holds the key it
names logs in under its username. A line sent by mail or chat could be
swapped on the way for one naming another key. Enrolling in person closes
that gap: the administrator sees the member's own physical key produce the
line, on the administrator's machine, and takes it straight into the
configuration.

## What you need

- The committee member, in person, with their security key. The key needs a
  PIN, which the member sets and enters themselves; the administrator never
  learns it. Without a PIN enrolment fails. Set one with
  `fido2-token -S <device>` or `ykman fido access change-pin`.
- On a key that supports it, the "user verification always" option (CTAP
  2.1 `alwaysUv`) must be on: the key then demands the PIN for every
  operation, enrolment and login alike, whatever the requesting page asks
  for. The script refuses a key that has the option but leaves it off. The
  member enables it, entering the PIN, with

  ```sh
  fido2-token -S -u <device>
  ```

  `fido2-token -I <device>` shows the state on its `options` line:
  `alwaysUv` when on, `noalwaysUv` when off. A key whose options list
  neither has no such option (YubiKeys with firmware before 5.7; enabling
  fails with "config_always_uv: option not found") and is enrolled as is.
  The login then still demands the PIN, because the login page requests
  user verification and the server rejects an assertion without it.
- The libfido2 command-line tools on the administrator's machine:
  `apt install fido2-tools` (Debian/Ubuntu), `brew install libfido2` (macOS).
  On Linux the key must be accessible without root; libfido2 ships udev
  rules for that (`70-u2f.rules`).
- The host of the CSB origin, i.e. `CSB_WEBAUTHN_ORIGIN` without the scheme:
  `csb.example.nl` for `https://csb.example.nl`, `localhost` for a
  development server. A key enrolled for one host only works there.
- The username agreed with the member: 1 to 64 letters, digits, `.`, `-`,
  `_` or `@`. It appears in the audit log.

## Enrol

The member plugs their key into the administrator's machine, and the
administrator runs, from the repository root:

```sh
bin/enrol_csb_user alice csb.example.nl
```

With more than one key attached, add the device path from `fido2-token -L`
as third argument. The member enters the PIN when asked and touches the key
when it blinks. The script prints one line:

```
alice:zGJB3Yi...:MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE...
```

That is `username:credential-id:public-key`, the blobs base64url without
padding, so the line pastes into a shell, `.env` or YAML file as is. It need
not be kept secret, but it goes from this terminal into the configuration,
never through mail or chat.

The credential is non-discoverable (not a "passkey" in the key's own
storage): the key stores nothing and derives the private key from the
credential id when the login page presents it, which is why the page lists
every configured credential. That is deliberate. The key keeps no trace of
e-KS, takes up none of its limited passkey slots, and revoking a key is
removing its line: there is nothing to delete from the key itself.

## Configure

The administrator adds the line to `CSB_WEBAUTHN_USERS`, comma-separated
when there are several, and restarts e-KS:

```sh
CSB_WEBAUTHN_ORIGIN=https://csb.example.nl
CSB_WEBAUTHN_USERS=alice:zGJB3Yi...:MFkw...,bob:5Qk1Lx...:MFkw...
```

A production build also requires `CSB_BIND_ADDRESS`: the login is only
served on the dedicated CSB listener, where `CSB_IP_ALLOW_LIST` can gate it.

Startup refuses the whole list on a malformed entry, a username listed
twice, or a key listed for two users, so a typo cannot silently drop a user.
To revoke a key, remove its line. To replace a lost key, enrol the new one in
person the same way and replace the line. A member with two keys gets two
lines with different usernames.

## Without the script

The script only wraps `fido2-cred`. By hand, after checking that
`fido2-token -I /dev/hidraw5` does not list `noalwaysUv` under `options`:

```sh
{ head -c 32 /dev/urandom | base64; echo csb.example.nl; echo alice; head -c 32 /dev/urandom | base64; } > param
fido2-cred -M -i param /dev/hidraw5 | fido2-cred -V -v -o cred
```

The first and last input lines are the client data hash and the user id,
which `fido2-cred` requires but which play no further role: the key stores
neither for a non-discoverable credential. `cred` then holds the credential
id (first line, base64) followed by the public key as PEM. The configuration
line is the username, the credential id and the PEM body without its header
and footer lines, separated by colons, each blob converted to unpadded
base64url with `tr '+/' '-_' | tr -d '=\n'`.
