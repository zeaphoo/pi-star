# UI upstream

Copied from the read-only pi-web checkout at `/home/ubuntu/workspace/pi-star/pi-web`.

- Source project: https://github.com/agegr/pi-web (MIT)
- Copied client modules: 230
- Re-copy: `node scripts/sync-ui-from-pi-web.mjs`
- Do not copy `app/api/**` or server-only `lib/**` modules; those are implemented in Rust.
