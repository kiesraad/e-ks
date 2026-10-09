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
alice:Qm1kbm9...:zGJB3Yi...:MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE...
```

That is `username:user-handle:credential-id:public-key`, the blobs base64url
without padding, so the line pastes into a shell, `.env` or YAML file as is.
It need not be kept secret, but it goes from this terminal into the
configuration, never through mail or chat.

## Configure

The administrator adds the line to `CSB_WEBAUTHN_USERS`, comma-separated
when there are several, and restarts e-KS:

```sh
CSB_WEBAUTHN_ORIGIN=https://csb.example.nl
CSB_WEBAUTHN_USERS=alice:Qm1kbm9...:zGJB3Yi...:MFkw...,bob:dXNlcm...:5Qk1Lx...:MFkw...
```

A production build also requires `CSB_BIND_ADDRESS`: the login is only
served on the dedicated CSB listener, where `CSB_IP_ALLOW_LIST` can gate it.

Startup refuses the whole list on a malformed entry, a username listed
twice, or a key listed for two users, so a typo cannot silently drop a user.
To revoke a key, remove its line. To replace a lost key, enrol the new one in
person the same way and replace the line. A member with two keys gets two
lines with different usernames.

## Without the script

The script only wraps `fido2-cred`. By hand:

```sh
user_handle=$(head -c 32 /dev/urandom | base64)
{ head -c 32 /dev/urandom | base64; echo csb.example.nl; echo alice; echo "$user_handle"; } > param
fido2-cred -M -i param /dev/hidraw5 | fido2-cred -V -v -o cred
```

`cred` then holds the credential id (first line, base64) followed by the
public key as PEM. The configuration line is the username, the user handle,
the credential id and the PEM body without its header and footer lines,
separated by colons, each blob converted to unpadded base64url with
`tr '+/' '-_' | tr -d '=\n'`.
