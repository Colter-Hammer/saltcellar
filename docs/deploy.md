# Deploying

Saltcellar is one container: the `budget` binary serving the API and the built frontend, with everything it keeps in `/data`. The repo's [`compose.yaml`](../compose.yaml) is an example to start from. Any specific home-server setup belongs in your own config, not in this repo.

## Run it

```sh
docker compose up -d --build
docker compose ps        # STATUS shows (healthy) once the database answers
```

The app is then at <http://localhost:8080>, on this machine only. Other devices reach it through your reverse proxy (below).

**Claim it.** A fresh install shows only the setup wizard, which asks first for the setup code. Find it in the log (`docker compose logs budget | grep "Setup code"`); a new one is printed each time the app starts, until setup is finished, when it stops working. The wizard then asks for the household currency and timezone and the first login.

- **Image.** `compose.yaml` builds from source. To run an image you built or pulled instead, set `BUDGET_IMAGE` (for example `BUDGET_IMAGE=saltcellar:1.0 docker compose up -d`).
- **Port.** `BUDGET_PORT` changes the localhost port (default 8080). Inside the container the app always listens on 8080.
- **Data.** The SQLite database lives in the `data` volume, mounted at `/data`. To use a folder on the host instead, swap the volume for a bind mount (`./data:/data`). The folder must be writable by uid 65532, the image's non-root user: `sudo chown 65532:65532 ./data`.
- **Health.** The image's `HEALTHCHECK` runs `budget healthcheck`, which asks `GET /api/v1/health` and passes only on a 200. That endpoint answers 200 when the database answers.
- **Logs.** `docker compose logs budget`: one line per request, never amounts, payees or secrets. The setup code is the one secret printed, on purpose.

## Environment

Only what's needed before anyone can log in, or what touches the host. Everything else is in the app's Settings. Startup fails with a message naming the variable if one is missing or invalid.

| Variable | Default | Meaning |
| --- | --- | --- |
| `BUDGET_DATA_DIR` | `/data` | Holds the SQLite file and, by default, backups. |
| `BUDGET_LISTEN` | `0.0.0.0:8080` | Listen address inside the container. |
| `BUDGET_BASE_URL` | none (required from milestone 6) | The public URL of the app, like `https://budget.example.com`, used in invite, reset and notification links. |
| `BUDGET_AUTH_MODE` | `builtin` | `builtin` (passwords and sessions) or `header` (trust a header from your proxy). |
| `BUDGET_TRUSTED_PROXY` | none | Header mode: the proxy address (IP or CIDR) whose header is trusted. Required in header mode. |
| `BUDGET_AUTH_HEADER` | `Remote-User` | Header mode: the header carrying the username. |
| `BUDGET_BACKUP_DIR` | `$BUDGET_DATA_DIR/backups` | Where nightly backups are written. |

`compose.yaml` passes `BUDGET_BASE_URL` through from your shell or a `.env` file next to it. Add the others under `environment:` as you need them.

## Behind an HTTPS reverse proxy

Run it behind any HTTPS reverse proxy, on your home network or a private VPN. It isn't meant for the public internet. HTTPS isn't optional: login cookies are `Secure`, and the installable app (milestone 6) needs it. Only `localhost` works over plain HTTP.

A minimal [Caddy](https://caddyserver.com/) site on the same host, with Caddy getting the certificate itself:

```caddyfile
budget.example.com {
	reverse_proxy 127.0.0.1:8080
}
```

Use your own domain in place of `budget.example.com`, and set `BUDGET_BASE_URL` to the same URL.
