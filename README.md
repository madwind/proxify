# Proxify

Rust HTTP proxy listening on a Unix socket. `PROXY_PATH` defaults to
`/api/proxy/file`; `SOCKET_PATH` defaults to `/run/proxify/proxify.sock`.

## JWT verification

From `1.1.10`, authentication requires an Ed25519 public key. Set
`JWT_PUBLIC_KEY_FILE` to a mounted SubjectPublicKeyInfo PEM file (`BEGIN PUBLIC KEY`)
matching Cinema's Ed25519 signing private key. Missing or invalid key configuration
prevents startup; authentication cannot be disabled by omitting the key.

Cinema alone holds the private key and signs EdDSA JWTs. Proxify holds only the
public key and verifies the token supplied through the existing `token` query
parameter. It accepts only EdDSA, requires an unexpired `exp` and a nonempty string
`sub`, validates `nbf` when present, uses no clock leeway, and limits tokens to
8 KiB. Unsupported JOSE `crit` and `b64` headers are rejected. Issuer and audience
claims are not required.

Deploy this version together with Cinema server `1.0.90` or newer. Old HS256,
HS384 and HS512 tokens are rejected; users must sign in again. The old
`JWT_SIGNING_KEY` setting is unused. Mount only the public key in Proxify, never
the signing private key, and keep keys out of Git and container images.

Generate the pair on the operator's machine outside the repositories:

```sh
umask 077
openssl genpkey -algorithm ED25519 -out cinema-jwt-private.pem
openssl pkey -in cinema-jwt-private.pem -pubout -out cinema-jwt-public.pem
```

Mount the private file into Cinema and configure its `JWT_PRIVATE_KEY_FILE`;
mount the public file into Proxify and configure `JWT_PUBLIC_KEY_FILE`. Synchronize
service clocks. Rotate by replacing the matching files and restarting both services;
only one public key is accepted, so previous tokens become invalid.
