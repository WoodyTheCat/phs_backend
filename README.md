[![Cargo Build & Test](https://github.com/WoodyTheCat/phs_backend/actions/workflows/rust.yml/badge.svg)](https://github.com/WoodyTheCat/phs_backend/actions/workflows/rust.yml)

# TODOS
- SSL fallback
- In-memory or Redis caching for dynamic pages
- Add rate limiter for logged in users - early warning
- Restrict CORS
- Currently, auth sessions from before a restart are invalidated as the server picks a new signing key for cookies
