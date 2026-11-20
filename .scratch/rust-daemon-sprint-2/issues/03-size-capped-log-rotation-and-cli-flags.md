# 03: Size-Capped Rolling File Logging and CLI Logging Flags

**What to build:**
Implement size-capped rolling file logging for `screentimed.log`:
- Rotate the log file when its size exceeds `max_size_mb` (default 10MB).
- Maintain up to `backup_count` historical files (default 5: `screentimed.log.1`, `.2`, etc.).
- Add `--log-level <LEVEL>` and `--log-console` CLI flags to `screentimed`.
- Integrate logging settings with the hierarchical config loader from Ticket 02.

**Blocked by:** 02

**Status:** open

- [ ] Implement size-capped file rolling appender in `screentimed`
- [ ] Add `--log-level` (trace, debug, info, warn, error) and `--log-console` flags to CLI parser
- [ ] Ensure non-ANSI formatting in file logs while preserving color in interactive terminal output
- [ ] Unit/integration tests verifying log file rotation and retention caps
